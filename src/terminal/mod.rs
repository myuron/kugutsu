//! PTY 上でシェルを動かすターミナルのモデル。

pub mod colors;
pub mod content;
pub mod keys;
mod listener;
pub mod process;
pub mod size;

use std::borrow::Cow;
use std::fs::File;
use std::io;
use std::sync::Arc;
use std::thread;

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, Notify, OnResize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::tty::{self, Options, Shell};
use futures::channel::mpsc::{self, UnboundedReceiver};

use content::Snapshot;
use listener::Listener;
use process::ProcessInfo;
use size::GridSize;

/// 起動するプログラム。`None` なら `$SHELL`（未設定なら passwd に登録されたシェル）。
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    pub program: Option<(String, Vec<String>)>,
}

/// ビューが反応すべきターミナルのイベント。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// 画面内容が変わった。
    Wakeup,
    Title(String),
    ResetTitle,
    /// プロセスが終了したか、PTY の I/O スレッドが止まった。
    Exit,
}

/// PTY と接続されたターミナル。
pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    notifier: Notifier,
    size: GridSize,
    /// フォアグラウンドのプロセスを調べるために複製した PTY の master 側。
    master: File,
}

impl Terminal {
    /// PTY を作成してプログラムを起動する。返り値の受信側にはターミナルからのイベントが届き、
    /// それぞれ [`Terminal::handle_event`] に渡す必要がある。
    pub fn spawn(
        options: SpawnOptions,
        size: GridSize,
    ) -> io::Result<(Self, UnboundedReceiver<Event>)> {
        let pty_options = Options {
            shell: options
                .program
                .map(|(program, args)| Shell::new(program, args)),
            drain_on_exit: true,
            ..Options::default()
        };
        let pty = tty::new(&pty_options, size.window_size(), 0)?;
        let master = pty.file().try_clone()?;

        let (sender, events) = mpsc::unbounded();
        let listener = Listener::new(sender);
        let term = Arc::new(FairMutex::new(Term::new(
            Config::default(),
            &size,
            listener.clone(),
        )));
        let event_loop = EventLoop::new(term.clone(), listener.clone(), pty, true, false)?;
        let notifier = Notifier(event_loop.channel());
        let io_thread = event_loop.spawn();

        // I/O スレッドは子プロセス終了時以外（I/O エラーや panic）では Exit を送らずに止まるため、
        // 終了を監視して必ず Exit を通知する。
        let watcher = thread::Builder::new()
            .name("PTY watcher".into())
            .spawn(move || {
                // join の戻り値（EventLoop と Pty）を破棄すると Pty の Drop が子プロセスの終了を待つため、
                // 先に Exit を通知してから破棄する
                let stopped = io_thread.join();
                listener.send_event(Event::Exit);
                drop(stopped);
            });
        if let Err(err) = watcher {
            let _ = notifier.0.send(Msg::Shutdown);
            return Err(err);
        }

        Ok((
            Self {
                term,
                notifier,
                size,
                master,
            },
            events,
        ))
    }

    /// alacritty からのイベントを処理する。PTY への応答が必要なもの（端末への問い合わせ）は
    /// ここで書き戻し、ビューが反応すべきものだけを返す。
    pub fn handle_event(&self, event: Event) -> Option<TerminalEvent> {
        match event {
            Event::Wakeup => Some(TerminalEvent::Wakeup),
            Event::Title(title) => Some(TerminalEvent::Title(title)),
            Event::ResetTitle => Some(TerminalEvent::ResetTitle),
            Event::Exit => Some(TerminalEvent::Exit),
            Event::PtyWrite(text) => {
                self.write(text.into_bytes());
                None
            }
            // fish などは起動時に色を問い合わせ、応答を待つので必ず返す
            Event::ColorRequest(index, format) => {
                let rgb = colors::index_to_rgb(index, self.term.lock().colors());
                if let Some(rgb) = rgb {
                    self.write(format(rgb).into_bytes());
                }
                None
            }
            Event::TextAreaSizeRequest(format) => {
                self.write(format(self.size.window_size()).into_bytes());
                None
            }
            // クリップボード連携・ベル・マウスカーソル形状は未対応
            Event::ClipboardStore(..)
            | Event::ClipboardLoad(..)
            | Event::Bell
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::ChildExit(_) => None,
        }
    }

