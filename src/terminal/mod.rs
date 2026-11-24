//! PTY 上でシェルを動かすターミナルのモデル。

pub mod colors;
pub mod content;
pub mod keys;
mod listener;
pub mod size;

use std::borrow::Cow;
use std::io;
use std::sync::Arc;

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, Notify, OnResize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Config;
use alacritty_terminal::tty::{self, Options, Shell};
use futures::channel::mpsc::{self, UnboundedReceiver};

use content::Snapshot;
use listener::Listener;
use size::GridSize;

/// 起動するプログラム。`None` ならユーザーのログインシェル（`$SHELL`）。
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    pub program: Option<(String, Vec<String>)>,
}

/// PTY と接続されたターミナル。
pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    notifier: Notifier,
    size: GridSize,
}

impl Terminal {
    /// PTY を作成してプログラムを起動する。返り値の受信側にはターミナルからのイベントが届く。
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

        let (sender, events) = mpsc::unbounded();
        let listener = Listener(sender);
        let term = Arc::new(FairMutex::new(Term::new(
            Config::default(),
            &size,
            listener.clone(),
        )));
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let notifier = Notifier(event_loop.channel());
        event_loop.spawn();

        Ok((
            Self {
                term,
                notifier,
                size,
            },
            events,
        ))
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

    pub fn snapshot(&self) -> Snapshot {
        content::snapshot(&self.term.lock())
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
    use std::time::{Duration, Instant};

    const SIZE: GridSize = GridSize {
        cols: 20,
        rows: 5,
        cell_width: 8.0,
        cell_height: 16.0,
    };

    fn sh(script: &str) -> SpawnOptions {
        SpawnOptions {
            program: Some(("sh".into(), vec!["-c".into(), script.into()])),
        }
    }

    /// `Exit` を受け取るまでイベントを集める。
    fn wait_for_exit(events: &mut UnboundedReceiver<Event>) -> Vec<Event> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut received = Vec::new();
        while Instant::now() < deadline {
            match events.try_next() {
                Ok(Some(event)) => {
                    let exit = matches!(event, Event::Exit);
                    received.push(event);
                    if exit {
                        return received;
                    }
                }
                Ok(None) => break,
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        panic!("terminal did not exit in time: {received:?}");
    }

    #[test]
    fn renders_program_output() {
        let (terminal, mut events) = Terminal::spawn(sh("printf hello"), SIZE).unwrap();
        wait_for_exit(&mut events);
        assert!(terminal.snapshot().rows[0].text.starts_with("hello"));
    }

    #[test]
    fn forwards_input_to_program() {
        let (terminal, mut events) =
            Terminal::spawn(sh("read line; printf \"got:$line\""), SIZE).unwrap();
        terminal.write(b"abc\r".as_slice());
        wait_for_exit(&mut events);
        let text: String = terminal
            .snapshot()
            .rows
            .iter()
            .map(|r| r.text.as_str())
            .collect();
        assert!(text.contains("got:abc"), "{text:?}");
    }

    #[test]
    fn resize_updates_grid_and_pty() {
        let (mut terminal, mut events) = Terminal::spawn(sh("read _; stty size"), SIZE).unwrap();
        let bigger = GridSize {
            cols: 30,
            rows: 7,
            ..SIZE
        };
        terminal.resize(bigger);
        assert_eq!(terminal.size(), bigger);
        terminal.write(b"\r".as_slice());
        wait_for_exit(&mut events);
        let snap = terminal.snapshot();
        assert_eq!(snap.rows.len(), 7);
        let text: String = snap.rows.iter().map(|r| r.text.as_str()).collect();
        assert!(text.contains("7 30"), "{text:?}");
    }
}
