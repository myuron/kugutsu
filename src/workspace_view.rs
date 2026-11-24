//! ウィンドウのルートビュー。複数のタブを持ち、各タブはターミナルを分割したペインを並べる。
//! タブバーとアクティブなタブのペインを描画する。

use alacritty_terminal::event::Event;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    AnyElement, Context, Entity, Focusable, KeyDownEvent, MouseButton, Render, ScrollHandle,
    SharedString, Stateful, Subscription, Task, Window, div, point, prelude::*, px, rgb, rgba,
};

use std::time::Duration;

use crate::agents::AgentState;
use crate::panes::{Axis, Node, PaneId, Panes};
use crate::tabs::Tabs;
use crate::tabs::prefix::{Command, KeyOutcome, PREFIX, PrefixKeymap, bindings};
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
const WORKSPACE_BAR_HEIGHT: f32 = 28.0;
const TAB_BAR_HEIGHT: f32 = 28.0;
/// workspace ライン・タブバーの各項目の幅（両者で揃える）。
const BAR_ITEM_WIDTH: f32 = 140.0;
/// バー右端の `<` `>` ボタンの幅。
const SCROLL_BUTTON_WIDTH: f32 = 24.0;
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
/// キーマップ一覧の後ろに敷く半透明の背景（RGBA）。
const KEYMAP_BACKDROP: u32 = 0x000000aa;
/// キーマップ一覧のキー列の幅。
const KEYMAP_KEY_COLUMN_WIDTH: f32 = 110.0;
/// which-key の 1 項目（キー＋説明）の幅と、そのうちキー部分の幅。
const WHICH_KEY_ITEM_WIDTH: f32 = 220.0;
const WHICH_KEY_KEY_WIDTH: f32 = 56.0;

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

/// タブのグループ。workspace ラインで切り替える。
struct Workspace {
    tabs: Tabs<Tab>,
}

impl Workspace {
    fn new(first: Pane) -> Self {
        Self {
            tabs: Tabs::new(Tab {
                panes: Panes::new(first),
            }),
        }
    }
}

pub struct WorkspaceView {
    workspaces: Tabs<Workspace>,
    keymap: PrefixKeymap,
    /// 新しいタブ・ペインで起動するプログラム。
    spawn_options: SpawnOptions,
    /// Agent 一覧のサイドバーを表示するか。
    sidebar_visible: bool,
    /// キーマップ一覧のオーバーレイを表示しているか。
    keymap_help_visible: bool,
    /// workspace ライン・タブバーの横スクロール位置。
    workspace_scroll: ScrollHandle,
    tab_scroll: ScrollHandle,
    _agent_poll: Task<()>,
}

