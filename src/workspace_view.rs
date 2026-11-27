//! ウィンドウのルートビュー。複数のタブを持ち、各タブはターミナルを分割したペインを並べる。
//! タブバーとアクティブなタブのペインを描画する。

use alacritty_terminal::event::Event;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    Animation, AnimationExt, AnyElement, App, BoxShadow, Context, ElementId, Entity, Focusable,
    FontWeight, KeyDownEvent, MouseButton, Render, Rgba, ScrollHandle, SharedString, Stateful,
    Subscription, Svg, Task, Transformation, Window, div, percentage, point, prelude::*,
    pulsating_between, px, rgb, rgba, svg,
};

use std::time::Duration;

use crate::agents::{AgentKind, AgentState};
use crate::config::{Config, Loaded};
use crate::icons::{Icon, state_icon};
use crate::panes::{Axis, Node, PaneId, Panes};
use crate::tabs::Tabs;
use crate::tabs::prefix::{Action, Command, KeyOutcome, PrefixKeymap};
use crate::terminal::size::GridSize;
use crate::terminal::{SpawnOptions, Terminal};
use crate::terminal_view::{TerminalView, TerminalViewEvent};
use crate::theme::{Theme, UiColors};

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
const WORKSPACE_BAR_HEIGHT: f32 = 34.0;
const TAB_BAR_HEIGHT: f32 = 34.0;
/// UI の文字の大きさ（補足・本文・見出し）。
const TEXT_SM: f32 = 11.0;
const TEXT_MD: f32 = 12.0;
const TEXT_LG: f32 = 13.0;
/// working のアイコンが 1 回転する時間と、blocked のアイコンが点滅する周期。
const SPINNER_PERIOD: Duration = Duration::from_millis(1200);
const PULSE_PERIOD: Duration = Duration::from_millis(1600);
/// workspace ライン・タブバーの左右の余白と、項目の間隔・角丸。
const BAR_PADDING: f32 = 8.0;
const BAR_ITEM_GAP: f32 = 4.0;
const BAR_ITEM_RADIUS: f32 = 6.0;
/// バーの項目とボタンの高さ（ボタンは正方形）。
const BAR_BUTTON_SIZE: f32 = 24.0;
/// ペインの間とペイン領域の縁の余白、ペインの角丸、ペインの内側の余白。
const PANE_GAP: f32 = 6.0;
const PANE_RADIUS: f32 = 8.0;
const PANE_PADDING: f32 = 6.0;
const STATUS_LINE_HEIGHT: f32 = 26.0;
const SIDEBAR_WIDTH: f32 = 260.0;
/// Agent の状態を調べる間隔。
const AGENT_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// キーマップ一覧の後ろに敷く半透明の背景（RGBA）。
const KEYMAP_BACKDROP: u32 = 0x000000aa;
/// キーマップ一覧のキー列の幅。
const KEYMAP_KEY_COLUMN_WIDTH: f32 = 130.0;
/// which-key の 1 項目（キー＋説明）の幅と、そのうちキー部分の幅。
const WHICH_KEY_ITEM_WIDTH: f32 = 220.0;
const WHICH_KEY_KEY_WIDTH: f32 = 64.0;
/// 浮いたパネルと画面の端・ステータスラインとの間隔。
const OVERLAY_MARGIN: f32 = 8.0;

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
    /// ユーザー設定（`init.lua`）。
    config: Config,
    /// `init.lua` の読み込みで起きたエラー。ステータスラインに表示する。
    config_errors: Vec<String>,
    /// spec にあるが、まだインストールされていないプラグインの名前。
    missing_plugins: Vec<String>,
    /// UI に使うフォント。`None` なら gpui の既定。
    ui_font: Option<SharedString>,
    _agent_poll: Task<()>,
}

impl WorkspaceView {
    /// 起動済みのターミナルを最初のタブとして持つワークスペースを作る。
    /// 以降に開くタブ・ペインでは `spawn_options` のプログラムを起動する。
    pub fn new(
        terminal: Terminal,
        events: UnboundedReceiver<Event>,
        spawn_options: SpawnOptions,
        config: Loaded,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Loaded {
            config,
            errors: config_errors,
            missing_plugins,
        } = config;
        let first = Self::create_pane(terminal, events, &config, window, cx);
        let ui_font = crate::font::resolve_ui_family(
            config.ui_font_family.as_deref(),
            &window.text_system().all_font_names(),
        )
        .map(SharedString::from);
        let this = Self {
            workspaces: Tabs::new(Workspace::new(first)),
            keymap: PrefixKeymap::new(config.prefix.clone(), config.keymap.clone()),
            config,
            config_errors,
            missing_plugins,
            ui_font,
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
        config: &Config,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pane {
        let view = cx.new(|cx| TerminalView::new(terminal, events, config, window, cx));
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
            Ok((terminal, events)) => Some(Self::create_pane(
                terminal,
                events,
                &self.config,
                window,
                cx,
            )),
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
            KeyOutcome::Action(action) => {
                cx.stop_propagation();
                self.perform(action, window, cx);
            }
        }
        if was_armed != self.keymap.is_armed() {
            cx.notify();
        }
    }

    /// キーに割り当てた動作を実行する。Lua 関数のエラーはステータスラインに出す。
    fn perform(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let commands = match action {
            Action::Command(command) => vec![command],
            Action::Lua(callback) => match callback.call() {
                Ok(commands) => commands,
                Err(err) => {
                    self.config_errors.push(err);
                    cx.notify();
                    return;
                }
            },
        };
        for command in commands {
            self.run(command, window, cx);
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
        let groups = self.agent_groups(cx);
        let items =
            groups
                .iter()
                .map(|group| {
                    let index = group.index;
                    let dots = agent_dots(group.agents.iter());
                    bar_item(
                        &format!("workspace-{index}"),
                        index == active_index,
                        Icon::Workspace,
                        workspace_label(index),
                        &dots,
                        &self.config,
                    )
                    .id(("workspace", index))
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
            Command::NewWorkspace,
            &self.config,
            cx,
        )
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut items = Vec::new();
        let groups = self.agent_groups(cx);
        let agents = groups
            .iter()
            .find(|group| group.active)
            .map_or(&[][..], |group| &group.agents[..]);
        if let Some(tabs) = self.active_tabs() {
            let active_index = tabs.active_index();
            for (index, tab) in tabs.iter().enumerate() {
                let title = tab.active_view().and_then(|view| view.read(cx).title());
                let dots = agent_dots(agents.iter().filter(|agent| agent.tab_index == index));
                items.push(
                    bar_item(
                        &format!("tab-{index}"),
                        index == active_index,
                        Icon::Terminal,
                        tab_label(index, title),
                        &dots,
                        &self.config,
                    )
                    .id(("tab", index))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run(Command::SelectTab(index), window, cx)
                    })),
                );
            }
        }
        scrollable_bar(
            "tab",
            TAB_BAR_HEIGHT,
            &self.tab_scroll,
            items,
            Command::NewTab,
            &self.config,
            cx,
        )
    }

    /// アクティブなタブのペインを分割どおりに並べる。
    fn render_panes(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panes = &self.active_tab()?.panes;
        let root = panes.root()?;
        // ペインが 1 つだけなら強調も減光もしない
        let highlight = matches!(root, Node::Split { .. }).then_some(panes.active_id());
        let order: Vec<PaneId> = panes.iter().map(|(id, _)| id).collect();
        Some(render_node(root, highlight, &order, &self.config, cx))
    }

