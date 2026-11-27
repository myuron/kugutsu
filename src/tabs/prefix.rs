//! tmux 風の prefix キーによるタブ・ペイン操作のキーマップ。
//!
//! prefix を押すと次の 1 キーを操作として解釈する。prefix を 2 回押すと prefix 自体を
//! ターミナルへ送る。割り当ての無いキーは捨てる。prefix を使わず、押すだけで操作になる
//! キー（[`Trigger::Direct`]）も割り当てられる。

use std::fmt;

use gpui::Keystroke;

use crate::config::LuaCallback;
use crate::panes::{Axis, Direction};

/// prefix キー（gpui のキーストローク表記）。
pub const DEFAULT_PREFIX: &str = "ctrl-b";

/// キーの表記で「prefix の後に押す」ことを表す接頭辞。
pub const PREFIX_PLACEHOLDER: &str = "<prefix>";

/// タブ・ペインに対する操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    NewTab,
    NextTab,
    PrevTab,
    /// 0 始まりの位置のタブへ切り替える。
    SelectTab(usize),
    /// アクティブなペインを分割する。
    Split(Axis),
    /// 隣のペインへフォーカスを移す。
    Focus(Direction),
    /// アクティブなペインを閉じる（最後のペインならタブごと閉じる）。
    ClosePane,
    /// Agent 一覧のサイドバーの表示を切り替える。
    ToggleSidebar,
    /// 新しい workspace（タブのグループ）を開く。
    NewWorkspace,
    NextWorkspace,
    PrevWorkspace,
    /// キーマップの一覧を表示する。
    ShowKeymap,
    /// `init.lua` を読み込み直す。
    ReloadConfig,
}

impl Command {
    /// 引数の無い操作と、設定（`init.lua`）で使う名前。
    pub const NAMED: &[(&str, Command)] = &[
        ("new_tab", Command::NewTab),
        ("next_tab", Command::NextTab),
        ("prev_tab", Command::PrevTab),
        ("split_horizontal", Command::Split(Axis::Horizontal)),
        ("split_vertical", Command::Split(Axis::Vertical)),
        ("focus_left", Command::Focus(Direction::Left)),
        ("focus_down", Command::Focus(Direction::Down)),
        ("focus_up", Command::Focus(Direction::Up)),
        ("focus_right", Command::Focus(Direction::Right)),
        ("close_pane", Command::ClosePane),
        ("toggle_sidebar", Command::ToggleSidebar),
        ("new_workspace", Command::NewWorkspace),
        ("next_workspace", Command::NextWorkspace),
        ("prev_workspace", Command::PrevWorkspace),
        ("show_keymap", Command::ShowKeymap),
        ("reload_config", Command::ReloadConfig),
    ];

    /// 設定で使う名前。引数のある操作（`SelectTab`）は名前を持たない。
    #[cfg(test)]
    pub fn name(self) -> Option<&'static str> {
        Self::NAMED
            .iter()
            .find(|&&(_, command)| command == self)
            .map(|&(name, _)| name)
    }

    /// 名前に対応する操作。
    pub fn from_name(name: &str) -> Option<Self> {
        Self::NAMED
            .iter()
            .find(|&&(candidate, _)| candidate == name)
            .map(|&(_, command)| command)
    }

    /// キーマップ一覧に表示する既定の説明。
    pub fn description(self) -> &'static str {
        match self {
            Command::NewTab => "New tab",
            Command::NextTab => "Next tab",
            Command::PrevTab => "Previous tab",
            Command::SelectTab(_) => "Select tab N",
            Command::Split(Axis::Horizontal) => "Split pane left / right",
            Command::Split(Axis::Vertical) => "Split pane top / bottom",
            Command::Focus(Direction::Left) => "Focus left pane",
            Command::Focus(Direction::Down) => "Focus pane below",
            Command::Focus(Direction::Up) => "Focus pane above",
            Command::Focus(Direction::Right) => "Focus right pane",
            Command::ClosePane => "Close pane",
            Command::ToggleSidebar => "Toggle agent sidebar",
            Command::NewWorkspace => "New workspace",
            Command::NextWorkspace => "Next workspace",
            Command::PrevWorkspace => "Previous workspace",
            Command::ShowKeymap => "Show key bindings",
            Command::ReloadConfig => "Reload init.lua",
        }
    }
}