impl WorkspaceView {
    /// 起動済みのターミナルを最初のタブとして持つワークスペースを作る。
    /// 以降に開くタブ・ペインでは `spawn_options` のプログラムを起動する。
    pub fn new(
        terminal: Terminal,
        events: UnboundedReceiver<Event>,
        spawn_options: SpawnOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let first = Self::create_pane(terminal, events, window, cx);
        let this = Self {
            workspaces: Tabs::new(Workspace::new(first)),
            keymap: PrefixKeymap::default(),
            spawn_options,
            sidebar_visible: true,
            keymap_help_visible: false,
            workspace_scroll: ScrollHandle::new(),
            tab_scroll: ScrollHandle::new(),
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

    /// アクティブな workspace のタブ集合。
    fn active_tabs(&self) -> Option<&Tabs<Tab>> {
        self.workspaces.active().map(|workspace| &workspace.tabs)
    }

    fn active_tabs_mut(&mut self) -> Option<&mut Tabs<Tab>> {
        self.workspaces
            .active_mut()
            .map(|workspace| &mut workspace.tabs)
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.active_tabs().and_then(Tabs::active)
    }

    fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.active_tabs_mut().and_then(Tabs::active_mut)
    }

    fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        let active_workspace = self.workspaces.active_index();
        let mut changed = false;
        for (workspace_index, workspace) in self.workspaces.iter().enumerate() {
            let active_tab = workspace.tabs.active_index();
            for (tab_index, tab) in workspace.tabs.iter().enumerate() {
                let active_pane = tab.panes.active_id();
                for (pane_id, pane) in tab.panes.iter() {
                    let focused = workspace_index == active_workspace
                        && tab_index == active_tab
                        && pane_id == active_pane;
                    changed |= pane.view.update(cx, |view, _| view.refresh_agent(focused));
                }
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
            .active_tab()
            .and_then(Tab::active_view)
            .map_or(INITIAL_SIZE, |view| view.read(cx).grid_size());
        match Terminal::spawn(self.spawn_options.clone(), size) {
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
                let found =
                    self.workspaces
                        .iter()
                        .enumerate()
                        .find_map(|(workspace_index, workspace)| {
                            workspace
                                .tabs
                                .iter()
                                .enumerate()
                                .find_map(|(tab_index, tab)| {
                                    let id = tab.panes.find(|pane| &pane.view == view)?;
                                    Some((workspace_index, tab_index, id))
                                })
                        });
                if let Some((workspace_index, tab_index, pane_id)) = found {
                    self.close_pane(workspace_index, tab_index, pane_id, window, cx);
                }
            }
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.keymap_help_visible {
            // 一覧の表示中はキーをターミナルにも prefix にも渡さない
            cx.stop_propagation();
            let keystroke = &event.keystroke;
            if !keystroke.modifiers.modified()
                && matches!(keystroke.key.as_str(), "escape" | "q" | "?")
            {
                self.close_keymap_help(cx);
            }
            return;
        }
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
            Command::NewWorkspace => return self.new_workspace(window, cx),
            Command::Split(axis) => return self.split(axis, window, cx),
            Command::ClosePane => {
                let workspace_index = self.workspaces.active_index();
                if let Some(tabs) = self.active_tabs() {
                    let tab_index = tabs.active_index();
                    if let Some(pane_id) = tabs.active().map(|tab| tab.panes.active_id()) {
                        self.close_pane(workspace_index, tab_index, pane_id, window, cx);
                    }
                }
                return;
            }
            Command::NextTab => self.active_tabs_mut().into_iter().for_each(Tabs::next),
            Command::PrevTab => self.active_tabs_mut().into_iter().for_each(Tabs::prev),
            Command::SelectTab(index) => {
                if let Some(tabs) = self.active_tabs_mut() {
                    tabs.select(index);
                }
            }
            Command::NextWorkspace => self.workspaces.next(),
            Command::PrevWorkspace => self.workspaces.prev(),
            Command::ToggleSidebar => self.sidebar_visible = !self.sidebar_visible,
            Command::ShowKeymap => self.keymap_help_visible = true,
            Command::Focus(direction) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.panes.focus(direction);
                }
            }
        }
        self.activate(window, cx);
        cx.notify();
    }

    fn close_keymap_help(&mut self, cx: &mut Context<Self>) {
        self.keymap_help_visible = false;
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.spawn_pane(window, cx) else {
            return;
        };
        let Some(tabs) = self.active_tabs_mut() else {
            return;
        };
        tabs.push(Tab {
            panes: Panes::new(pane),
        });
        self.activate(window, cx);
        cx.notify();
    }

    fn new_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.spawn_pane(window, cx) else {
            return;
        };
        self.workspaces.push(Workspace::new(pane));
        self.activate(window, cx);
        cx.notify();
    }

    fn select_workspace(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.workspaces.select(index);
        self.activate(window, cx);
        cx.notify();
    }

    fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.spawn_pane(window, cx) else {
            return;
        };
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        tab.panes.split(axis, pane);
        self.activate(window, cx);
        cx.notify();
    }

    /// 指定した workspace・タブのペインへ移る（サイドバーの Agent をクリックしたとき）。
    fn focus_agent_pane(
        &mut self,
        workspace_index: usize,
        tab_index: usize,
        pane_id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspaces.select(workspace_index);
        if let Some(tabs) = self.active_tabs_mut() {
            tabs.select(tab_index);
        }
        self.focus_pane(pane_id, window, cx);
    }

    /// アクティブなタブの中で指定したペインをアクティブにする（クリック時）。
    fn focus_pane(&mut self, pane_id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.panes.set_active(pane_id);
        }
        self.activate(window, cx);
        cx.notify();
    }

    /// ペインを閉じる（ターミナルは破棄時にシェルを終了させる）。最後のペインならタブを閉じる。
    fn close_pane(
        &mut self,
        workspace_index: usize,
        tab_index: usize,
        pane_id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .workspaces
            .get_mut(workspace_index)
            .and_then(|workspace| workspace.tabs.get_mut(tab_index))
        else {
            return;
        };
        if tab.panes.remove(pane_id).is_none() {
            return;
        }
        if tab.panes.is_empty() {
            return self.close_tab(workspace_index, tab_index, window, cx);
        }
        self.activate(window, cx);
        cx.notify();
    }

    /// タブを閉じる。最後のタブなら workspace を閉じる。
    fn close_tab(
        &mut self,
        workspace_index: usize,
        tab_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspaces.get_mut(workspace_index) else {
            return;
        };
        if workspace.tabs.remove(tab_index).is_none() {
            return;
        }
        if workspace.tabs.is_empty() {
            return self.close_workspace(workspace_index, window, cx);
        }
        self.activate(window, cx);
        cx.notify();
    }

    /// workspace を閉じる。最後の workspace ならウィンドウを閉じる。
    fn close_workspace(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspaces.remove(index).is_none() {
            return;
        }
        if self.workspaces.is_empty() {
            window.remove_window();
            return;
        }
        self.activate(window, cx);
        cx.notify();
    }

    /// アクティブなタブのアクティブなペインにフォーカスを移し、ウィンドウタイトルを合わせる。
    /// アクティブな workspace・タブがバーからはみ出していれば、見える位置までスクロールする。
    fn activate(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace_scroll
            .scroll_to_item(self.workspaces.active_index());
        if let Some(tabs) = self.active_tabs() {
            self.tab_scroll.scroll_to_item(tabs.active_index());
        }
        if let Some(view) = self.active_tab().and_then(Tab::active_view) {
            window.focus(&view.focus_handle(cx));
        }
        self.sync_window_title(window, cx);
    }

    fn sync_window_title(&self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self
            .active_tab()
            .and_then(Tab::active_view)
            .and_then(|view| view.read(cx).title());
        window.set_window_title(window_title(title));
    }

    /// タブバーの上に置く workspace の一覧。クリックでその workspace へ切り替える。
    fn render_workspace_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active_index = self.workspaces.active_index();
        let items = (0..self.workspaces.iter().count())
            .map(|index| {
                bar_item(index == active_index, workspace_label(index))
                    .id(("workspace", index))
                    .debug_selector(|| format!("workspace-{index}"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_workspace(index, window, cx)
                    }))
            })
            .collect();
        scrollable_bar(
            "workspace",
            WORKSPACE_BAR_HEIGHT,
            &self.workspace_scroll,
            items,
            cx,
        )
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut items = Vec::new();
        if let Some(tabs) = self.active_tabs() {
            let active_index = tabs.active_index();
            for (index, tab) in tabs.iter().enumerate() {
                let title = tab.active_view().and_then(|view| view.read(cx).title());
                items.push(
                    bar_item(index == active_index, tab_label(index, title))
                        .id(("tab", index))
                        .debug_selector(|| format!("tab-{index}"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run(Command::SelectTab(index), window, cx)
                        })),
                );
            }
        }
        scrollable_bar("tab", TAB_BAR_HEIGHT, &self.tab_scroll, items, cx)
    }

    /// アクティブなタブのペインを分割どおりに並べる。
    fn render_panes(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panes = &self.active_tab()?.panes;
        let root = panes.root()?;
        // ペインが 1 つだけなら枠で強調する必要は無い
        let highlight = matches!(root, Node::Split { .. }).then_some(panes.active_id());
        Some(render_node(root, highlight, cx))
    }

    /// 全 workspace・全タブ・全ペインで動いている Agent の一覧。クリックでそのペインへ移る。
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active_workspace = self.workspaces.active_index();
        let mut rows = Vec::new();
        let tabs = self
            .workspaces
            .iter()
            .enumerate()
            .flat_map(|(workspace_index, workspace)| {
                let active_tab = workspace.tabs.active_index();
                workspace
                    .tabs
                    .iter()
                    .enumerate()
                    .map(move |(tab_index, tab)| {
                        let active = workspace_index == active_workspace && tab_index == active_tab;
                        (workspace_index, tab_index, tab, active)
                    })
            });
        for (workspace_index, tab_index, tab, active_tab) in tabs {
            for (pane_id, pane) in tab.panes.iter() {
                let view = pane.view.read(cx);
                let Some(agent) = view.agent() else { continue };
                let location = agent_location(workspace_index, tab_index, view.title());
                let active = active_tab && pane_id == tab.panes.active_id();
                let color = rgb(state_color(agent.state));
                let index = rows.len();
                let row = div()
                    .id(("agent", index))
                    .debug_selector(|| format!("agent-{index}"))
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
                        this.focus_agent_pane(workspace_index, tab_index, pane_id, window, cx)
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
                // workspace ライン＋タブバーと高さ・下線を揃える
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(WORKSPACE_BAR_HEIGHT + TAB_BAR_HEIGHT))
                    .px(px(10.0))
                    .border_b_1()
                    .border_color(rgb(BAR_BORDER))
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

    /// 画面下部のステータスライン。prefix 入力待ちのときはその旨を表示する。
    fn render_status_line(&self) -> impl IntoElement {
        div()
            .debug_selector(|| "status-line".into())
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

    /// prefix 入力待ちのとき、ステータスラインのすぐ上に重ねて出す次のキーの候補
    /// （vim の which-key 風）。ターミナルを縮めてリサイズさせないよう、レイアウトには
    /// 参加させずに重ねる。
    fn render_which_key(&self) -> impl IntoElement {
        let items = bindings().into_iter().map(|(key, description)| {
            div()
                .flex()
                .flex_none()
                .gap(px(8.0))
                .w(px(WHICH_KEY_ITEM_WIDTH))
                .overflow_hidden()
                .child(
                    div()
                        .flex_none()
                        .w(px(WHICH_KEY_KEY_WIDTH))
                        .text_color(rgb(PREFIX_INDICATOR))
                        .child(key),
                )
                .child(
                    div()
                        .min_w(px(0.0))
                        .truncate()
                        .text_color(rgb(colors::DEFAULT_FOREGROUND))
                        .child(description),
                )
        });
        div()
            .debug_selector(|| "which-key".into())
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(STATUS_LINE_HEIGHT))
            .flex()
            .flex_wrap()
            .gap_y(px(4.0))
            .px(px(12.0))
            .py(px(8.0))
            .text_size(px(12.0))
            .bg(rgb(BAR_BACKGROUND))
            .border_t_1()
            .border_color(rgb(BAR_BORDER))
            .children(items)
    }

    /// ウィンドウ全体に重ねるキーマップ一覧。背景をクリックすると閉じる。
    fn render_keymap_help(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = bindings().into_iter().map(|(key, description)| {
            div()
                .flex()
                .gap(px(16.0))
                .child(
                    div()
                        .w(px(KEYMAP_KEY_COLUMN_WIDTH))
                        .flex_none()
                        .text_color(rgb(PREFIX_INDICATOR))
                        .child(format!("{PREFIX} {key}")),
                )
                .child(
                    div()
                        .text_color(rgb(colors::DEFAULT_FOREGROUND))
                        .child(description),
                )
        });
        let panel = div()
            .id("keymap-help")
            .debug_selector(|| "keymap-help".into())
            .occlude()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .p(px(16.0))
            .text_size(px(12.0))
            .bg(rgb(BAR_BACKGROUND))
            .border_1()
            .border_color(rgb(BAR_BORDER))
            .child(
                div()
                    .pb(px(8.0))
                    .text_color(rgb(INACTIVE_TAB_FOREGROUND))
                    .child("KEY BINDINGS"),
            )
            .children(rows)
            .child(
                div()
                    .pt(px(8.0))
                    .text_color(rgb(INACTIVE_TAB_FOREGROUND))
                    .child("esc / q to close"),
            );
        div()
            .id("keymap-help-backdrop")
            .debug_selector(|| "keymap-help-backdrop".into())
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(KEYMAP_BACKDROP))
            .on_click(cx.listener(|this, _, _, cx| this.close_keymap_help(cx)))
            .child(panel)
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
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors::DEFAULT_BACKGROUND))
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .children(self.sidebar_visible.then(|| self.render_sidebar(cx)))
                    // タブバーはターミナル領域の上にだけ置く
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(self.render_workspace_bar(cx))
                            .child(self.render_tab_bar(cx))
                            .child(
                                div()
                                    .debug_selector(|| "panes".into())
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .overflow_hidden()
                                    .children(self.render_panes(cx)),
                            ),
                    ),
            )
            .child(self.render_status_line())
            .when(self.keymap.is_armed(), |root| {
                root.child(self.render_which_key())
            })
            .when(self.keymap_help_visible, |root| {
                root.child(self.render_keymap_help(cx))
            })
    }
}