    /// 全 workspace・全タブ・全ペインで動いている Agent を workspace ごとにまとめる。
    fn agent_groups(&self, cx: &App) -> Vec<WorkspaceGroup> {
        let active_workspace = self.workspaces.active_index();
        self.workspaces
            .iter()
            .enumerate()
            .map(|(workspace_index, workspace)| {
                let active = workspace_index == active_workspace;
                let active_tab = workspace.tabs.active_index();
                let mut agents = Vec::new();
                for (tab_index, tab) in workspace.tabs.iter().enumerate() {
                    for (pane_number, (pane_id, pane)) in tab.panes.iter().enumerate() {
                        let view = pane.view.read(cx);
                        let Some(agent) = view.agent() else { continue };
                        agents.push(AgentRow {
                            workspace_index,
                            tab_index,
                            pane_id,
                            kind: agent.kind,
                            state: agent.state,
                            location: agent_location(tab_index, view.title(), pane_number + 1),
                            active: active
                                && tab_index == active_tab
                                && pane_id == tab.panes.active_id(),
                        });
                    }
                }
                WorkspaceGroup {
                    index: workspace_index,
                    active,
                    agents,
                }
            })
            .collect()
    }

    /// 全 workspace・全タブ・全ペインで動いている Agent の一覧。クリックでそのペインへ移る。
    /// workspace ごとに見出しを置き、その下に Agent を並べる。
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = &self.config.theme.ui;
        let groups = self.agent_groups(cx);
        let (blocked, total) = self.sidebar_counts(&groups);
        let mut next_row = 0;
        let mut elements = Vec::new();
        for group in groups {
            let first_row = next_row;
            next_row += group.agents.len();
            elements.push(self.render_agent_group(group, first_row, cx));
        }

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .overflow_hidden()
            .text_size(px(TEXT_MD))
            .bg(self.config.translucent(ui.bar_background))
            .border_r_1()
            .border_color(rgb(ui.bar_border))
            .child(
                // workspace ラインと高さを揃える
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .h(px(WORKSPACE_BAR_HEIGHT))
                    .px(px(16.0))
                    .text_size(px(TEXT_LG))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(ui.foreground))
                    .child("kugutsu"),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(10.0))
                    .h(px(TAB_BAR_HEIGHT))
                    .px(px(14.0))
                    .text_size(px(TEXT_SM))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(ui.inactive_foreground))
                            .child("AGENTS"),
                    )
                    .children(blocked.map(|blocked| {
                        div()
                            .debug_selector(|| "agents-blocked-count".into())
                            .text_color(rgb(ui.agent_blocked))
                            .child(blocked)
                    }))
                    .child(div().text_color(rgb(ui.inactive_foreground)).child(total)),
            )
            .child(
                div()
                    .id("agent-groups")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .gap(px(10.0))
                    .px(px(10.0))
                    .pb(px(10.0))
                    .children(elements),
            )
    }

    /// サイドバーのヘッダに出す blocked の数（0 なら `None`）と Agent の総数。
    fn sidebar_counts(&self, groups: &[WorkspaceGroup]) -> (Option<String>, String) {
        let counts = AgentCounts::of(groups);
        let blocked = (counts.blocked > 0).then(|| format!("{} blocked", counts.blocked));
        (blocked, format!("{} total", counts.total()))
    }

    /// workspace の見出しと、細い縦線（操り糸）でつないだ Agent の行。
    /// `first_row` はこの workspace の最初の行の通し番号（`agent-{n}` のセレクタに使う）。
    fn render_agent_group(
        &self,
        group: WorkspaceGroup,
        first_row: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ui = &self.config.theme.ui;
        let index = group.index;
        let note = if group.agents.is_empty() {
            Some(("no agents", format!("agent-group-{index}-empty")))
        } else if group.active {
            Some(("current", format!("agent-group-{index}-current")))
        } else {
            None
        };
        let heading_color = rgb(if group.active {
            ui.foreground
        } else {
            ui.inactive_foreground
        });
        let heading = div()
            .debug_selector(move || format!("agent-group-{index}"))
            .flex()
            .items_center()
            .gap(px(6.0))
            .h(px(24.0))
            .px(px(4.0))
            .child(icon(Icon::Workspace, 13.0).text_color(heading_color))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(TEXT_LG))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(heading_color)
                    .child(workspace_label(index)),
            )
            .children(note.map(|(text, selector)| {
                div()
                    .debug_selector(move || selector)
                    .flex_none()
                    .text_size(px(TEXT_SM))
                    .text_color(rgb(ui.inactive_foreground))
                    .child(text)
            }));
        let mut rows = Vec::new();
        for (offset, agent) in group.agents.into_iter().enumerate() {
            rows.push(self.render_agent_row(agent, first_row + offset, cx));
        }
        div()
            .flex()
            .flex_col()
            .child(heading)
            .when(!rows.is_empty(), |group| {
                group.child(
                    div()
                        .debug_selector(move || format!("agent-group-{index}-strings"))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .ml(px(10.0))
                        .py(px(2.0))
                        .border_l_1()
                        .border_color(rgb(ui.strong_border))
                        .children(rows),
                )
            })
            .into_any_element()
    }

    /// サイドバーの Agent の 1 行。クリックでそのペインへ移る。
    fn render_agent_row(&self, agent: AgentRow, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let ui = &self.config.theme.ui;
        let color = state_color(ui, agent.state);
        let blocked = agent.state == AgentState::Blocked;
        let AgentRow {
            workspace_index,
            tab_index,
            pane_id,
            ..
        } = agent;
        let item = div()
            .id(("agent", row))
            .debug_selector(move || format!("agent-{row}"))
            .flex()
            .flex_1()
            .min_w(px(0.0))
            .items_center()
            .gap(px(10.0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(Rgba::default())
            .cursor_pointer()
            .hover(|item| item.bg(rgb(ui.raised_background)))
            .when(agent.active, |item| item.bg(rgb(ui.raised_background)))
            .when(blocked, |item| {
                item.bg(with_alpha(ui.agent_blocked, 0.10))
                    .border_color(with_alpha(ui.agent_blocked, 0.45))
            })
            .child(state_icon_element(agent.state, color, ("agent-icon", row)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(TEXT_LG))
                            .text_color(rgb(ui.foreground))
                            .child(agent.kind.label()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(TEXT_SM))
                            .text_color(rgb(ui.inactive_foreground))
                            .child(agent.location),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(TEXT_SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(color))
                    .child(agent.state.label()),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.focus_agent_pane(workspace_index, tab_index, pane_id, window, cx)
            }));
        // 縦線から各行へ短い横線を伸ばす
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .flex_none()
                    .w(px(8.0))
                    .h(px(1.0))
                    .bg(rgb(ui.strong_border)),
            )
            .child(item)
            .into_any_element()
    }

    /// ステータスラインに出す、今いる workspace・タブ・ペインの場所。
    fn status_location(&self, cx: &App) -> Option<String> {
        let tab = self.active_tab()?;
        let title = tab.active_view().and_then(|view| view.read(cx).title());
        let tab_index = self.active_tabs()?.active_index();
        let active = tab.panes.active_id();
        let pane_number = tab.panes.iter().position(|(id, _)| id == active)? + 1;
        Some(status_location(
            self.workspaces.active_index(),
            &tab_label(tab_index, title),
            pane_number,
            tab.panes.iter().count(),
        ))
    }

    /// 画面下部のステータスライン。左から prefix キー、今いる場所、状態ごとの Agent の数、
    /// 設定のエラーを並べる。prefix 入力待ちのときは prefix キーを強調する。
    fn render_status_line(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = &self.config.theme.ui;
        let armed = self.keymap.is_armed();
        let counts = AgentCounts::of(&self.agent_groups(cx));
        let agents = status_agents(counts).into_iter().map(|(state, count)| {
            let color = state_color(ui, state);
            div()
                .debug_selector(move || format!("status-agents-{}", state.label()))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.0))
                .ml(px(12.0))
                .text_color(rgb(if state == AgentState::Working {
                    ui.foreground
                } else {
                    color
                }))
                .child(icon(state_icon(state), 12.0).text_color(rgb(color)))
                .child(format!("{count} {}", state.label()))
        });
        let prefix = div()
            .debug_selector(|| "status-prefix".into())
            .flex()
            .flex_none()
            .items_center()
            .gap(px(5.0))
            .h(px(STATUS_LINE_HEIGHT - 6.0))
            .px(px(7.0))
            .rounded(px(4.0))
            .border_1()
            .map(|chip| {
                if armed {
                    chip.bg(rgb(ui.raised_background))
                        .border_color(rgb(ui.prefix_indicator))
                        .text_color(rgb(ui.prefix_indicator))
                } else {
                    chip.border_color(rgb(ui.strong_border))
                        .text_color(rgb(ui.inactive_foreground))
                }
            })
            .child(icon(Icon::Keyboard, 12.0).text_color(rgb(if armed {
                ui.prefix_indicator
            } else {
                ui.inactive_foreground
            })))
            .child(self.prefix_label());
        div()
            .debug_selector(|| "status-line".into())
            .flex()
            .flex_none()
            .items_center()
            .h(px(STATUS_LINE_HEIGHT))
            .px(px(8.0))
            .text_size(px(TEXT_SM))
            .bg(self.config.translucent(ui.bar_background))
            .border_t_1()
            .border_color(rgb(ui.bar_border))
            .child(prefix)
            // 場所の表示が残りの幅を取り、Agent の数とエラーを右端に寄せる
            .child(
                div()
                    .debug_selector(|| "status-location".into())
                    .flex_1()
                    .min_w(px(0.0))
                    .ml(px(12.0))
                    .truncate()
                    .text_color(rgb(ui.inactive_foreground))
                    .children(self.status_location(cx)),
            )
            .children(agents)
            .children(self.missing_plugins_message().map(|message| {
                div()
                    .debug_selector(|| "plugins-missing".into())
                    .flex_none()
                    .ml(px(12.0))
                    .text_color(rgb(self.config.theme.ui.error))
                    .child(message)
            }))
            .children(self.config_error_message().map(|message| {
                div()
                    .debug_selector(|| "config-error".into())
                    .ml(px(12.0))
                    .min_w(px(0.0))
                    .truncate()
                    .text_color(rgb(self.config.theme.ui.error))
                    .child(message)
            }))
    }

    /// 表示用の prefix キー（gpui の表記）。
    fn prefix_label(&self) -> String {
        self.keymap.prefix().unparse()
    }

    /// ステータスラインに出す設定のエラー。エラーが無ければ `None`。
    fn config_error_message(&self) -> Option<String> {
        config_error_message(&self.config_errors)
    }

    /// ステータスラインに出す、インストールされていないプラグインの数。全部あれば `None`。
    fn missing_plugins_message(&self) -> Option<String> {
        (!self.missing_plugins.is_empty()).then(|| {
            format!(
                "plugins: {} missing — run kugutsu install",
                self.missing_plugins.len()
            )
        })
    }

    /// prefix 入力待ちのとき、ステータスラインのすぐ上に重ねて出す次のキーの候補
    /// （vim の which-key 風）。ターミナルを縮めてリサイズさせないよう、レイアウトには
    /// 参加させずに重ねる。
    fn render_which_key(&self) -> impl IntoElement {
        let items = self
            .keymap
            .keymap()
            .entries()
            .into_iter()
            .map(|(key, description)| {
                let selector = format!("which-key:{key}:{description}");
                div()
                    .debug_selector(move || selector)
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .w(px(WHICH_KEY_ITEM_WIDTH))
                    .overflow_hidden()
                    .child(
                        div().flex().flex_none().w(px(WHICH_KEY_KEY_WIDTH)).child(
                            keycap(&self.config.theme.ui, &key)
                                .debug_selector(move || format!("which-key-cap:{key}")),
                        ),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .truncate()
                            .text_color(rgb(self.config.theme.ui.foreground))
                            .child(description),
                    )
            });
        // ステータスラインの上に浮かせたパネル。画面を縦に埋めないよう横幅いっぱいに並べる
        overlay_panel(&self.config.theme.ui)
            .debug_selector(|| "which-key".into())
            .absolute()
            .left(px(OVERLAY_MARGIN))
            .right(px(OVERLAY_MARGIN))
            .bottom(px(STATUS_LINE_HEIGHT + OVERLAY_MARGIN))
            .flex()
            .flex_wrap()
            .gap_y(px(6.0))
            .px(px(14.0))
            .py(px(10.0))
            .text_size(px(TEXT_MD))
            .children(items)
    }

    /// ウィンドウ全体に重ねるキーマップ一覧。背景をクリックすると閉じる。
    fn render_keymap_help(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = &self.config.theme.ui;
        // 分類ごとに見出しを付け、prefix 付きの割り当ての後に prefix を使わない割り当てを並べる
        let groups = self
            .keymap
            .keymap()
            .help_groups(&self.prefix_label())
            .into_iter()
            .map(|(group, entries)| {
                let rows = entries.into_iter().map(|(key, description)| {
                    let selector = format!("keymap-help:{key}:{description}");
                    div()
                        .debug_selector(move || selector)
                        .flex()
                        .items_center()
                        .gap(px(16.0))
                        .child(
                            div()
                                .flex()
                                .w(px(KEYMAP_KEY_COLUMN_WIDTH))
                                .flex_none()
                                .child(keycap(ui, &key)),
                        )
                        .child(
                            div()
                                .min_w(px(0.0))
                                .truncate()
                                .text_color(rgb(ui.foreground))
                                .child(description),
                        )
                });
                let name = group.label().to_lowercase();
                div()
                    .debug_selector(move || format!("keymap-help-group-{name}"))
                    .flex()
                    .flex_col()
                    .gap(px(5.0))
                    .child(
                        div()
                            .pb(px(2.0))
                            .text_size(px(TEXT_SM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(ui.inactive_foreground))
                            .child(group.label().to_uppercase()),
                    )
                    .children(rows)
            });
        let panel = overlay_panel(ui)
            .id("keymap-help")
            .debug_selector(|| "keymap-help".into())
            .occlude()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .p(px(20.0))
            .text_size(px(TEXT_MD))
            .child(
                div()
                    .text_size(px(TEXT_LG))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(ui.foreground))
                    .child("Key bindings"),
            )
            .children(groups)
            .child(
                div()
                    .text_size(px(TEXT_SM))
                    .text_color(rgb(ui.inactive_foreground))
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

/// which-key やキーマップ一覧に使う、浮いたパネル。
fn overlay_panel(ui: &UiColors) -> gpui::Div {
    div()
        .rounded(px(8.0))
        .border_1()
        .border_color(rgb(ui.strong_border))
        .bg(rgb(ui.overlay_background))
        .shadow(vec![BoxShadow {
            color: with_alpha(0x000000, 0.45).into(),
            offset: point(px(0.0), px(6.0)),
            blur_radius: px(20.0),
            spread_radius: px(0.0),
        }])
}

/// キーをキーキャップ風に描く。
fn keycap(ui: &UiColors, key: &str) -> gpui::Div {
    div()
        .flex_none()
        .px(px(6.0))
        .py(px(1.0))
        .rounded(px(4.0))
        .border_1()
        .border_b_2()
        .border_color(rgb(ui.strong_border))
        .bg(rgb(ui.raised_background))
        .text_size(px(TEXT_SM))
        .text_color(rgb(ui.foreground))
        .child(key.to_string())
}

/// 分割の中でのペインの立場。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneState {
    /// 分割されていない、ただ 1 つのペイン。
    Only,
    Active,
    Inactive,
}

/// ペイン木の節を描画する。分割は子を均等な大きさで並べ、間を少し空ける。
/// `highlight` が指すペインは枠とグローで強調し、ほかのペインは薄くする。
/// `order` はペインを左上から並べた順で、デバッグ用セレクタ `pane-{n}` の番号に使う。
fn render_node(
    node: &Node<Pane>,
    highlight: Option<PaneId>,
    order: &[PaneId],
    config: &Config,
    cx: &mut Context<WorkspaceView>,
) -> AnyElement {
    match node {
        Node::Leaf { id, item } => {
            let id = *id;
            let number = order.iter().position(|&candidate| candidate == id);
            let state = match highlight {
                None => PaneState::Only,
                Some(active) if active == id => PaneState::Active,
                Some(_) => PaneState::Inactive,
            };
            let theme = &config.theme;
            div()
                .when_some(number, |pane, number| {
                    pane.debug_selector(move || format!("pane-{number}"))
                })
                .size_full()
                .p(px(PANE_PADDING))
                .overflow_hidden()
                .rounded(px(PANE_RADIUS))
                .border_1()
                .border_color(pane_border_color(theme, state == PaneState::Active))
                .bg(config.translucent(theme.terminal.background))
                .opacity(pane_opacity(config, state))
                .when(state == PaneState::Active, |pane| {
                    pane.shadow(vec![BoxShadow {
                        color: with_alpha(theme.ui.accent, 0.18).into(),
                        offset: point(px(0.0), px(0.0)),
                        blur_radius: px(12.0),
                        spread_radius: px(0.0),
                    }])
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| this.focus_pane(id, window, cx)),
                )
                .child(item.view.clone())
                .into_any_element()
        }
        Node::Split { axis, children } => {
            let mut container = div().size_full().flex().gap(px(PANE_GAP));
            container = match axis {
                Axis::Horizontal => container.flex_row(),
                Axis::Vertical => container.flex_col(),
            };
            for child in children {
                container = container.child(
                    // 中身の大きさに引きずられず均等に分けるため、基準サイズ 0 から伸ばす
                    div()
                        .flex_1()
                        .flex_basis(px(0.0))
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .child(render_node(child, highlight, order, config, cx)),
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
            .when_some(self.ui_font.clone(), |root, family| {
                root.font_family(family)
            })
            .flex()
            .flex_col()
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
                                    .bg(self
                                        .config
                                        .translucent(self.config.theme.ui.bar_background))
                                    .p(px(PANE_GAP))
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .overflow_hidden()
                                    .children(self.render_panes(cx)),
                            ),
                    ),
            )
            .child(self.render_status_line(cx))
            .when(self.keymap.is_armed(), |root| {
                root.child(self.render_which_key())
            })
            .when(self.keymap_help_visible, |root| {
                root.child(self.render_keymap_help(cx))
            })
    }
}

/// workspace ライン・タブバー。項目は横スクロールし、右端の `<` `>` で 1 項目分ずつ動かす。
/// 項目の後ろの `+` で `new_command` を実行する。
/// `name` はデバッグ用セレクタ（`{name}-bar`, `{name}-scroll-left`, `{name}-new` など）の接頭辞。
fn scrollable_bar(
    name: &'static str,
    height: f32,
    scroll: &ScrollHandle,
    items: Vec<Stateful<gpui::Div>>,
    new_command: Command,
    config: &Config,
    cx: &mut Context<WorkspaceView>,
) -> gpui::Div {
    let ui = &config.theme.ui;
    let step = bar_item_step(config);
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(height))
        .px(px(BAR_PADDING))
        .text_size(px(TEXT_MD))
        .bg(config.translucent(ui.bar_background))
        .border_b_1()
        .border_color(rgb(ui.bar_border))
        .child(
            div()
                .id(SharedString::from(format!("{name}-bar")))
                .debug_selector(move || format!("{name}-bar"))
                .flex()
                .flex_1()
                .min_w(px(0.0))
                .items_center()
                .gap(px(BAR_ITEM_GAP))
                .overflow_x_scroll()
                .track_scroll(scroll)
                .children(items)
                .child(bar_button(name, "new", Icon::Plus, ui).on_click(
                    cx.listener(move |this, _, window, cx| this.run(new_command, window, cx)),
                )),
        )
        .child(scroll_button(
            name,
            "left",
            Icon::ChevronLeft,
            step,
            scroll,
            ui,
            cx,
        ))
        .child(scroll_button(
            name,
            "right",
            Icon::ChevronRight,
            -step,
            scroll,
            ui,
            cx,
        ))
}

