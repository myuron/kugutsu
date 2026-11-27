mod agents;
mod cli;
mod config;
mod font;
mod icons;
mod panes;
mod plugins;
mod tabs;
mod terminal;
mod terminal_view;
mod theme;
mod workspace_view;

use std::rc::Rc;

use clap::Parser;
use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};

use terminal::{SpawnOptions, Terminal};
use workspace_view::{INITIAL_SIZE, WorkspaceView};

fn main() {
    let cli = cli::Cli::parse();
    let paths = plugins::paths::resolve(|name| std::env::var_os(name));
    if let Some(command) = cli.command {
        let Some(paths) = paths else {
            eprintln!("cannot find the config directory: set XDG_CONFIG_HOME or HOME");
            std::process::exit(1);
        };
        let ok = cli::run(
            &command,
            &paths,
            &plugins::git::GitCli,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
        std::process::exit(if ok { 0 } else { 1 });
    }

    // TERM / COLORTERM を設定する（内部で set_var を呼ぶため、スレッド生成前に一度だけ呼ぶ）
    alacritty_terminal::tty::setup_env();

    let load_config: workspace_view::ConfigLoader =
        Rc::new(move || paths.as_ref().map(config::load).unwrap_or_default());
    let config = load_config();

    let (terminal, events) = match Terminal::spawn(SpawnOptions::default(), INITIAL_SIZE) {
        Ok(spawned) => spawned,
        Err(err) => {
            eprintln!("failed to start terminal: {err}");
            std::process::exit(1);
        }
    };

    Application::new()
        .with_assets(icons::Assets)
        .run(move |cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(800.0), px(500.0)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_background: config.config.window_background(),
                ..Default::default()
            };
            let opened = cx.open_window(options, |window, cx| {
                cx.new(|cx| {
                    WorkspaceView::new(
                        terminal,
                        events,
                        SpawnOptions::default(),
                        config,
                        load_config,
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