/// workspace ライン・タブバー。項目は横スクロールし、右端の `<` `>` で 1 項目分ずつ動かす。
/// `name` はデバッグ用セレクタ（`{name}-bar`, `{name}-scroll-left` など）の接頭辞。
fn scrollable_bar(
    name: &'static str,
    height: f32,
    scroll: &ScrollHandle,
    items: Vec<Stateful<gpui::Div>>,
    cx: &mut Context<WorkspaceView>,
) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .h(px(height))
        .text_size(px(12.0))
        .bg(rgb(BAR_BACKGROUND))
        .border_b_1()
        .border_color(rgb(BAR_BORDER))
        .child(
            div()
                .id(SharedString::from(format!("{name}-bar")))
                .debug_selector(move || format!("{name}-bar"))
                .flex()
                .flex_1()
                .min_w(px(0.0))
                .overflow_x_scroll()
                .track_scroll(scroll)
                .children(items),
        )
        .child(scroll_button(name, "left", "<", BAR_ITEM_WIDTH, scroll, cx))
        .child(scroll_button(
            name,
            "right",
            ">",
            -BAR_ITEM_WIDTH,
            scroll,
            cx,
        ))
}

/// バーを `delta` だけ横にスクロールするボタン（正の値で左端側へ戻る）。
fn scroll_button(
    name: &'static str,
    side: &'static str,
    label: &'static str,
    delta: f32,
    scroll: &ScrollHandle,
    cx: &mut Context<WorkspaceView>,
) -> impl IntoElement {
    let scroll = scroll.clone();
    div()
        .id(SharedString::from(format!("{name}-scroll-{side}")))
        .debug_selector(move || format!("{name}-scroll-{side}"))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(SCROLL_BUTTON_WIDTH))
        .border_l_1()
        .border_color(rgb(BAR_BORDER))
        .text_color(rgb(INACTIVE_TAB_FOREGROUND))
        .cursor_pointer()
        .hover(|button| button.text_color(rgb(colors::DEFAULT_FOREGROUND)))
        .child(label)
        .on_click(cx.listener(move |_, _, _, cx| {
            scroll_by(&scroll, delta);
            cx.notify();
        }))
}