    /// PTY に入力を書き込む。
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        self.notifier.notify(bytes);
    }

    /// グリッドと PTY のサイズを変更する。
    pub fn resize(&mut self, size: GridSize) {
        if size == self.size {
            return;
        }
        self.size = size;
        self.notifier.on_resize(size.window_size());
        self.term.lock().resize(size);
    }

    pub fn size(&self) -> GridSize {
        self.size
    }

    /// DECCKM（アプリケーションカーソルキーモード）が有効か。
    pub fn app_cursor(&self) -> bool {
        self.term.lock().mode().contains(TermMode::APP_CURSOR)
    }

    pub fn snapshot(&self) -> Snapshot {
        content::snapshot(&self.term.lock())
    }

    /// 画面の各行の文字列（行末の空白は除く）。
    pub fn screen_lines(&self) -> Vec<String> {
        self.snapshot()
            .rows
            .iter()
            .map(|row| row.text().trim_end().to_string())
            .collect()
    }

    /// PTY のフォアグラウンドで動いているプロセス。
    pub fn foreground_process(&self) -> Option<ProcessInfo> {
        process::foreground_process(&self.master)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc::TryRecvError;
    use std::time::{Duration, Instant};

    const SIZE: GridSize = GridSize {
        cols: 40,
        rows: 5,
        cell_width: 8.0,
        cell_height: 16.0,
    };

    fn sh(script: &str) -> SpawnOptions {
        SpawnOptions {
            program: Some(("sh".into(), vec!["-c".into(), script.into()])),
        }
    }

    /// 次のイベントを待つ。チャネルが閉じたか期限切れなら panic。
    fn next_event(events: &mut UnboundedReceiver<Event>, deadline: Instant) -> Event {
        loop {
            match events.try_recv() {
                Ok(event) => return event,
                Err(TryRecvError::Closed) => panic!("event channel closed"),
                Err(TryRecvError::Empty) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(TryRecvError::Empty) => panic!("timed out waiting for terminal event"),
            }
        }
    }

    /// ビューと同じように `handle_event` でイベントを処理し、`Exit` まで進める。
    fn run_until_exit(
        terminal: &Terminal,
        events: &mut UnboundedReceiver<Event>,
    ) -> Vec<TerminalEvent> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut handled = Vec::new();
        loop {
            if let Some(event) = terminal.handle_event(next_event(events, deadline)) {
                let exit = event == TerminalEvent::Exit;
                handled.push(event);
                if exit {
                    return handled;
                }
            }
        }
    }

    fn screen_text(terminal: &Terminal) -> String {
        terminal.snapshot().rows.iter().map(|r| r.text()).collect()
    }

    #[test]
    fn renders_program_output() {
        let (terminal, mut events) = Terminal::spawn(sh("printf hello"), SIZE).unwrap();
        run_until_exit(&terminal, &mut events);
        assert!(terminal.snapshot().rows[0].text().starts_with("hello"));
    }

    #[test]
    fn forwards_input_to_program() {
        let (terminal, mut events) =
            Terminal::spawn(sh("read line; printf \"got:$line\""), SIZE).unwrap();
        terminal.write(b"abc\r".as_slice());
        run_until_exit(&terminal, &mut events);
        let text = screen_text(&terminal);
        assert!(text.contains("got:abc"), "{text:?}");
    }

    #[test]
    fn resize_updates_grid_and_pty() {
        let (mut terminal, mut events) = Terminal::spawn(sh("read _; stty size"), SIZE).unwrap();
        let bigger = GridSize {
            cols: 50,
            rows: 7,
            ..SIZE
        };
        terminal.resize(bigger);
        assert_eq!(terminal.size(), bigger);
        terminal.write(b"\r".as_slice());
        run_until_exit(&terminal, &mut events);
        assert_eq!(terminal.snapshot().rows.len(), 7);
        let text = screen_text(&terminal);
        assert!(text.contains("7 50"), "{text:?}");
    }

    #[test]
    fn reports_title_changes() {
        let (terminal, mut events) =
            Terminal::spawn(sh("printf '\\033]2;hello\\007'"), SIZE).unwrap();
        let handled = run_until_exit(&terminal, &mut events);
        assert!(handled.contains(&TerminalEvent::Title("hello".into())));
    }

    #[test]
    fn answers_color_queries_with_current_palette() {
        // 背景色を OSC 11 で変更してから問い合わせ、返ってきた応答（24 バイト）を表示させる
        let script = "stty raw -echo; \
            printf '\\033]11;#ffffff\\007\\033]11;?\\007'; \
            reply=$(dd bs=1 count=24 2>/dev/null); \
            printf 'got:%s' \"$reply\" | tr -d '\\033\\007'";
        let (terminal, mut events) = Terminal::spawn(sh(script), SIZE).unwrap();
        run_until_exit(&terminal, &mut events);
        let text = screen_text(&terminal);
        assert!(text.contains("got:]11;rgb:ffff/ffff/ffff"), "{text:?}");
    }

    #[test]
    fn tracks_app_cursor_mode() {
        let (terminal, mut events) = Terminal::spawn(sh("printf '\\033[?1h'"), SIZE).unwrap();
        run_until_exit(&terminal, &mut events);
        assert!(terminal.app_cursor());
    }

    #[test]
    fn screen_lines_trim_trailing_spaces() {
        let (terminal, mut events) = Terminal::spawn(sh("printf 'ab\\ncd'"), SIZE).unwrap();
        run_until_exit(&terminal, &mut events);
        let lines = terminal.screen_lines();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "ab");
        assert_eq!(lines[1], "cd");
        assert_eq!(lines[2], "");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reports_foreground_process() {
        let (terminal, _events) = Terminal::spawn(sh("exec sleep 5"), SIZE).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let process = terminal.foreground_process();
            if let Some(process) = process.filter(|p| p.comm == "sleep") {
                assert_eq!(process.argv, ["sleep", "5"]);
                break;
            }
            assert!(Instant::now() < deadline, "sleep never became foreground");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn dropping_terminal_reports_exit() {
        let (terminal, mut events) = Terminal::spawn(sh("sleep 10"), SIZE).unwrap();
        drop(terminal);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !matches!(next_event(&mut events, deadline), Event::Exit) {}
    }

    #[test]
    fn reports_exit_without_waiting_for_child_that_ignores_sighup() {
        // 子が SIGHUP を無視すると Pty の Drop（SIGHUP → wait）がブロックするが、Exit はその前に届くべき
        let (terminal, mut events) =
            Terminal::spawn(sh("trap '' HUP; echo ready; sleep 8"), SIZE).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        // trap が設定されるまで待つ
        while !terminal.snapshot().rows[0].text().starts_with("ready") {
            terminal.handle_event(next_event(&mut events, deadline));
        }
        drop(terminal);
        while !matches!(next_event(&mut events, deadline), Event::Exit) {}
    }
}