/// キーマップ一覧で動作をまとめる分類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Tabs,
    Panes,
    Workspaces,
    /// サイドバー・キーマップ一覧・Lua 関数など。
    General,
}

impl Group {
    /// キーマップ一覧に並べる順。
    pub const ALL: [Group; 4] = [Group::Tabs, Group::Panes, Group::Workspaces, Group::General];

    pub fn label(self) -> &'static str {
        match self {
            Group::Tabs => "Tabs",
            Group::Panes => "Panes",
            Group::Workspaces => "Workspaces",
            Group::General => "General",
        }
    }
}

impl Command {
    /// キーマップ一覧での分類。
    pub fn group(self) -> Group {
        match self {
            Command::NewTab | Command::NextTab | Command::PrevTab | Command::SelectTab(_) => {
                Group::Tabs
            }
            Command::Split(_) | Command::Focus(_) | Command::ClosePane => Group::Panes,
            Command::NewWorkspace | Command::NextWorkspace | Command::PrevWorkspace => {
                Group::Workspaces
            }
            Command::ToggleSidebar | Command::ShowKeymap | Command::ReloadConfig => Group::General,
        }
    }
}

/// キーに割り当てる動作。
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Command(Command),
    /// `init.lua` で割り当てた Lua 関数。
    Lua(LuaCallback),
}

impl Action {
    /// 説明が指定されなかったときの説明。
    fn default_description(&self) -> &'static str {
        match self {
            Action::Command(command) => command.description(),
            Action::Lua(_) => "Lua function",
        }
    }

    /// キーマップ一覧での分類。Lua 関数は何をするか分からないので General にする。
    fn group(&self) -> Group {
        match self {
            Action::Command(command) => command.group(),
            Action::Lua(_) => Group::General,
        }
    }
}

/// 操作を起こすキー。
#[derive(Debug, Clone, PartialEq)]
pub enum Trigger {
    /// prefix の後に押すキー（`<prefix>t`）。
    Prefixed(Keystroke),
    /// 押すだけで操作になるキー（`ctrl-shift-t`）。
    Direct(Keystroke),
}

impl Trigger {
    /// `<prefix>t` や `ctrl-shift-t` のような表記を解析する。
    pub fn parse(text: &str) -> Result<Self, String> {
        let invalid = || format!("invalid key {text:?}");
        let (prefixed, key) = match text.strip_prefix(PREFIX_PLACEHOLDER) {
            Some(key) => (true, key),
            None => (false, text),
        };
        let keystroke = Keystroke::parse(key)
            .ok()
            .filter(|keystroke| !keystroke.key.is_empty())
            .ok_or_else(invalid)?;
        Ok(if prefixed {
            Trigger::Prefixed(keystroke)
        } else {
            Trigger::Direct(keystroke)
        })
    }

    /// 同じキー・修飾キーを指すか（IME の入力結果などは比べない）。
    fn same(&self, other: &Trigger) -> bool {
        match (self, other) {
            (Trigger::Prefixed(a), Trigger::Prefixed(b))
            | (Trigger::Direct(a), Trigger::Direct(b)) => same_key(a, b),
            _ => false,
        }
    }
}

impl fmt::Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trigger::Prefixed(keystroke) => {
                write!(f, "{PREFIX_PLACEHOLDER}{}", keystroke.unparse())
            }
            Trigger::Direct(keystroke) => write!(f, "{}", keystroke.unparse()),
        }
    }
}

/// キーと動作の割り当て 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub trigger: Trigger,
    pub action: Action,
    /// which-key やキーマップ一覧に出す説明。
    pub desc: String,
}

