mod agents;
mod config;
mod font;
mod panes;
mod tabs;
mod terminal;
mod terminal_view;
mod workspace_view;

use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};

use terminal::{SpawnOptions, Terminal};
use workspace_view::{INITIAL_SIZE, WorkspaceView};

fn main() {
    // TERM / COLORTERM を設定する（内部で set_var を呼ぶため、スレッド生成前に一度だけ呼ぶ）
    alacritty_terminal::tty::setup_env();

    let config = config::config_path(|name| std::env::var_os(name))
        .map(|path| config::load(&path))
        .unwrap_or_default();

    let (terminal, events) = match Terminal::spawn(SpawnOptions::default(), INITIAL_SIZE) {
        Ok(spawned) => spawned,
        Err(err) => {
            eprintln!("failed to start terminal: {err}");
            std::process::exit(1);
        }
    };

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.0), px(500.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| {
                WorkspaceView::new(
                    terminal,
                    events,
                    SpawnOptions::default(),
                    config,
                    window,
                    cx,
                )
            })
        });
        if let Err(err) = opened {
            eprintln!("failed to open window: {err:#}");
            cx.quit();
        }
        // macOS では最後のウィンドウを閉じてもアプリが終了しないため明示的に終了する
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
