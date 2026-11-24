//! ウィンドウのルートビュー。複数のタブを持ち、各タブはターミナルを分割したペインを並べる。
//! タブバーとアクティブなタブのペインを描画する。

use alacritty_terminal::event::Event;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    AnyElement, Context, Entity, Focusable, KeyDownEvent, MouseButton, Render, Subscription, Task,
    Window, div, prelude::*, px, rgb,
};

use std::time::Duration;

use crate::agents::AgentState;
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
const SIDEBAR_WIDTH: f32 = 220.0;
/// Agent の状態を調べる間隔。
const AGENT_POLL_INTERVAL: Duration = Duration::from_millis(500);
const WORKING_COLOR: u32 = 0x569cd6;
const BLOCKED_COLOR: u32 = 0xe5c07b;
const DONE_COLOR: u32 = 0x89d185;

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
    /// Agent 一覧のサイドバーを表示するか。
    sidebar_visible: bool,
    _agent_poll: Task<()>,
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
            sidebar_visible: true,
            _agent_poll: Self::poll_agents(window, cx),
        };
        this.activate(window, cx);
        this
    }

    /// 一定間隔で各ペインの Agent の状態を更新する。
    fn poll_agents(window: &Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(AGENT_POLL_INTERVAL).await;
                if this.update(cx, Self::refresh_agents).is_err() {
                    break;
                }
            }
        })
    }

    fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        let active_tab = self.tabs.active_index();
        let mut changed = false;
        for (tab_index, tab) in self.tabs.iter().enumerate() {
            let active_pane = tab.panes.active_id();
            for (pane_id, pane) in tab.panes.iter() {
                let focused = tab_index == active_tab && pane_id == active_pane;
                changed |= pane.view.update(cx, |view, _| view.refresh_agent(focused));
            }
        }
        if changed {
            cx.notify();
        }
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
            Command::ToggleSidebar => self.sidebar_visible = !self.sidebar_visible,
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
    /// 指定したタブのペインへ移る（サイドバーの Agent をクリックしたとき）。
    fn focus_tab_pane(
        &mut self,
        tab_index: usize,
        pane_id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tabs.select(tab_index);
        self.focus_pane(pane_id, window, cx);
    }

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
    /// 全タブ・全ペインで動いている Agent の一覧。クリックでそのペインへ移る。
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active_tab = self.tabs.active_index();
        let mut rows = Vec::new();
        for (tab_index, tab) in self.tabs.iter().enumerate() {
            for (pane_id, pane) in tab.panes.iter() {
                let view = pane.view.read(cx);
                let Some(agent) = view.agent() else { continue };
                let location = tab_label(tab_index, view.title());
                let active = tab_index == active_tab && pane_id == tab.panes.active_id();
                let color = rgb(state_color(agent.state));
                let row = div()
                    .id(("agent", rows.len()))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .cursor_pointer()
                    .when(active, |row| row.bg(rgb(colors::DEFAULT_BACKGROUND)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(div().text_color(color).child("●"))
                            .child(
                                div()
                                    .flex_1()
                                    .text_color(rgb(colors::DEFAULT_FOREGROUND))
                                    .child(agent.kind.label()),
                            )
                            .child(div().text_color(color).child(agent.state.label())),
                    )
                    .child(
                        div()
                            .text_color(rgb(INACTIVE_TAB_FOREGROUND))
                            .truncate()
                            .child(location),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.focus_tab_pane(tab_index, pane_id, window, cx)
                    }));
                rows.push(row);
            }
        }

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .overflow_hidden()
            .text_size(px(12.0))
            .bg(rgb(BAR_BACKGROUND))
            .border_r_1()
            .border_color(rgb(BAR_BORDER))
            .child(
                div()
                    .px(px(10.0))
                    .py(px(6.0))
                    .text_color(rgb(INACTIVE_TAB_FOREGROUND))
                    .child("AGENTS"),
            )
            .when(rows.is_empty(), |sidebar| {
                sidebar.child(
                    div()
                        .px(px(10.0))
                        .text_color(rgb(INACTIVE_TAB_FOREGROUND))
                        .child("No agents"),
                )
            })
            .children(rows)
    }

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
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .children(self.sidebar_visible.then(|| self.render_sidebar(cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .h_full()
                            .overflow_hidden()
                            .children(self.render_panes(cx)),
                    ),
            )
            .child(self.render_status_line())
    }
}

/// サイドバーで Agent の状態を表す色。
fn state_color(state: AgentState) -> u32 {
    match state {
        AgentState::Working => WORKING_COLOR,
        AgentState::Blocked => BLOCKED_COLOR,
        AgentState::Done => DONE_COLOR,
        AgentState::Idle => INACTIVE_TAB_FOREGROUND,
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
    fn agent_states_have_distinct_colors() {
        let colors = [
            AgentState::Working,
            AgentState::Blocked,
            AgentState::Done,
            AgentState::Idle,
        ]
        .map(state_color);
        for (i, a) in colors.iter().enumerate() {
            for b in &colors[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn window_title_falls_back_to_app_name() {
        assert_eq!(window_title(Some("vim")), "vim");
        assert_eq!(window_title(None), "kugutsu");
    }
}