/// prefix の後のキー（gpui のキーストローク表記）と操作の既定の対応。
/// 数字キーによるタブ選択は `Keymap::default` で `p` の後に足す。
const DEFAULT_KEYS: &[(&str, Command)] = &[
    ("c", Command::NewTab),
    ("n", Command::NextTab),
    ("p", Command::PrevTab),
    ("v", Command::Split(Axis::Horizontal)),
    ("s", Command::Split(Axis::Vertical)),
    ("h", Command::Focus(Direction::Left)),
    ("j", Command::Focus(Direction::Down)),
    ("k", Command::Focus(Direction::Up)),
    ("l", Command::Focus(Direction::Right)),
    ("x", Command::ClosePane),
    ("b", Command::ToggleSidebar),
    ("shift-c", Command::NewWorkspace),
    ("shift-n", Command::NextWorkspace),
    ("shift-p", Command::PrevWorkspace),
    ("?", Command::ShowKeymap),
    ("r", Command::ReloadConfig),
];

/// キーの割り当ての一覧（並びは表示順）。
#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

impl Default for Keymap {
    fn default() -> Self {
        let mut keymap = Self {
            bindings: Vec::new(),
        };
        for &(key, command) in DEFAULT_KEYS {
            keymap.push_default(key, command);
            if command == Command::PrevTab {
                for number in 1..=9 {
                    keymap.push_default(&number.to_string(), Command::SelectTab(number - 1));
                }
            }
        }
        keymap
    }
}

impl Keymap {
    fn push_default(&mut self, key: &str, command: Command) {
        let keystroke = Keystroke::parse(key).expect("default key is a valid keystroke");
        self.set(Trigger::Prefixed(keystroke), Action::Command(command), None);
    }

    #[cfg(test)]
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// `trigger` に `action` を割り当てる。割り当て済みなら同じ位置で置き換え、
    /// 無ければ末尾に足す。`desc` を省略するとアクションの既定の説明を使う。
    pub fn set(&mut self, trigger: Trigger, action: Action, desc: Option<String>) {
        let desc = desc.unwrap_or_else(|| action.default_description().to_string());
        let binding = Binding {
            trigger,
            action,
            desc,
        };
        match self.position(&binding.trigger) {
            Some(index) => self.bindings[index] = binding,
            None => self.bindings.push(binding),
        }
    }

    /// `trigger` の割り当てを外す。割り当てが無ければ `false`。
    pub fn del(&mut self, trigger: &Trigger) -> bool {
        self.position(trigger)
            .map(|index| self.bindings.remove(index))
            .is_some()
    }

    /// `trigger` に割り当てた動作。
    pub fn lookup(&self, trigger: &Trigger) -> Option<&Binding> {
        self.position(trigger).map(|index| &self.bindings[index])
    }

    fn position(&self, trigger: &Trigger) -> Option<usize> {
        self.bindings
            .iter()
            .position(|binding| binding.trigger.same(trigger))
    }

    /// which-key に出す（prefix の後のキー, 説明）。prefix 付きの割り当てだけを並べ、
    /// 隣り合う同じ説明の行はまとめる（連続した数字は `1-9`、それ以外は `t/shift-t`）。
    pub fn entries(&self) -> Vec<(String, String)> {
        self.prefixed_entries()
            .into_iter()
            .map(|(key, desc, _)| (key, desc))
            .collect()
    }

    /// [`Keymap::entries`] に、割り当てた動作の分類を添えたもの。
    fn prefixed_entries(&self) -> Vec<(String, String, Group)> {
        let mut runs: Vec<(Vec<String>, &str, Group)> = Vec::new();
        for binding in &self.bindings {
            let Trigger::Prefixed(keystroke) = &binding.trigger else {
                continue;
            };
            let key = keystroke.unparse();
            let group = binding.action.group();
            match runs.last_mut() {
                Some((keys, desc, last)) if *desc == binding.desc && *last == group => {
                    keys.push(key)
                }
                _ => runs.push((vec![key], &binding.desc, group)),
            }
        }
        runs.into_iter()
            .flat_map(|(keys, desc, group)| {
                join_keys(&keys)
                    .into_iter()
                    .map(move |key| (key, desc.to_string(), group))
            })
            .collect()
    }

