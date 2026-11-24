//! ウィンドウのルートビュー。複数のターミナルをタブとして持ち、タブバーと
//! アクティブなターミナルを描画する。

use alacritty_terminal::event::Event;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    Context, Entity, Focusable, KeyDownEvent, Render, Subscription, Window, div, prelude::*, px,
    rgb,
};

use crate::tabs::Tabs;
use crate::tabs::prefix::{KeyOutcome, PREFIX, PrefixKeymap, TabCommand};
use crate::terminal::size::GridSize;
use crate::terminal::{SpawnOptions, Terminal, colors};
use crate::terminal_view::{TerminalView, TerminalViewEvent};

/// 起動直後の仮サイズ。最初の描画で表示領域に合わせてリサイズされる。
pub const INITIAL_SIZE: GridSize = GridSize {
    cols: 80,
    rows: 24,
    cell_width: 8.0,
    cell_height: 18.0,
};

const DEFAULT_TITLE: &str = "kugutsu";
/// タイトル未設定のタブに表示する名前。
const UNTITLED_TAB: &str = "shell";
const TAB_BAR_HEIGHT: f32 = 28.0;
const STATUS_LINE_HEIGHT: f32 = 22.0;
const BAR_BACKGROUND: u32 = 0x181818;
const BAR_BORDER: u32 = 0x2b2b2b;
const INACTIVE_TAB_FOREGROUND: u32 = 0x8a8a8a;
const PREFIX_INDICATOR: u32 = 0xdcdcaa;

struct Tab {
    view: Entity<TerminalView>,
    _subscription: Subscription,
}

pub struct WorkspaceView {
    tabs: Tabs<Tab>,
    keymap: PrefixKeymap,
}

impl WorkspaceView {
    /// 起動済みのターミナルを最初のタブとして持つワークスペースを作る。
    pub fn new(
        terminal: Terminal,
        events: UnboundedReceiver<Event>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let first = Self::create_tab(terminal, events, window, cx);
        let this = Self {
            tabs: Tabs::new(first),
            keymap: PrefixKeymap::default(),
        };
        this.activate(window, cx);
        this
    }

    fn create_tab(
        terminal: Terminal,
        events: UnboundedReceiver<Event>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Tab {
        let view = cx.new(|cx| TerminalView::new(terminal, events, window, cx));
        let subscription = cx.subscribe_in(&view, window, Self::on_terminal_event);
        Tab {
            view,
            _subscription: subscription,
        }
    }

    fn on_terminal_event(
        &mut self,
        view: &Entity<TerminalView>,
        event: &TerminalViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalViewEvent::TitleChanged => {
                self.sync_window_title(window, cx);
                cx.notify();
            }
            TerminalViewEvent::Exited => {
                if let Some(index) = self.tabs.position(|tab| &tab.view == view) {
                    self.close_tab(index, window, cx);
                }
            }
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let was_armed = self.keymap.is_armed();
        match self.keymap.handle(&event.keystroke) {
            KeyOutcome::Forward => {}
            KeyOutcome::Consumed => cx.stop_propagation(),
            KeyOutcome::Command(command) => {
                cx.stop_propagation();
                self.run(command, window, cx);
            }
        }
        if was_armed != self.keymap.is_armed() {
            cx.notify();
        }
    }

    fn run(&mut self, command: TabCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            TabCommand::New => return self.new_tab(window, cx),
            TabCommand::Close => return self.close_tab(self.tabs.active_index(), window, cx),
            TabCommand::Next => self.tabs.next(),
            TabCommand::Prev => self.tabs.prev(),
            TabCommand::Select(index) => self.tabs.select(index),
        }
        self.activate(window, cx);
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 新しいタブは今のタブと同じ表示領域に出るので、そのサイズで起動して再描画時のリサイズを避ける
        let size = self
            .tabs
            .active()
            .map_or(INITIAL_SIZE, |tab| tab.view.read(cx).grid_size());
        let (terminal, events) = match Terminal::spawn(SpawnOptions::default(), size) {
            Ok(spawned) => spawned,
            Err(err) => {
                eprintln!("failed to start terminal: {err}");
                return;
            }
        };
        let tab = Self::create_tab(terminal, events, window, cx);
        self.tabs.push(tab);
        self.activate(window, cx);
        cx.notify();
    }

    /// タブを閉じる（ターミナルは破棄時にシェルを終了させる）。最後のタブならウィンドウを閉じる。
    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.remove(index).is_none() {
            return;
        }
        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }
        self.activate(window, cx);
        cx.notify();
    }

    /// アクティブなタブにフォーカスを移し、ウィンドウタイトルを合わせる。
    fn activate(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.active() {
            window.focus(&tab.view.focus_handle(cx));
        }
        self.sync_window_title(window, cx);
    }

    fn sync_window_title(&self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.tabs.active().and_then(|tab| tab.view.read(cx).title());
        window.set_window_title(window_title(title));
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active_index = self.tabs.active_index();
        let tabs = self.tabs.iter().enumerate().map(|(index, tab)| {
            let label = tab_label(index, tab.view.read(cx).title());
            let active = index == active_index;
            div()
                .id(("tab", index))
                .flex()
                .items_center()
                .px(px(12.0))
                .border_r_1()
                .border_color(rgb(BAR_BORDER))
                .text_color(rgb(if active {
                    colors::DEFAULT_FOREGROUND
                } else {
                    INACTIVE_TAB_FOREGROUND
                }))
                .when(active, |tab| tab.bg(rgb(colors::DEFAULT_BACKGROUND)))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run(TabCommand::Select(index), window, cx)
                }))
        });

        div()
            .flex()
            .flex_none()
            .h(px(TAB_BAR_HEIGHT))
            .text_size(px(12.0))
            .bg(rgb(BAR_BACKGROUND))
            .border_b_1()
            .border_color(rgb(BAR_BORDER))
            .children(tabs)
    }

    /// 画面下部のステータスライン。prefix 入力待ちのときはその旨を表示する。
    fn render_status_line(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(STATUS_LINE_HEIGHT))
            .px(px(12.0))
            .text_size(px(12.0))
            .bg(rgb(BAR_BACKGROUND))
            .border_t_1()
            .border_color(rgb(BAR_BORDER))
            .when(self.keymap.is_armed(), |line| {
                line.child(div().text_color(rgb(PREFIX_INDICATOR)).child(PREFIX))
            })
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors::DEFAULT_BACKGROUND))
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(self.render_tab_bar(cx))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .children(self.tabs.active().map(|tab| tab.view.clone())),
            )
            .child(self.render_status_line())
    }
}

/// タブバーに表示する名前（番号は prefix + 数字キーで選ぶときの番号）。
fn tab_label(index: usize, title: Option<&str>) -> String {
    format!("{}: {}", index + 1, title.unwrap_or(UNTITLED_TAB))
}

fn window_title(title: Option<&str>) -> &str {
    title.unwrap_or(DEFAULT_TITLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_label_numbers_from_one() {
        assert_eq!(tab_label(0, Some("vim")), "1: vim");
        assert_eq!(tab_label(2, None), "3: shell");
    }

    #[test]
    fn window_title_falls_back_to_app_name() {
        assert_eq!(window_title(Some("vim")), "vim");
        assert_eq!(window_title(None), "kugutsu");
    }
}
