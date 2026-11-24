mod font;
mod terminal;
mod terminal_view;

use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};

use terminal_view::TerminalView;

fn main() {
    // TERM / COLORTERM を設定する（スレッド生成前に一度だけ呼ぶ必要がある）
    alacritty_terminal::tty::setup_env();

    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.0), px(500.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| TerminalView::new(window, cx).expect("failed to start terminal"))
        });
        if let Err(err) = opened {
            eprintln!("failed to open window: {err:#}");
            cx.quit();
        }
        cx.activate(true);
    });
}