    /// prefix を使わない割り当ての（キー, 説明）。
    #[cfg(test)]
    pub fn direct_entries(&self) -> Vec<(String, String)> {
        self.direct_entries_with_group()
            .into_iter()
            .map(|(key, desc, _)| (key, desc))
            .collect()
    }

    fn direct_entries_with_group(&self) -> Vec<(String, String, Group)> {
        self.bindings
            .iter()
            .filter_map(|binding| match &binding.trigger {
                Trigger::Direct(keystroke) => Some((
                    keystroke.unparse(),
                    binding.desc.clone(),
                    binding.action.group(),
                )),
                Trigger::Prefixed(_) => None,
            })
            .collect()
    }

    /// キーマップ一覧に出す、分類ごとの（キー, 説明）。prefix 付きのキーは `prefix` を前に付け、
    /// prefix を使わない割り当ての前に並べる。割り当ての無い分類は省く。
    pub fn help_groups(&self, prefix: &str) -> Vec<(Group, Vec<(String, String)>)> {
        let entries: Vec<_> = self
            .prefixed_entries()
            .into_iter()
            .map(|(key, desc, group)| (format!("{prefix} {key}"), desc, group))
            .chain(self.direct_entries_with_group())
            .collect();
        Group::ALL
            .into_iter()
            .map(|group| {
                let rows = entries
                    .iter()
                    .filter(|(_, _, candidate)| *candidate == group)
                    .map(|(key, desc, _)| (key.clone(), desc.clone()))
                    .collect();
                (group, rows)
            })
            .filter(|(_, rows): &(Group, Vec<_>)| !rows.is_empty())
            .collect()
    }
}

/// 同じ説明のキーをまとめた表示。連続した数字の並びは `1-9` にし、途切れたら分ける。
/// 数字だけでない組は `/` で繋いで 1 つにする。
fn join_keys(keys: &[String]) -> Vec<String> {
    let digits: Option<Vec<u32>> = keys
        .iter()
        .map(|key| match key.as_bytes() {
            [digit] if digit.is_ascii_digit() => Some(u32::from(digit - b'0')),
            _ => None,
        })
        .collect();
    let Some(digits) = digits.filter(|digits| digits.len() > 1) else {
        return vec![keys.join("/")];
    };
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for digit in digits {
        match runs.last_mut() {
            Some((_, last)) if *last + 1 == digit => *last = digit,
            _ => runs.push((digit, digit)),
        }
    }
    runs.into_iter()
        .map(|(first, last)| {
            if first == last {
                first.to_string()
            } else {
                format!("{first}-{last}")
            }
        })
        .collect()
}

/// キー入力をどう扱うか。
#[derive(Debug, Clone, PartialEq)]
pub enum KeyOutcome {
    /// ターミナルへそのまま送る。
    Forward,
    /// キーマップが消費した（ターミナルへは送らない）。
    Consumed,
    Action(Action),
}

#[derive(Debug)]
pub struct PrefixKeymap {
    prefix: Keystroke,
    keymap: Keymap,
    armed: bool,
}

impl Default for PrefixKeymap {
    fn default() -> Self {
        Self::new(
            Keystroke::parse(DEFAULT_PREFIX).expect("default prefix is a valid keystroke"),
            Keymap::default(),
        )
    }
}

impl PrefixKeymap {
    pub fn new(prefix: Keystroke, keymap: Keymap) -> Self {
        Self {
            prefix,
            keymap,
            armed: false,
        }
    }

    /// prefix キー。
    pub fn prefix(&self) -> &Keystroke {
        &self.prefix
    }

    /// キーの割り当て。
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    pub fn handle(&mut self, keystroke: &Keystroke) -> KeyOutcome {
        let is_prefix = same_key(keystroke, &self.prefix);
        if !std::mem::take(&mut self.armed) {
            if is_prefix {
                self.armed = true;
                return KeyOutcome::Consumed;
            }
            return self.action_for(Trigger::Direct(keystroke.clone()), KeyOutcome::Forward);
        }
        if is_prefix {
            return KeyOutcome::Forward;
        }
        self.action_for(Trigger::Prefixed(keystroke.clone()), KeyOutcome::Consumed)
    }