/// 横スクロール位置を `delta` だけ動かす（スクロールできる範囲に収める）。
fn scroll_by(scroll: &ScrollHandle, delta: f32) {
    let offset = scroll.offset();
    let min = -scroll.max_offset().width;
    let x = (offset.x + px(delta)).clamp(min, px(0.0));
    scroll.set_offset(point(x, offset.y));
}

/// workspace ライン・タブバーの 1 項目。アクティブなものは背景を本文と揃えて強調する。
/// 上下の段で区切りが揃うよう幅は固定し、収まらないラベルは省略する。
fn bar_item(active: bool, label: String) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(px(BAR_ITEM_WIDTH))
        .overflow_hidden()
        .px(px(12.0))
        .border_r_1()
        .border_color(rgb(BAR_BORDER))
        .text_color(rgb(if active {
            colors::DEFAULT_FOREGROUND
        } else {
            INACTIVE_TAB_FOREGROUND
        }))
        .when(active, |item| item.bg(rgb(colors::DEFAULT_BACKGROUND)))
        .child(div().min_w(px(0.0)).truncate().child(label))
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

/// workspace ラインに表示する名前。
fn workspace_label(index: usize) -> String {
    format!("Workspace {}", index + 1)
}

/// サイドバーで Agent がどの workspace・タブにいるかを表す。
fn agent_location(workspace_index: usize, tab_index: usize, title: Option<&str>) -> String {
    format!("W{} {}", workspace_index + 1, tab_label(tab_index, title))
}