/// バーの小さなアイコンのボタン（`{name}-{action}`）。
fn bar_button(
    name: &'static str,
    action: &'static str,
    glyph: Icon,
    ui: &UiColors,
) -> Stateful<gpui::Div> {
    let hover_background = rgb(ui.raised_background);
    div()
        .id(SharedString::from(format!("{name}-{action}")))
        .debug_selector(move || format!("{name}-{action}"))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(BAR_BUTTON_SIZE))
        .rounded(px(BAR_ITEM_RADIUS))
        .text_color(rgb(ui.inactive_foreground))
        .cursor_pointer()
        .hover(move |button| button.bg(hover_background))
        // svg は親の文字色を継承しないので色を直接付ける
        .child(icon(glyph, 12.0).text_color(rgb(ui.inactive_foreground)))
}

/// バーを `delta` だけ横にスクロールするボタン（正の値で左端側へ戻る）。
fn scroll_button(
    name: &'static str,
    side: &'static str,
    glyph: Icon,
    delta: f32,
    scroll: &ScrollHandle,
    ui: &UiColors,
    cx: &mut Context<WorkspaceView>,
) -> impl IntoElement {
    let scroll = scroll.clone();
    let action = if side == "left" {
        "scroll-left"
    } else {
        "scroll-right"
    };
    bar_button(name, action, glyph, ui).on_click(cx.listener(move |_, _, _, cx| {
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

/// バーの項目の幅と間隔を足した、スクロールボタンで 1 項目分動かす量。
fn bar_item_step(config: &Config) -> f32 {
    config.ui.item_width + BAR_ITEM_GAP
}

/// 項目に並べる Agent の状態のドット。注意が必要な順に、状態ごとに 1 つ。
fn agent_dots<'a>(agents: impl Iterator<Item = &'a AgentRow>) -> Vec<AgentState> {
    let states: Vec<_> = agents.map(|agent| agent.state).collect();
    [
        AgentState::Blocked,
        AgentState::Working,
        AgentState::Done,
        AgentState::Idle,
    ]
    .into_iter()
    .filter(|state| states.contains(state))
    .collect()
}

/// workspace ライン・タブバーの 1 項目（デバッグ用セレクタは `selector`）。アクティブなものは
/// 背景を浮かせて強調する。上下の段で揃うよう幅は固定し、収まらないラベルは省略する。
/// `dots` の Agent の状態をラベルの後ろに色付きの点で示す。
fn bar_item(
    selector: &str,
    active: bool,
    glyph: Icon,
    label: String,
    dots: &[AgentState],
    config: &Config,
) -> gpui::Div {
    let ui = &config.theme.ui;
    let text_color = if active {
        ui.foreground
    } else {
        ui.inactive_foreground
    };
    let hover_color = rgb(ui.foreground);
    let item_selector = selector.to_string();
    let dots = dots.iter().map(|&state| {
        let dot_selector = format!("{selector}-dot-{}", state.label());
        div()
            .debug_selector(move || dot_selector)
            .flex_none()
            .size(px(6.0))
            .rounded_full()
            .bg(rgb(state_color(ui, state)))
    });
    div()
        .debug_selector(move || item_selector)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .w(px(config.ui.item_width))
        .h(px(BAR_BUTTON_SIZE))
        .overflow_hidden()
        .px(px(10.0))
        .rounded(px(BAR_ITEM_RADIUS))
        .border_1()
        .border_color(Rgba::default())
        .text_color(rgb(text_color))
        .cursor_pointer()
        .hover(move |item| item.text_color(hover_color))
        .when(active, |item| {
            item.bg(rgb(ui.raised_background))
                .border_color(rgb(ui.strong_border))
                .font_weight(FontWeight::MEDIUM)
        })
        .child(icon(glyph, 12.0).text_color(rgb(text_color)))
        .child(div().flex_1().min_w(px(0.0)).truncate().child(label))
        .children(dots)
}

/// 同梱の SVG アイコン（色は `text_color` で付ける）。
fn icon(icon: Icon, size: f32) -> Svg {
    svg().path(icon.path()).flex_none().size(px(size))
}

/// Agent の状態のアイコン。working は回し、blocked は点滅させて注意を引く。
fn state_icon_element(state: AgentState, color: u32, id: impl Into<ElementId>) -> AnyElement {
    let element = icon(state_icon(state), 14.0).text_color(rgb(color));
    match state {
        AgentState::Working => element
            .with_animation(id, Animation::new(SPINNER_PERIOD).repeat(), |svg, delta| {
                svg.with_transformation(Transformation::rotate(percentage(delta)))
            })
            .into_any_element(),
        AgentState::Blocked => element
            .with_animation(
                id,
                Animation::new(PULSE_PERIOD)
                    .repeat()
                    .with_easing(pulsating_between(0.35, 1.0)),
                |svg, delta| svg.opacity(delta),
            )
            .into_any_element(),
        AgentState::Done | AgentState::Idle => element.into_any_element(),
    }
}

/// 色 `color` に不透明度 `alpha` を付ける。
fn with_alpha(color: u32, alpha: f32) -> Rgba {
    Rgba {
        a: alpha,
        ..rgb(color)
    }
}

/// サイドバーで Agent の状態を表す色。
fn state_color(ui: &UiColors, state: AgentState) -> u32 {
    match state {
        AgentState::Working => ui.agent_working,
        AgentState::Blocked => ui.agent_blocked,
        AgentState::Done => ui.agent_done,
        AgentState::Idle => ui.inactive_foreground,
    }
}

/// ペインの枠の色。分割時のアクティブなペインだけを目立たせる。
fn pane_border_color(theme: &Theme, active: bool) -> Rgba {
    rgb(if active {
        theme.ui.active_pane_border
    } else {
        theme.ui.strong_border
    })
}

/// ペインの不透明度。分割時に非アクティブなペインだけを薄くする。
fn pane_opacity(config: &Config, state: PaneState) -> f32 {
    match state {
        PaneState::Inactive => config.inactive_pane_opacity,
        PaneState::Active | PaneState::Only => 1.0,
    }
}

/// 設定のエラーの表示。最初のエラーを出し、ほかにもあれば件数を添える。
fn config_error_message(errors: &[String]) -> Option<String> {
    let (first, rest) = errors.split_first()?;
    Some(if rest.is_empty() {
        first.clone()
    } else {
        format!("{first} (+{})", rest.len())
    })
}

/// タブバーに表示する名前（番号は prefix + 数字キーで選ぶときの番号）。
fn tab_label(index: usize, title: Option<&str>) -> String {
    format!("{}: {}", index + 1, title.unwrap_or(UNTITLED_TAB))
}

/// workspace ラインに表示する名前。
fn workspace_label(index: usize) -> String {
    format!("Workspace {}", index + 1)
}

/// サイドバーで Agent が workspace の中のどのタブ・ペインにいるかを表す。
/// `pane_number` はタブの中で左上から数えた 1 始まりの番号。
fn agent_location(tab_index: usize, title: Option<&str>, pane_number: usize) -> String {
    format!("{} · pane {pane_number}", tab_label(tab_index, title))
}

/// ステータスラインに出す場所（`Workspace 1 › 1: kugutsu › pane 1/3`）。
fn status_location(
    workspace_index: usize,
    tab_label: &str,
    pane_number: usize,
    pane_count: usize,
) -> String {
    format!(
        "{} › {tab_label} › pane {pane_number}/{pane_count}",
        workspace_label(workspace_index)
    )
}

/// ステータスラインに出す Agent の数。1 つ以上いる状態だけを working・blocked・done の順に
/// 並べる（idle は目を向ける必要が無いので出さない）。
fn status_agents(counts: AgentCounts) -> Vec<(AgentState, usize)> {
    [
        (AgentState::Working, counts.working),
        (AgentState::Blocked, counts.blocked),
        (AgentState::Done, counts.done),
    ]
    .into_iter()
    .filter(|&(_, count)| count > 0)
    .collect()
}

/// サイドバーの Agent の 1 行。
#[derive(Debug, Clone)]
struct AgentRow {
    workspace_index: usize,
    tab_index: usize,
    pane_id: PaneId,
    kind: AgentKind,
    state: AgentState,
    location: String,
    /// ユーザーが今見ているペインか。
    active: bool,
}

/// サイドバーの workspace ごとのまとまり。
#[derive(Debug, Clone)]
struct WorkspaceGroup {
    index: usize,
    active: bool,
    agents: Vec<AgentRow>,
}

/// 状態ごとの Agent の数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AgentCounts {
    working: usize,
    blocked: usize,
    done: usize,
    idle: usize,
}

impl AgentCounts {
    fn of(groups: &[WorkspaceGroup]) -> Self {
        let mut counts = Self::default();
        for agent in groups.iter().flat_map(|group| &group.agents) {
            *match agent.state {
                AgentState::Working => &mut counts.working,
                AgentState::Blocked => &mut counts.blocked,
                AgentState::Done => &mut counts.done,
                AgentState::Idle => &mut counts.idle,
            } += 1;
        }
        counts
    }

    fn total(self) -> usize {
        self.working + self.blocked + self.done + self.idle
    }
}

fn window_title(title: Option<&str>) -> &str {
    title.unwrap_or(DEFAULT_TITLE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentTracker;
    use gpui::{TestAppContext, VisualTestContext};

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
        open_workspace_with_config(cx, first, Loaded::default())
    }

    fn open_workspace_with_config(
        cx: &mut TestAppContext,
        first: SpawnOptions,
        config: Loaded,
    ) -> (Entity<WorkspaceView>, &mut VisualTestContext) {
        let (terminal, events) = Terminal::spawn(first, INITIAL_SIZE).unwrap();
        cx.add_window_view(|window, cx| {
            WorkspaceView::new(terminal, events, sleep_options(), config, window, cx)
        })
    }

    /// `init.lua` の内容から設定を読み込んでワークスペースを開く。
    fn open_workspace_with_init_lua<'a>(
        cx: &'a mut TestAppContext,
        init_lua: &str,
    ) -> (Entity<WorkspaceView>, &'a mut VisualTestContext) {
        let config = crate::config::evaluate(init_lua, crate::config::FILE_NAME);
        open_workspace_with_config(cx, sleep_options(), config)
    }

    #[gpui::test]
    fn configured_prefix_drives_commands(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_init_lua(cx, r#"kugutsu.opt.prefix = "ctrl-a""#);
        cx.simulate_keystrokes("ctrl-b c");
        assert_eq!(active_tab_count(&workspace, cx), 1);
        cx.simulate_keystrokes("ctrl-a c");
        assert_eq!(active_tab_count(&workspace, cx), 2);
    }

    #[gpui::test]
    fn configured_prefix_is_shown(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_init_lua(cx, r#"kugutsu.opt.prefix = "ctrl-a""#);
        assert_eq!(
            workspace.read_with(cx, |workspace, _| workspace.prefix_label()),
            "ctrl-a"
        );
        cx.simulate_keystrokes("ctrl-a");
        cx.run_until_parked();
        assert!(cx.debug_bounds("which-key").is_some());
    }

    #[gpui::test]
    fn ui_font_follows_the_config(cx: &mut TestAppContext) {
        let (workspace, cx) =
            open_workspace_with_init_lua(cx, r#"kugutsu.opt.ui_font_family = "Some Sans""#);
        let expected = cx.update(|window, _| {
            crate::font::resolve_ui_family(
                Some("Some Sans"),
                &window.text_system().all_font_names(),
            )
        });
        assert_eq!(
            workspace.read_with(cx, |workspace, _| workspace.ui_font.clone()),
            expected.map(SharedString::from)
        );
    }

    #[gpui::test]
    fn config_errors_are_shown_in_status_line(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_init_lua(cx, "kugutsu.opt.foo = 1");
        cx.run_until_parked();
        let error = bounds(cx, "config-error");
        assert!(bounds(cx, "status-line").contains(&error.center()));
        let message = workspace.read_with(cx, |workspace, _| workspace.config_error_message());
        assert_eq!(message.as_deref(), Some("init.lua:1: unknown option 'foo'"));
    }

    #[gpui::test]
    fn unknown_theme_is_reported_and_falls_back_to_default(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_init_lua(cx, "kugutsu.opt.theme = 'nope'");
        cx.run_until_parked();
        assert!(cx.debug_bounds("config-error").is_some());
        workspace.read_with(cx, |workspace, _| {
            assert_eq!(
                workspace.config_error_message().as_deref(),
                Some("kugutsu.opt.theme: unknown theme 'nope'")
            );
            assert_eq!(workspace.config.theme, crate::theme::Theme::default());
        });
    }

    #[gpui::test]
    fn no_config_error_without_errors(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.run_until_parked();
        assert!(cx.debug_bounds("config-error").is_none());
        assert_eq!(
            workspace.read_with(cx, |workspace, _| workspace.config_error_message()),
            None
        );
    }

    #[gpui::test]
    fn missing_plugins_are_shown_in_status_line(cx: &mut TestAppContext) {
        let config = Loaded {
            missing_plugins: vec!["a.kugutsu".into(), "b.kugutsu".into()],
            errors: vec!["boom".into()],
            ..Loaded::default()
        };
        let (workspace, cx) = open_workspace_with_config(cx, sleep_options(), config);
        cx.run_until_parked();
        let missing = bounds(cx, "plugins-missing");
        assert!(bounds(cx, "status-line").contains(&missing.center()));
        // 設定のエラーと並べて出す
        assert!(missing.right() <= bounds(cx, "config-error").left());
        assert_eq!(
            workspace.read_with(cx, |workspace, _| workspace.missing_plugins_message()),
            Some("plugins: 2 missing — run kugutsu install".to_string())
        );
    }

    #[gpui::test]
    fn no_missing_plugins_message_when_all_are_installed(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.run_until_parked();
        assert!(cx.debug_bounds("plugins-missing").is_none());
        assert_eq!(
            workspace.read_with(cx, |workspace, _| workspace.missing_plugins_message()),
            None
        );
    }

    #[test]
    fn config_error_message_counts_extra_errors() {
        let errors = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(config_error_message(&errors).as_deref(), Some("a (+2)"));
        assert_eq!(config_error_message(&errors[..1]).as_deref(), Some("a"));
        assert_eq!(config_error_message(&[]), None);
    }

    fn light() -> crate::theme::Theme {
        crate::theme::builtin("kugutsu-light").unwrap().clone()
    }

    #[test]
    fn state_colors_come_from_the_theme() {
        let ui = light().ui;
        assert_eq!(state_color(&ui, AgentState::Working), ui.agent_working);
        assert_eq!(state_color(&ui, AgentState::Blocked), ui.agent_blocked);
        assert_eq!(state_color(&ui, AgentState::Done), ui.agent_done);
        assert_eq!(state_color(&ui, AgentState::Idle), ui.inactive_foreground);
    }

    #[test]
    fn pane_border_uses_the_theme() {
        let theme = light();
        assert_eq!(
            pane_border_color(&theme, true),
            rgb(theme.ui.active_pane_border)
        );
        assert_eq!(
            pane_border_color(&theme, false),
            rgb(theme.ui.strong_border)
        );
    }

    #[test]
    fn only_inactive_split_panes_are_dimmed() {
        let config = Config {
            inactive_pane_opacity: 0.6,
            ..Config::default()
        };
        assert_eq!(pane_opacity(&config, PaneState::Inactive), 0.6);
        assert_eq!(pane_opacity(&config, PaneState::Active), 1.0);
        assert_eq!(pane_opacity(&config, PaneState::Only), 1.0);
    }

    #[gpui::test]
    fn split_panes_are_separated_by_a_gap(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace(cx);
        cx.simulate_keystrokes("ctrl-b v");
        cx.run_until_parked();
        let left = bounds(cx, "pane-0");
        let right = bounds(cx, "pane-1");
        assert_eq!(right.left() - left.right(), px(PANE_GAP));
        assert_eq!(left.top(), right.top());
        // ペイン領域の縁からも離す
        assert_eq!(left.left() - bounds(cx, "panes").left(), px(PANE_GAP));
    }

    #[gpui::test]
    fn panes_use_the_configured_theme(cx: &mut TestAppContext) {
        let theme = light();
        let config = Loaded {
            config: Config {
                theme: theme.clone(),
                ..Config::default()
            },
            ..Loaded::default()
        };
        let (workspace, cx) = open_workspace_with_config(cx, sleep_options(), config);
        // 後から開いたタブのペインもテーマを使う
        cx.simulate_keystrokes("ctrl-b c");
        workspace.read_with(cx, |workspace, cx| {
            let view = workspace
                .active_tab()
                .and_then(Tab::active_view)
                .unwrap()
                .read(cx);
            assert_eq!(view.colors(), &theme.terminal);
        });
    }

    #[gpui::test]
    fn panes_use_theme_overrides_from_init_lua(cx: &mut TestAppContext) {
        let config = crate::config::evaluate(
            r##"kugutsu.opt.theme = "kugutsu-light"
            kugutsu.opt.theme_overrides = {
                terminal = { background = "#010203" },
                ui = { active_pane_border = "#040506" },
            }"##,
            "init.lua",
        );
        assert_eq!(config.errors, Vec::<String>::new());
        let (workspace, cx) = open_workspace_with_config(cx, sleep_options(), config);
        cx.simulate_keystrokes("ctrl-b c");
        workspace.read_with(cx, |workspace, cx| {
            let view = workspace
                .active_tab()
                .and_then(Tab::active_view)
                .unwrap()
                .read(cx);
            assert_eq!(view.colors().background, 0x010203);
            assert_eq!(view.colors().foreground, light().terminal.foreground);
            assert_eq!(
                pane_border_color(&workspace.config.theme, true),
                rgb(0x040506)
            );
        });
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
        assert_eq!(
            workspace_item.size.width,
            px(Config::default().ui.item_width)
        );
        // 上下の段で項目の左端も揃う
        assert_eq!(workspace_item.left(), tab_item.left());
    }

    #[gpui::test]
    fn bar_item_width_is_configurable(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace_with_init_lua(cx, "kugutsu.ui.item_width = 200");
        cx.run_until_parked();
        assert_eq!(bounds(cx, "workspace-0").size.width, px(200.0));
        assert_eq!(bounds(cx, "tab-0").size.width, px(200.0));
    }

    #[gpui::test]
    fn plus_buttons_open_a_workspace_and_a_tab(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.run_until_parked();
        click(cx, "tab-new");
        assert_eq!(active_tab_count(&workspace, cx), 2);
        click(cx, "workspace-new");
        assert_eq!(workspace_count(&workspace, cx), 2);
        assert_eq!(active_position(&workspace, cx), (1, 0));
    }

    #[gpui::test]
    fn bar_items_show_agent_state_dots(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_agents(cx);
        // workspace 1 は blocked と working、workspace 2 は done
        assert!(
            bounds(cx, "workspace-0").contains(&bounds(cx, "workspace-0-dot-blocked").center())
        );
        assert!(cx.debug_bounds("workspace-0-dot-working").is_some());
        assert!(cx.debug_bounds("workspace-1-dot-done").is_some());
        assert!(cx.debug_bounds("tab-0-dot-blocked").is_some());
        assert_eq!(
            workspace.read_with(cx, |w, cx| {
                let groups = w.agent_groups(cx);
                (
                    agent_dots(groups[0].agents.iter()),
                    agent_dots(groups[2].agents.iter()),
                )
            }),
            (vec![AgentState::Blocked, AgentState::Working], vec![])
        );
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
        assert_eq!(
            bounds(cx, "tab-0").left(),
            start + px(bar_item_step(&Config::default()))
        );

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
        assert_eq!(
            bounds(cx, "workspace-0").left(),
            start + px(bar_item_step(&Config::default()))
        );

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
    fn agent_location_includes_tab_and_pane() {
        assert_eq!(agent_location(1, Some("kugutsu"), 2), "2: kugutsu · pane 2");
        assert_eq!(agent_location(0, None, 1), "1: shell · pane 1");
    }

    /// `workspace_index` 番目の workspace・`tab_index` 番目のタブの `pane_nth` 番目のペインに
    /// Agent がいることにする（ポーリングを進めない限り上書きされない）。
    fn set_agent(
        workspace: &Entity<WorkspaceView>,
        cx: &mut VisualTestContext,
        (workspace_index, tab_index, pane_nth): (usize, usize, usize),
        kind: AgentKind,
        state: AgentState,
    ) {
        let view = workspace.read_with(cx, |w, _| {
            let tab = w
                .workspaces
                .iter()
                .nth(workspace_index)
                .and_then(|ws| ws.tabs.iter().nth(tab_index))
                .unwrap();
            tab.panes.iter().nth(pane_nth).unwrap().1.view.clone()
        });
        view.update(cx, |view, _| {
            view.set_agent(Some(AgentTracker { kind, state }))
        });
        workspace.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }

    /// Agent が 3 つの workspace にまたがる状態。workspace 1 のタブ 1 は 2 ペイン。
    fn open_workspace_with_agents(
        cx: &mut TestAppContext,
    ) -> (Entity<WorkspaceView>, &mut VisualTestContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.simulate_keystrokes("ctrl-b v");
        cx.simulate_keystrokes("ctrl-b shift-c");
        cx.simulate_keystrokes("ctrl-b shift-c");
        cx.simulate_keystrokes("ctrl-b shift-n");
        set_agent(
            &workspace,
            cx,
            (0, 0, 0),
            AgentKind::Codex,
            AgentState::Blocked,
        );
        let claude = AgentKind::ClaudeCode;
        set_agent(&workspace, cx, (0, 0, 1), claude, AgentState::Working);
        set_agent(&workspace, cx, (1, 0, 0), claude, AgentState::Done);
        (workspace, cx)
    }

    #[gpui::test]
    fn agents_are_grouped_by_workspace(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_agents(cx);
        let groups = workspace.read_with(cx, |w, cx| w.agent_groups(cx));
        let summary: Vec<_> = groups
            .iter()
            .map(|group| {
                let agents: Vec<_> = group
                    .agents
                    .iter()
                    .map(|agent| (agent.kind, agent.state, agent.location.clone()))
                    .collect();
                (group.index, group.active, agents)
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    0,
                    true,
                    vec![
                        (
                            AgentKind::Codex,
                            AgentState::Blocked,
                            "1: shell · pane 1".to_string()
                        ),
                        (
                            AgentKind::ClaudeCode,
                            AgentState::Working,
                            "1: shell · pane 2".to_string()
                        ),
                    ]
                ),
                (
                    1,
                    false,
                    vec![(
                        AgentKind::ClaudeCode,
                        AgentState::Done,
                        "1: shell · pane 1".to_string()
                    )]
                ),
                (2, false, vec![]),
            ]
        );
    }

    #[gpui::test]
    fn sidebar_lists_agents_under_their_workspace(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace_with_agents(cx);
        let top = |cx: &mut VisualTestContext, selector| bounds(cx, selector).top();
        let order = [
            "agent-group-0",
            "agent-0",
            "agent-1",
            "agent-group-1",
            "agent-2",
            "agent-group-2",
        ];
        let tops: Vec<_> = order.iter().map(|selector| top(cx, selector)).collect();
        assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
        // Agent のいない workspace は見出しだけ
        assert!(cx.debug_bounds("agent-group-2-empty").is_some());
        // Agent のいるアクティブな workspace には current と出す
        assert!(cx.debug_bounds("agent-group-0-current").is_some());
        // 見出しから Agent へ糸のような線でつなぐ
        let strings = bounds(cx, "agent-group-0-strings");
        assert!(strings.contains(&bounds(cx, "agent-1").center()));
    }

    #[gpui::test]
    fn sidebar_header_counts_blocked_agents(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_agents(cx);
        let (blocked, total) =
            workspace.read_with(cx, |w, cx| w.sidebar_counts(&w.agent_groups(cx)));
        assert_eq!(blocked.as_deref(), Some("1 blocked"));
        assert_eq!(total, "3 total");
        assert!(cx.debug_bounds("agents-blocked-count").is_some());
    }

    #[gpui::test]
    fn sidebar_header_hides_blocked_count_without_blocked_agents(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);
        cx.run_until_parked();
        assert!(cx.debug_bounds("agents-blocked-count").is_none());
        assert!(cx.debug_bounds("agent-group-0-empty").is_some());
        let (blocked, total) =
            workspace.read_with(cx, |w, cx| w.sidebar_counts(&w.agent_groups(cx)));
        assert_eq!(blocked, None);
        assert_eq!(total, "0 total");
    }

    #[test]
    fn status_location_names_workspace_tab_and_pane() {
        assert_eq!(
            status_location(0, "1: kugutsu", 1, 3),
            "Workspace 1 › 1: kugutsu › pane 1/3"
        );
    }

    #[test]
    fn status_agents_skip_empty_states_and_idle() {
        let counts = AgentCounts {
            working: 2,
            blocked: 0,
            done: 1,
            idle: 4,
        };
        assert_eq!(
            status_agents(counts),
            vec![(AgentState::Working, 2), (AgentState::Done, 1)]
        );
    }

    #[gpui::test]
    fn status_line_shows_prefix_location_and_agents(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_agents(cx);
        assert_eq!(
            workspace.read_with(cx, |w, cx| w.status_location(cx)),
            Some("Workspace 1 › 1: shell › pane 2/2".to_string())
        );
        let line = bounds(cx, "status-line");
        let order = [
            "status-prefix",
            "status-location",
            "status-agents-working",
            "status-agents-blocked",
            "status-agents-done",
        ];
        let parts: Vec<_> = order.iter().map(|selector| bounds(cx, selector)).collect();
        assert!(parts.iter().all(|part| line.contains(&part.center())));
        assert!(
            parts
                .windows(2)
                .all(|pair| pair[0].right() <= pair[1].left()),
            "{parts:?}"
        );
    }

    #[gpui::test]
    fn agents_are_counted_by_state(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_agents(cx);
        let counts = workspace.read_with(cx, |w, cx| AgentCounts::of(&w.agent_groups(cx)));
        assert_eq!(
            counts,
            AgentCounts {
                working: 1,
                blocked: 1,
                done: 1,
                idle: 0,
            }
        );
        assert_eq!(counts.total(), 3);
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
        // ステータスラインから少し浮かせ、左右の端にも付けない
        assert_eq!(
            bounds(cx, "status-line").top() - which_key.bottom(),
            px(OVERLAY_MARGIN)
        );
        assert_eq!(which_key.left(), px(OVERLAY_MARGIN));
        // 縦に伸ばさないよう、ウィンドウの横幅いっぱいに広げて並べる
        let line = bounds(cx, "status-line");
        assert_eq!(line.right() - which_key.right(), px(OVERLAY_MARGIN));
        // キーはキーキャップとして説明と分けて描く
        let keycap = bounds(cx, "which-key-cap:c");
        assert!(which_key.contains(&keycap.center()));
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

    fn active_pane_count(workspace: &Entity<WorkspaceView>, cx: &mut VisualTestContext) -> usize {
        workspace.read_with(cx, |w, _| {
            w.active_tabs()
                .unwrap()
                .active()
                .unwrap()
                .panes
                .iter()
                .count()
        })
    }

    #[gpui::test]
    fn mapped_key_after_prefix_runs_its_action(cx: &mut TestAppContext) {
        let (workspace, cx) =
            open_workspace_with_init_lua(cx, r#"kugutsu.keymap.set("<prefix>t", "new_tab")"#);
        cx.simulate_keystrokes("ctrl-b t");
        assert_eq!(active_tab_count(&workspace, cx), 2);
    }

    #[gpui::test]
    fn mappings_follow_the_configured_prefix(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_init_lua(
            cx,
            r#"kugutsu.keymap.set("<prefix>t", "new_tab")
            kugutsu.opt.prefix = "ctrl-a""#,
        );
        cx.simulate_keystrokes("ctrl-b t");
        assert_eq!(active_tab_count(&workspace, cx), 1);
        cx.simulate_keystrokes("ctrl-a t");
        assert_eq!(active_tab_count(&workspace, cx), 2);
    }

    #[gpui::test]
    fn direct_mapping_runs_without_the_prefix(cx: &mut TestAppContext) {
        let (workspace, cx) =
            open_workspace_with_init_lua(cx, r#"kugutsu.keymap.set("ctrl-shift-t", "new_tab")"#);
        cx.simulate_keystrokes("ctrl-shift-t");
        assert_eq!(active_tab_count(&workspace, cx), 2);
    }

    #[gpui::test]
    fn deleted_mapping_does_nothing(cx: &mut TestAppContext) {
        let (workspace, cx) =
            open_workspace_with_init_lua(cx, r#"kugutsu.keymap.del("<prefix>c")"#);
        cx.simulate_keystrokes("ctrl-b c");
        assert_eq!(active_tab_count(&workspace, cx), 1);
    }

    #[gpui::test]
    fn lua_mapping_runs_the_actions_it_calls(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_init_lua(
            cx,
            r#"kugutsu.keymap.set("<prefix>w", function()
                kugutsu.action.new_tab()
                kugutsu.action.split_horizontal()
            end)"#,
        );
        cx.simulate_keystrokes("ctrl-b w");
        assert_eq!(active_tab_count(&workspace, cx), 2);
        assert_eq!(active_pane_count(&workspace, cx), 2);
    }

    #[gpui::test]
    fn lua_mapping_errors_are_shown_in_status_line(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace_with_init_lua(
            cx,
            r#"kugutsu.keymap.set("<prefix>w", function() error("boom") end)"#,
        );
        cx.run_until_parked();
        assert!(cx.debug_bounds("config-error").is_none());
        cx.simulate_keystrokes("ctrl-b w");
        cx.run_until_parked();
        assert!(cx.debug_bounds("config-error").is_some());
        assert_eq!(
            workspace.read_with(cx, |workspace, _| workspace.config_error_message()),
            Some("init.lua:1: boom".to_string())
        );
    }

    #[gpui::test]
    fn which_key_shows_mapping_descriptions(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace_with_init_lua(
            cx,
            r#"kugutsu.keymap.set("<prefix>t", "new_tab", { desc = "Open a tab" })
            kugutsu.keymap.set("ctrl-shift-t", "new_tab", { desc = "Tab right away" })"#,
        );
        cx.simulate_keystrokes("ctrl-b");
        cx.run_until_parked();
        assert!(cx.debug_bounds("which-key:t:Open a tab").is_some());
        assert!(cx.debug_bounds("which-key:1-9:Select tab N").is_some());
        // prefix を使わない割り当ては which-key に出さない
        assert!(
            cx.debug_bounds("which-key:ctrl-shift-t:Tab right away")
                .is_none()
        );
    }

    #[gpui::test]
    fn keymap_help_lists_prefixed_and_direct_mappings(cx: &mut TestAppContext) {
        let (_workspace, cx) = open_workspace_with_init_lua(
            cx,
            r#"kugutsu.keymap.set("<prefix>t", "new_tab", { desc = "Open a tab" })
            kugutsu.keymap.set("ctrl-shift-t", "new_tab", { desc = "Tab right away" })"#,
        );
        cx.simulate_keystrokes("ctrl-b ?");
        cx.run_until_parked();
        assert!(cx.debug_bounds("keymap-help:ctrl-b t:Open a tab").is_some());
        assert!(
            cx.debug_bounds("keymap-help:ctrl-shift-t:Tab right away")
                .is_some()
        );
        // 分類ごとの見出しの下に並ぶ
        let tabs = bounds(cx, "keymap-help-group-tabs");
        assert!(tabs.contains(&bounds(cx, "keymap-help:ctrl-b t:Open a tab").center()));
        assert!(tabs.contains(&bounds(cx, "keymap-help:ctrl-shift-t:Tab right away").center()));
        let panes = bounds(cx, "keymap-help-group-panes");
        assert!(panes.contains(&bounds(cx, "keymap-help:ctrl-b x:Close pane").center()));
        assert!(cx.debug_bounds("keymap-help-group-workspaces").is_some());
        assert!(cx.debug_bounds("keymap-help-group-general").is_some());
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
        for theme in [crate::theme::Theme::default(), light()] {
            let colors = [
                AgentState::Working,
                AgentState::Blocked,
                AgentState::Done,
                AgentState::Idle,
            ]
            .map(|state| state_color(&theme.ui, state));
            for (i, a) in colors.iter().enumerate() {
                for b in &colors[i + 1..] {
                    assert_ne!(a, b, "{}", theme.name);
                }
            }
        }
    }

    #[test]
    fn window_title_falls_back_to_app_name() {
        assert_eq!(window_title(Some("vim")), "vim");
        assert_eq!(window_title(None), "kugutsu");
    }
}