    /// `trigger` に割り当てた動作。割り当てが無ければ `otherwise`。
    fn action_for(&self, trigger: Trigger, otherwise: KeyOutcome) -> KeyOutcome {
        self.keymap.lookup(&trigger).map_or(otherwise, |binding| {
            KeyOutcome::Action(binding.action.clone())
        })
    }

    /// prefix が押され、次のキーを待っているか。
    pub fn is_armed(&self) -> bool {
        self.armed
    }
}

/// 2 つのキーストロークが同じキー・修飾キーか（IME の入力結果などは比べない）。
pub fn same_key(keystroke: &Keystroke, expected: &Keystroke) -> bool {
    keystroke.key == expected.key && keystroke.modifiers == expected.modifiers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(keymap: &mut PrefixKeymap, key: &str) -> KeyOutcome {
        keymap.handle(&Keystroke::parse(key).unwrap())
    }

    fn command(command: Command) -> KeyOutcome {
        KeyOutcome::Action(Action::Command(command))
    }

    fn trigger(text: &str) -> Trigger {
        Trigger::parse(text).unwrap()
    }

    fn with_keymap(keymap: Keymap) -> PrefixKeymap {
        PrefixKeymap::new(Keystroke::parse(DEFAULT_PREFIX).unwrap(), keymap)
    }

    #[test]
    fn forwards_ordinary_keys() {
        let mut keymap = PrefixKeymap::default();
        assert_eq!(press(&mut keymap, "a"), KeyOutcome::Forward);
        assert_eq!(press(&mut keymap, "ctrl-c"), KeyOutcome::Forward);
        assert!(!keymap.is_armed());
    }

    #[test]
    fn prefix_arms_and_is_consumed() {
        let mut keymap = PrefixKeymap::default();
        assert_eq!(press(&mut keymap, DEFAULT_PREFIX), KeyOutcome::Consumed);
        assert!(keymap.is_armed());
    }

    #[test]
    fn maps_keys_after_prefix_to_commands() {
        let cases = [
            ("c", Command::NewTab),
            ("n", Command::NextTab),
            ("p", Command::PrevTab),
            ("1", Command::SelectTab(0)),
            ("3", Command::SelectTab(2)),
            ("9", Command::SelectTab(8)),
            ("v", Command::Split(Axis::Horizontal)),
            ("s", Command::Split(Axis::Vertical)),
            ("h", Command::Focus(Direction::Left)),
            ("j", Command::Focus(Direction::Down)),
            ("k", Command::Focus(Direction::Up)),
            ("l", Command::Focus(Direction::Right)),
            ("x", Command::ClosePane),
            ("b", Command::ToggleSidebar),
            ("shift-c", Command::NewWorkspace),
            ("shift-n", Command::NextWorkspace),
            ("shift-p", Command::PrevWorkspace),
            ("?", Command::ShowKeymap),
            ("r", Command::ReloadConfig),
        ];
        for (key, expected) in cases {
            let mut keymap = PrefixKeymap::default();
            press(&mut keymap, DEFAULT_PREFIX);
            assert_eq!(press(&mut keymap, key), command(expected), "{key}");
            assert!(!keymap.is_armed(), "{key}");
        }
    }

    #[test]
    fn double_prefix_forwards_prefix() {
        let mut keymap = PrefixKeymap::default();
        press(&mut keymap, DEFAULT_PREFIX);
        assert_eq!(press(&mut keymap, DEFAULT_PREFIX), KeyOutcome::Forward);
        assert!(!keymap.is_armed());
    }

    #[test]
    fn unknown_key_after_prefix_is_discarded() {
        let mut keymap = PrefixKeymap::default();
        press(&mut keymap, DEFAULT_PREFIX);
        assert_eq!(press(&mut keymap, "z"), KeyOutcome::Consumed);
        assert!(!keymap.is_armed());
        assert_eq!(press(&mut keymap, "c"), KeyOutcome::Forward);
    }

    #[test]
    fn escape_cancels_prefix() {
        let mut keymap = PrefixKeymap::default();
        press(&mut keymap, DEFAULT_PREFIX);
        assert_eq!(press(&mut keymap, "escape"), KeyOutcome::Consumed);
        assert!(!keymap.is_armed());
    }

    #[test]
    fn uses_a_custom_prefix() {
        let mut keymap = PrefixKeymap::new(Keystroke::parse("ctrl-a").unwrap(), Keymap::default());
        assert_eq!(press(&mut keymap, DEFAULT_PREFIX), KeyOutcome::Forward);
        assert!(!keymap.is_armed());
        assert_eq!(press(&mut keymap, "ctrl-a"), KeyOutcome::Consumed);
        assert!(keymap.is_armed());
        assert_eq!(press(&mut keymap, "c"), command(Command::NewTab));
    }

    #[test]
    fn double_custom_prefix_forwards_it() {
        let mut keymap = PrefixKeymap::new(Keystroke::parse("ctrl-a").unwrap(), Keymap::default());
        press(&mut keymap, "ctrl-a");
        assert_eq!(press(&mut keymap, "ctrl-a"), KeyOutcome::Forward);
        assert!(!keymap.is_armed());
    }

    #[test]
    fn exposes_the_prefix_for_display() {
        let keymap = PrefixKeymap::new(Keystroke::parse("ctrl-a").unwrap(), Keymap::default());
        assert_eq!(keymap.prefix().unparse(), "ctrl-a");
        assert_eq!(PrefixKeymap::default().prefix().unparse(), DEFAULT_PREFIX);
    }

    #[test]
    fn modified_keys_are_not_commands() {
        for key in ["ctrl-c", "alt-n", "shift-x", "0"] {
            let mut keymap = PrefixKeymap::default();
            press(&mut keymap, DEFAULT_PREFIX);
            assert_eq!(press(&mut keymap, key), KeyOutcome::Consumed, "{key}");
        }
    }

    #[test]
    fn command_names_round_trip() {
        let named = [
            ("new_tab", Command::NewTab),
            ("next_tab", Command::NextTab),
            ("prev_tab", Command::PrevTab),
            ("split_horizontal", Command::Split(Axis::Horizontal)),
            ("split_vertical", Command::Split(Axis::Vertical)),
            ("focus_left", Command::Focus(Direction::Left)),
            ("focus_down", Command::Focus(Direction::Down)),
            ("focus_up", Command::Focus(Direction::Up)),
            ("focus_right", Command::Focus(Direction::Right)),
            ("close_pane", Command::ClosePane),
            ("toggle_sidebar", Command::ToggleSidebar),
            ("new_workspace", Command::NewWorkspace),
            ("next_workspace", Command::NextWorkspace),
            ("prev_workspace", Command::PrevWorkspace),
            ("show_keymap", Command::ShowKeymap),
            ("reload_config", Command::ReloadConfig),
        ];
        assert_eq!(Command::NAMED.len(), named.len());
        for (name, expected) in named {
            assert_eq!(Command::from_name(name), Some(expected), "{name}");
            assert_eq!(expected.name(), Some(name), "{name}");
        }
        assert_eq!(Command::SelectTab(0).name(), None);
        assert_eq!(Command::from_name("select_tab"), None);
        assert_eq!(Command::from_name("nope"), None);
    }

    #[test]
    fn parses_triggers() {
        assert_eq!(
            trigger("<prefix>t"),
            Trigger::Prefixed(Keystroke::parse("t").unwrap())
        );
        assert_eq!(
            trigger("<prefix>shift-c"),
            Trigger::Prefixed(Keystroke::parse("shift-c").unwrap())
        );
        assert_eq!(
            trigger("ctrl-shift-t"),
            Trigger::Direct(Keystroke::parse("ctrl-shift-t").unwrap())
        );
        for invalid in ["", "<prefix>", "ctrl-", "<prefix>ctrl-"] {
            assert!(Trigger::parse(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn triggers_are_shown_like_they_are_written() {
        assert_eq!(trigger("<prefix>t").to_string(), "<prefix>t");
        assert_eq!(trigger("ctrl-shift-t").to_string(), "ctrl-shift-t");
    }

    #[test]
    fn set_adds_a_new_binding_at_the_end() {
        let mut keymap = Keymap::default();
        let count = keymap.bindings().len();
        keymap.set(
            trigger("<prefix>t"),
            Action::Command(Command::NewTab),
            Some("Open a tab".into()),
        );
        assert_eq!(keymap.bindings().len(), count + 1);
        let last = keymap.bindings().last().unwrap();
        assert_eq!(last.trigger, trigger("<prefix>t"));
        assert_eq!(last.desc, "Open a tab");

        let mut prefix_keymap = with_keymap(keymap);
        press(&mut prefix_keymap, DEFAULT_PREFIX);
        assert_eq!(press(&mut prefix_keymap, "t"), command(Command::NewTab));
    }

    #[test]
    fn set_replaces_an_existing_binding_in_place() {
        let mut keymap = Keymap::default();
        let position = keymap
            .bindings()
            .iter()
            .position(|binding| binding.trigger == trigger("<prefix>c"))
            .unwrap();
        let count = keymap.bindings().len();
        keymap.set(
            trigger("<prefix>c"),
            Action::Command(Command::NewWorkspace),
            None,
        );
        assert_eq!(keymap.bindings().len(), count);
        let binding = &keymap.bindings()[position];
        assert_eq!(binding.action, Action::Command(Command::NewWorkspace));
        // desc を省略するとアクションの既定の説明
        assert_eq!(binding.desc, "New workspace");
    }

    #[test]
    fn del_removes_a_binding() {
        let mut keymap = Keymap::default();
        assert!(keymap.del(&trigger("<prefix>c")));
        assert!(!keymap.del(&trigger("<prefix>c")));
        assert!(!keymap.del(&trigger("<prefix>z")));
        let mut prefix_keymap = with_keymap(keymap);
        press(&mut prefix_keymap, DEFAULT_PREFIX);
        assert_eq!(press(&mut prefix_keymap, "c"), KeyOutcome::Consumed);
    }

    #[test]
    fn direct_bindings_fire_without_the_prefix() {
        let mut keymap = Keymap::default();
        keymap.set(
            trigger("ctrl-shift-t"),
            Action::Command(Command::NewTab),
            None,
        );
        let mut prefix_keymap = with_keymap(keymap);
        assert_eq!(
            press(&mut prefix_keymap, "ctrl-shift-t"),
            command(Command::NewTab)
        );
        assert!(!prefix_keymap.is_armed());
        // prefix の後では prefix 付きのマッピングだけを見る
        press(&mut prefix_keymap, DEFAULT_PREFIX);
        assert_eq!(
            press(&mut prefix_keymap, "ctrl-shift-t"),
            KeyOutcome::Consumed
        );
    }

    #[test]
    fn prefixed_bindings_do_not_fire_without_the_prefix() {
        let mut keymap = PrefixKeymap::default();
        assert_eq!(press(&mut keymap, "c"), KeyOutcome::Forward);
    }

    #[test]
    fn entries_list_prefixed_bindings_with_digits_merged() {
        let entries = Keymap::default().entries();
        let pair = |key: &str, desc: &str| (key.to_string(), desc.to_string());
        assert_eq!(
            entries[..4],
            [
                pair("c", "New tab"),
                pair("n", "Next tab"),
                pair("p", "Previous tab"),
                pair("1-9", "Select tab N"),
            ]
        );
        assert!(entries.contains(&pair("?", "Show key bindings")));
        assert!(entries.contains(&pair("r", "Reload init.lua")));
        assert!(entries.contains(&pair("shift-c", "New workspace")));
        assert_eq!(entries.len(), 17);
    }

    #[test]
    fn entries_show_descriptions_and_skip_direct_bindings() {
        let mut keymap = Keymap::default();
        keymap.set(
            trigger("<prefix>t"),
            Action::Command(Command::NewTab),
            Some("Open a tab".into()),
        );
        keymap.set(
            trigger("ctrl-shift-t"),
            Action::Command(Command::NewTab),
            None,
        );
        let entries = keymap.entries();
        assert_eq!(
            entries.last(),
            Some(&("t".to_string(), "Open a tab".to_string()))
        );
        assert!(!entries.iter().any(|(key, _)| key.contains("ctrl-shift-t")));
        assert_eq!(
            keymap.direct_entries(),
            vec![("ctrl-shift-t".to_string(), "New tab".to_string())]
        );
    }

    #[test]
    fn commands_belong_to_groups() {
        assert_eq!(Command::NewTab.group(), Group::Tabs);
        assert_eq!(Command::SelectTab(2).group(), Group::Tabs);
        assert_eq!(Command::Split(Axis::Vertical).group(), Group::Panes);
        assert_eq!(Command::Focus(Direction::Left).group(), Group::Panes);
        assert_eq!(Command::ClosePane.group(), Group::Panes);
        assert_eq!(Command::NewWorkspace.group(), Group::Workspaces);
        assert_eq!(Command::PrevWorkspace.group(), Group::Workspaces);
        assert_eq!(Command::ToggleSidebar.group(), Group::General);
        assert_eq!(Command::ShowKeymap.group(), Group::General);
        assert_eq!(Command::ReloadConfig.group(), Group::General);
        assert_eq!(Group::Workspaces.label(), "Workspaces");
    }

    #[test]
    fn help_groups_bindings_in_a_fixed_order() {
        let mut keymap = Keymap::default();
        keymap.set(
            trigger("ctrl-shift-t"),
            Action::Command(Command::NewTab),
            None,
        );
        let groups = keymap.help_groups("ctrl-b");
        let names: Vec<_> = groups.iter().map(|(group, _)| *group).collect();
        assert_eq!(
            names,
            [Group::Tabs, Group::Panes, Group::Workspaces, Group::General]
        );
        let tabs = &groups[0].1;
        let entry = |key: &str, desc: &str| (key.to_string(), desc.to_string());
        assert_eq!(tabs[0], entry("ctrl-b c", "New tab"));
        assert!(tabs.contains(&entry("ctrl-b 1-9", "Select tab N")));
        // prefix を使わない割り当ては、prefix 付きの後に並べる
        assert_eq!(tabs.last(), Some(&entry("ctrl-shift-t", "New tab")));
        assert!(
            groups[3]
                .1
                .contains(&entry("ctrl-b ?", "Show key bindings"))
        );
    }

    #[test]
    fn help_groups_skip_empty_groups() {
        let mut keymap = Keymap::default();
        for key in ["shift-c", "shift-n", "shift-p"] {
            keymap.del(&trigger(&format!("<prefix>{key}")));
        }
        let names: Vec<_> = keymap
            .help_groups("ctrl-b")
            .into_iter()
            .map(|(group, _)| group)
            .collect();
        assert_eq!(names, [Group::Tabs, Group::Panes, Group::General]);
    }

    #[test]
    fn entries_split_a_broken_digit_run() {
        let mut keymap = Keymap::default();
        keymap.del(&trigger("<prefix>3"));
        let keys: Vec<_> = keymap
            .entries()
            .into_iter()
            .filter(|(_, desc)| desc == "Select tab N")
            .map(|(key, _)| key)
            .collect();
        assert_eq!(keys, ["1-2", "4-9"]);
    }

    #[test]
    fn entries_join_other_keys_with_the_same_description() {
        let mut keymap = Keymap::default();
        keymap.set(trigger("<prefix>t"), Action::Command(Command::NewTab), None);
        keymap.set(
            trigger("<prefix>shift-t"),
            Action::Command(Command::NewTab),
            None,
        );
        assert_eq!(
            keymap.entries().last(),
            Some(&("t/shift-t".to_string(), "New tab".to_string()))
        );
    }
}
