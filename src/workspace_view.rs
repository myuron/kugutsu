//! ウィンドウのルートビュー。複数のタブを持ち、各タブはターミナルを分割したペインを並べる。
//! タブバーとアクティブなタブのペインを描画する。

use alacritty_terminal::event::Event;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    AnyElement, Context, Entity, Focusable, KeyDownEvent, MouseButton, Render, Subscription,
    Window, div, prelude::*, px, rgb,
};

use crate::panes::{Axis, Node, PaneId, Panes};
use crate::tabs::Tabs;
use crate::tabs::prefix::{Command, KeyOutcome, PREFIX, PrefixKeymap};
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
/// 分割時にアクティブなペインを囲む枠の色。
const ACTIVE_PANE_BORDER: u32 = 0x569cd6;

struct Pane {
    view: Entity<TerminalView>,
    _subscription: Subscription,
}

struct Tab {
    panes: Panes<Pane>,
}

impl Tab {
    fn active_view(&self) -> Option<&Entity<TerminalView>> {
        self.panes.active().map(|pane| &pane.view)
    }
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
        let first = Self::create_pane(terminal, events, window, cx);
        let this = Self {
            tabs: Tabs::new(Tab {
                panes: Panes::new(first),
            }),
            keymap: PrefixKeymap::default(),
        };
        this.activate(window, cx);
        this
    }

    fn create_pane(
        terminal: Terminal,
        events: UnboundedReceiver<Event>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pane {
        let view = cx.new(|cx| TerminalView::new(terminal, events, window, cx));
        let subscription = cx.subscribe_in(&view, window, Self::on_terminal_event);
        Pane {
            view,
            _subscription: subscription,
        }
    }

    /// アクティブなペインと同じサイズでターミナルを起動してペインにする。新しいペインは
    /// 今のペインと同程度の領域に出るので、再描画時のリサイズを小さく抑えられる。
    fn spawn_pane(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<Pane> {
        let size = self
            .tabs
            .active()
            .and_then(Tab::active_view)
            .map_or(INITIAL_SIZE, |view| view.read(cx).grid_size());
        match Terminal::spawn(SpawnOptions::default(), size) {
            Ok((terminal, events)) => Some(Self::create_pane(terminal, events, window, cx)),
            Err(err) => {
                eprintln!("failed to start terminal: {err}");
                None
            }
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
                let found = self.tabs.iter().enumerate().find_map(|(index, tab)| {
                    let id = tab.panes.find(|pane| &pane.view == view)?;
                    Some((index, id))
                });
                if let Some((tab_index, pane_id)) = found {
                    self.close_pane(tab_index, pane_id, window, cx);
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

    fn run(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::NewTab => return self.new_tab(window, cx),
            Command::Split(axis) => return self.split(axis, window, cx),
            Command::ClosePane => {
                let tab_index = self.tabs.active_index();
                if let Some(pane_id) = self.tabs.active().map(|tab| tab.panes.active_id()) {
                    self.close_pane(tab_index, pane_id, window, cx);
                }
                return;
            }
            Command::NextTab => self.tabs.next(),
            Command::PrevTab => self.tabs.prev(),
            Command::SelectTab(index) => self.tabs.select(index),
            Command::Focus(direction) => {
                if let Some(tab) = self.tabs.active_mut() {
                    tab.panes.focus(direction);
                }
            }
        }
        self.activate(window, cx);
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.spawn_pane(window, cx) else {
            return;
        };
        self.tabs.push(Tab {
            panes: Panes::new(pane),
        });
        self.activate(window, cx);
        cx.notify();
    }

    fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.spawn_pane(window, cx) else {
            return;
        };
        let Some(tab) = self.tabs.active_mut() else {
            return;
        };
        tab.panes.split(axis, pane);
        self.activate(window, cx);
        cx.notify();
    }

    /// アクティブなタブの中で指定したペインをアクティブにする（クリック時）。
    fn focus_pane(&mut self, pane_id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.active_mut() {
            tab.panes.set_active(pane_id);
        }
        self.activate(window, cx);
        cx.notify();
    }

    /// ペインを閉じる（ターミナルは破棄時にシェルを終了させる）。最後のペインならタブを閉じる。
    fn close_pane(
        &mut self,
        tab_index: usize,
        pane_id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get_mut(tab_index) else {
            return;
        };
        if tab.panes.remove(pane_id).is_none() {
            return;
        }
        if tab.panes.is_empty() {
            return self.close_tab(tab_index, window, cx);
        }
        self.activate(window, cx);
        cx.notify();
    }

    /// タブを閉じる。最後のタブならウィンドウを閉じる。
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

    /// アクティブなタブのアクティブなペインにフォーカスを移し、ウィンドウタイトルを合わせる。
    fn activate(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.tabs.active().and_then(Tab::active_view) {
            window.focus(&view.focus_handle(cx));
        }
        self.sync_window_title(window, cx);
    }

    fn sync_window_title(&self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self
            .tabs
            .active()
            .and_then(Tab::active_view)
            .and_then(|view| view.read(cx).title());
        window.set_window_title(window_title(title));
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active_index = self.tabs.active_index();
        let tabs = self.tabs.iter().enumerate().map(|(index, tab)| {
            let title = tab.active_view().and_then(|view| view.read(cx).title());
            let label = tab_label(index, title);
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
                    this.run(Command::SelectTab(index), window, cx)
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

    /// アクティブなタブのペインを分割どおりに並べる。
    fn render_panes(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panes = &self.tabs.active()?.panes;
        let root = panes.root()?;
        // ペインが 1 つだけなら枠で強調する必要は無い
        let highlight = matches!(root, Node::Split { .. }).then_some(panes.active_id());
        Some(render_node(root, highlight, cx))
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

/// ペイン木の節を描画する。分割は子を均等な大きさで並べ、間に境界線を引く。
/// `highlight` が指すペインは枠で強調する。
fn render_node(
    node: &Node<Pane>,
    highlight: Option<PaneId>,
    cx: &mut Context<WorkspaceView>,
) -> AnyElement {
    match node {
        Node::Leaf { id, item } => {
            let id = *id;
            div()
                .size_full()
                .when_some(highlight, |pane, active| {
                    pane.border_1().border_color(rgb(if id == active {
                        ACTIVE_PANE_BORDER
                    } else {
                        colors::DEFAULT_BACKGROUND
                    }))
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| this.focus_pane(id, window, cx)),
                )
                .child(item.view.clone())
                .into_any_element()
        }
        Node::Split { axis, children } => {
            let mut container = div().size_full().flex();
            container = match axis {
                Axis::Horizontal => container.flex_row(),
                Axis::Vertical => container.flex_col(),
            };
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    let separator = div().flex_none().bg(rgb(BAR_BORDER));
                    container = container.child(match axis {
                        Axis::Horizontal => separator.w(px(1.0)),
                        Axis::Vertical => separator.h(px(1.0)),
                    });
                }
                container = container.child(
                    // 中身の大きさに引きずられず均等に分けるため、基準サイズ 0 から伸ばす
                    div()
                        .flex_1()
                        .flex_basis(px(0.0))
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .overflow_hidden()
                        .child(render_node(child, highlight, cx)),
                );
            }
            container.into_any_element()
        }
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
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .children(self.render_panes(cx)),
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