fn window_title(title: Option<&str>) -> &str {
    title.unwrap_or(DEFAULT_TITLE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{App, TestAppContext, VisualTestContext};

    fn sleep_options() -> SpawnOptions {
        SpawnOptions {
            program: Some(("sleep".into(), vec!["30".into()])),
        }
    }

    /// 最初のペインで `first` を動かし、以降のペインは `sleep` を動かすワークスペースを開く。
    fn open_workspace_with(
        cx: &mut TestAppContext,
        first: SpawnOptions,
    ) -> (Entity<WorkspaceView>, &mut VisualTestContext) {
        let (terminal, events) = Terminal::spawn(first, INITIAL_SIZE).unwrap();
        cx.add_window_view(|window, cx| {
            WorkspaceView::new(terminal, events, sleep_options(), window, cx)
        })
    }

    /// シェルの代わりに終了しない軽いプログラムを動かすワークスペースを開く。
    fn open_workspace(cx: &mut TestAppContext) -> (Entity<WorkspaceView>, &mut VisualTestContext) {
        open_workspace_with(cx, sleep_options())
    }

    /// `claude` という名前で動く（Claude Code として検出される）偽の Agent。
    fn fake_claude() -> SpawnOptions {
        let dir = std::env::temp_dir().join(format!("kugutsu-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("claude");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        SpawnOptions {
            program: Some((script.to_string_lossy().into_owned(), Vec::new())),
        }
    }

    /// ポーリングを進め、条件を満たすまで待つ（PTY 上のプロセスは実時間で動くため）。
    fn poll_until(
        workspace: &Entity<WorkspaceView>,
        cx: &mut VisualTestContext,
        mut done: impl FnMut(&WorkspaceView, &App) -> bool,
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            cx.executor().advance_clock(AGENT_POLL_INTERVAL);
            cx.run_until_parked();
            if workspace.read_with(cx, |workspace, cx| done(workspace, cx)) {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "condition never met");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// アクティブな workspace の位置と、その中のアクティブなタブの位置。
    fn active_position(
        workspace: &Entity<WorkspaceView>,
        cx: &mut VisualTestContext,
    ) -> (usize, usize) {
        workspace.read_with(cx, |w, _| {
            (
                w.workspaces.active_index(),
                w.active_tabs().unwrap().active_index(),
            )
        })
    }

    fn workspace_count(workspace: &Entity<WorkspaceView>, cx: &mut VisualTestContext) -> usize {
        workspace.read_with(cx, |w, _| w.workspaces.iter().count())
    }

    fn active_tab_count(workspace: &Entity<WorkspaceView>, cx: &mut VisualTestContext) -> usize {
        workspace.read_with(cx, |w, _| w.active_tabs().unwrap().iter().count())
    }

    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn clicking_agent_in_sidebar_focuses_its_tab(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with(cx, fake_claude());
        cx.simulate_keystrokes("ctrl-b c");
        assert_eq!(active_position(&workspace, cx), (0, 1));

        poll_until(&workspace, cx, |workspace, cx| {
            let first_tab = workspace.active_tabs().unwrap().iter().next().unwrap();
            first_tab
                .active_view()
                .is_some_and(|view| view.read(cx).agent().is_some())
        });

        let row = cx.debug_bounds("agent-0").expect("agent row is rendered");
        cx.simulate_click(row.center(), gpui::Modifiers::none());
        assert_eq!(active_position(&workspace, cx), (0, 0));
    }

    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn clicking_agent_in_other_workspace_switches_to_it(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with(cx, fake_claude());
        cx.simulate_keystrokes("ctrl-b shift-c");
        assert_eq!(active_position(&workspace, cx), (1, 0));

        poll_until(&workspace, cx, |workspace, cx| {
            let first = workspace.workspaces.iter().next().unwrap();
            first
                .tabs
                .active()
                .and_then(Tab::active_view)
                .is_some_and(|view| view.read(cx).agent().is_some())
        });

        let row = cx.debug_bounds("agent-0").expect("agent row is rendered");
        cx.simulate_click(row.center(), gpui::Modifiers::none());
        assert_eq!(active_position(&workspace, cx), (0, 0));
    }

    #[gpui::test]
    fn prefix_shift_c_opens_new_workspace(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        assert_eq!(workspace_count(&workspace, cx), 1);

        cx.simulate_keystrokes("ctrl-b shift-c");
        assert_eq!(workspace_count(&workspace, cx), 2);
        assert_eq!(active_position(&workspace, cx), (1, 0));
        assert_eq!(active_tab_count(&workspace, cx), 1);
    }

    #[gpui::test]
    fn workspaces_keep_their_own_tabs(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.simulate_keystrokes("ctrl-b c");
        cx.simulate_keystrokes("ctrl-b shift-c");
        assert_eq!(active_tab_count(&workspace, cx), 1);

        cx.simulate_keystrokes("ctrl-b shift-p");
        assert_eq!(active_position(&workspace, cx), (0, 1));
        assert_eq!(active_tab_count(&workspace, cx), 2);

        cx.simulate_keystrokes("ctrl-b shift-n");
        assert_eq!(active_position(&workspace, cx), (1, 0));
        // 末尾からは先頭へ循環する
        cx.simulate_keystrokes("ctrl-b shift-n");
        assert_eq!(active_position(&workspace, cx), (0, 1));
    }

    #[gpui::test]
    fn clicking_workspace_selects_it(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.simulate_keystrokes("ctrl-b shift-c");
        assert_eq!(active_position(&workspace, cx), (1, 0));

        let item = cx
            .debug_bounds("workspace-0")
            .expect("workspace item is rendered");
        cx.simulate_click(item.center(), gpui::Modifiers::none());
        assert_eq!(active_position(&workspace, cx), (0, 0));
    }

    #[gpui::test]
    fn workspace_and_tab_items_have_same_width(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace(cx);
        cx.run_until_parked();

        let workspace_item = cx
            .debug_bounds("workspace-0")
            .expect("workspace item is rendered");
        let tab_item = cx.debug_bounds("tab-0").expect("tab item is rendered");
        // ラベルの長さ（"Workspace 1" と "1: shell"）が違っても幅は揃う
        assert_eq!(workspace_item.size.width, tab_item.size.width);
        assert_eq!(workspace_item.size.width, px(BAR_ITEM_WIDTH));
    }

    /// 項目が 5 個ほどしか並ばない幅のウィンドウで、`keys` を `count` 回押す。
    fn overflow_bar(cx: &mut VisualTestContext, keys: &str, count: usize) {
        cx.simulate_resize(gpui::size(px(800.0), px(500.0)));
        for _ in 0..count {
            cx.simulate_keystrokes(keys);
        }
        cx.run_until_parked();
    }

    fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> gpui::Bounds<gpui::Pixels> {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is rendered"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let center = bounds(cx, selector).center();
        cx.simulate_click(center, gpui::Modifiers::none());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn active_tab_stays_visible_when_tabs_overflow(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace(cx);
        overflow_bar(cx, "ctrl-b c", 8);

        // 末尾の（アクティブな）タブは右端のボタンより左に収まる
        let last = bounds(cx, "tab-8");
        let button = bounds(cx, "tab-scroll-left");
        assert!(last.right() <= button.left(), "{last:?} / {button:?}");

        // 先頭のタブへ切り替えると、そこが見える位置まで戻る
        cx.simulate_keystrokes("ctrl-b 1");
        cx.run_until_parked();
        let bar = bounds(cx, "tab-bar");
        assert!(bounds(cx, "tab-0").left() >= bar.left());
    }

    #[gpui::test]
    fn scroll_buttons_scroll_tab_bar_by_one_item(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace(cx);
        overflow_bar(cx, "ctrl-b c", 8);
        let start = bounds(cx, "tab-0").left();

        click(cx, "tab-scroll-left");
        assert_eq!(bounds(cx, "tab-0").left(), start + px(BAR_ITEM_WIDTH));

        click(cx, "tab-scroll-right");
        assert_eq!(bounds(cx, "tab-0").left(), start);
    }

    #[gpui::test]
    fn scroll_buttons_scroll_workspace_bar_by_one_item(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace(cx);
        overflow_bar(cx, "ctrl-b shift-c", 8);
        let last = bounds(cx, "workspace-8");
        assert!(last.right() <= bounds(cx, "workspace-scroll-left").left());
        let start = bounds(cx, "workspace-0").left();

        click(cx, "workspace-scroll-left");
        assert_eq!(bounds(cx, "workspace-0").left(), start + px(BAR_ITEM_WIDTH));

        click(cx, "workspace-scroll-right");
        assert_eq!(bounds(cx, "workspace-0").left(), start);
    }

    #[gpui::test]
    fn closing_last_tab_closes_workspace(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.simulate_keystrokes("ctrl-b shift-c");
        assert_eq!(workspace_count(&workspace, cx), 2);

        cx.simulate_keystrokes("ctrl-b x");
        assert_eq!(workspace_count(&workspace, cx), 1);
        assert_eq!(active_position(&workspace, cx), (0, 0));
    }

    #[test]
    fn workspace_label_numbers_from_one() {
        assert_eq!(workspace_label(0), "Workspace 1");
        assert_eq!(workspace_label(2), "Workspace 3");
    }

    #[test]
    fn agent_location_includes_workspace_and_tab() {
        assert_eq!(agent_location(0, 1, Some("vim")), "W1 2: vim");
        assert_eq!(agent_location(2, 0, None), "W3 1: shell");
    }

    #[gpui::test]
    fn prefix_b_toggles_sidebar(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        assert!(workspace.read_with(cx, |workspace, _| workspace.sidebar_visible));

        cx.simulate_keystrokes("ctrl-b b");
        assert!(!workspace.read_with(cx, |workspace, _| workspace.sidebar_visible));

        cx.simulate_keystrokes("ctrl-b b");
        assert!(workspace.read_with(cx, |workspace, _| workspace.sidebar_visible));
    }

    fn keymap_help_visible(workspace: &Entity<WorkspaceView>, cx: &mut VisualTestContext) -> bool {
        workspace.read_with(cx, |workspace, _| workspace.keymap_help_visible)
    }

    #[gpui::test]
    fn prefix_question_shows_keymap_help(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        assert!(!keymap_help_visible(&workspace, cx));
        assert!(cx.debug_bounds("keymap-help").is_none());

        cx.simulate_keystrokes("ctrl-b ?");
        assert!(keymap_help_visible(&workspace, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("keymap-help").is_some());
    }

    #[gpui::test]
    fn keymap_help_closes_with_escape_q_or_question(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        for key in ["escape", "q", "?"] {
            cx.simulate_keystrokes("ctrl-b ?");
            assert!(keymap_help_visible(&workspace, cx), "{key}");
            cx.simulate_keystrokes(key);
            assert!(!keymap_help_visible(&workspace, cx), "{key}");
        }
    }

    #[gpui::test]
    fn keys_are_consumed_while_keymap_help_is_open(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.simulate_keystrokes("ctrl-b ?");
        cx.simulate_keystrokes("ctrl-b c");
        assert_eq!(active_tab_count(&workspace, cx), 1);
        assert!(keymap_help_visible(&workspace, cx));
    }

    #[gpui::test]
    fn clicking_backdrop_closes_keymap_help(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.simulate_keystrokes("ctrl-b ?");
        cx.run_until_parked();

        // パネルの外側（背景の左上隅）をクリックする
        let backdrop = bounds(cx, "keymap-help-backdrop");
        let corner = backdrop.origin + gpui::point(px(5.0), px(5.0));
        cx.simulate_click(corner, gpui::Modifiers::none());
        assert!(!keymap_help_visible(&workspace, cx));
    }

    #[gpui::test]
    fn prefix_shows_which_key_above_status_line(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace(cx);
        cx.run_until_parked();
        assert!(cx.debug_bounds("which-key").is_none());
        let panes_before = bounds(cx, "panes");

        cx.simulate_keystrokes("ctrl-b");
        cx.run_until_parked();
        let which_key = bounds(cx, "which-key");
        assert_eq!(which_key.bottom(), bounds(cx, "status-line").top());
        // ターミナルを縮めず（リサイズさせず）上に重ねる
        assert_eq!(bounds(cx, "panes"), panes_before);

        cx.simulate_keystrokes("b");
        cx.run_until_parked();
        assert!(cx.debug_bounds("which-key").is_none());
    }

    #[gpui::test]
    fn which_key_hides_after_unknown_key(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace(cx);
        cx.simulate_keystrokes("ctrl-b z");
        cx.run_until_parked();
        assert!(cx.debug_bounds("which-key").is_none());
    }

    #[gpui::test]
    fn clicking_keymap_help_panel_keeps_it_open(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.simulate_keystrokes("ctrl-b ?");
        cx.run_until_parked();

        click(cx, "keymap-help");
        assert!(keymap_help_visible(&workspace, cx));
    }

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
