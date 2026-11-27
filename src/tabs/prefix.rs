//! tmux 風の prefix キーによるタブ・ペイン操作のキーマップ。
//!
//! prefix を押すと次の 1 キーを操作として解釈する。prefix を 2 回押すと prefix 自体を
//! ターミナルへ送る。割り当ての無いキーは捨てる。

use gpui::Keystroke;

use crate::panes::{Axis, Direction};

/// prefix キー（gpui のキーストローク表記）。
pub const DEFAULT_PREFIX: &str = "ctrl-b";

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
}

impl Command {
    /// キーマップ一覧に表示する説明。
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
        }
    }
}

/// prefix の後のキー（gpui のキーストローク表記）と操作の対応。
/// 数字キーによるタブ選択（`SelectTab`）だけは範囲なので `command_for` で別に扱う。
const KEYS: &[(&str, Command)] = &[
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
];

/// キーマップ一覧に表示する（キー操作, 説明）の組。
/// キーは prefix の後に押すもの（prefix 自体は含まない）。
pub fn bindings() -> Vec<(&'static str, &'static str)> {
    let mut bindings: Vec<_> = KEYS
        .iter()
        .map(|&(key, command)| (key, command.description()))
        .collect();
    // タブ切り替えの行の後ろに並べる
    let select_tab = KEYS
        .iter()
        .position(|&(_, command)| command == Command::PrevTab)
        .map_or(bindings.len(), |index| index + 1);
    bindings.insert(select_tab, ("1-9", Command::SelectTab(0).description()));
    bindings
}

/// キー入力をどう扱うか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// ターミナルへそのまま送る。
    Forward,
    /// キーマップが消費した（ターミナルへは送らない）。
    Consumed,
    Command(Command),
}

#[derive(Debug)]
pub struct PrefixKeymap {
    prefix: Keystroke,
    armed: bool,
}

impl Default for PrefixKeymap {
    fn default() -> Self {
        Self::new(Keystroke::parse(DEFAULT_PREFIX).expect("default prefix is a valid keystroke"))
    }
}

impl PrefixKeymap {
    pub fn new(prefix: Keystroke) -> Self {
        Self {
            prefix,
            armed: false,
        }
    }

    /// prefix キー。
    pub fn prefix(&self) -> &Keystroke {
        &self.prefix
    }

    pub fn handle(&mut self, keystroke: &Keystroke) -> KeyOutcome {
        let is_prefix = same_key(keystroke, &self.prefix);
        if !std::mem::take(&mut self.armed) {
            self.armed = is_prefix;
            return if is_prefix {
                KeyOutcome::Consumed
            } else {
                KeyOutcome::Forward
            };
        }
        if is_prefix {
            return KeyOutcome::Forward;
        }
        command_for(keystroke).map_or(KeyOutcome::Consumed, KeyOutcome::Command)
    }

    /// prefix が押され、次のキーを待っているか。
    pub fn is_armed(&self) -> bool {
        self.armed
    }
}

/// prefix の後に押されたキーに対応する操作。
fn command_for(keystroke: &Keystroke) -> Option<Command> {
    if let Some(&(_, command)) = KEYS.iter().find(|(key, _)| matches(keystroke, key)) {
        return Some(command);
    }
    if keystroke.modifiers.modified() {
        return None;
    }
    match keystroke.key.parse::<usize>() {
        Ok(number @ 1..=9) => Some(Command::SelectTab(number - 1)),
        _ => None,
    }
}

/// `keystroke` が gpui 表記の `expected` と同じキー・修飾キーか。
fn matches(keystroke: &Keystroke, expected: &str) -> bool {
    let expected = Keystroke::parse(expected).expect("key binding is a valid keystroke");
    same_key(keystroke, &expected)
}

/// 2 つのキーストロークが同じキー・修飾キーか（IME の入力結果などは比べない）。
fn same_key(keystroke: &Keystroke, expected: &Keystroke) -> bool {
    keystroke.key == expected.key && keystroke.modifiers == expected.modifiers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(keymap: &mut PrefixKeymap, key: &str) -> KeyOutcome {
        keymap.handle(&Keystroke::parse(key).unwrap())
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
        ];
        for (key, command) in cases {
            let mut keymap = PrefixKeymap::default();
            press(&mut keymap, DEFAULT_PREFIX);
            assert_eq!(
                press(&mut keymap, key),
                KeyOutcome::Command(command),
                "{key}"
            );
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
    fn listed_bindings_trigger_their_commands() {
        for &(key, command) in KEYS {
            let mut keymap = PrefixKeymap::default();
            press(&mut keymap, DEFAULT_PREFIX);
            assert_eq!(
                press(&mut keymap, key),
                KeyOutcome::Command(command),
                "{key}"
            );
        }
    }

    #[test]
    fn bindings_list_every_key_after_prefix() {
        let bindings = bindings();
        assert_eq!(bindings.len(), KEYS.len() + 1);
        assert!(bindings.contains(&("?", "Show key bindings")));
        assert!(bindings.contains(&("1-9", "Select tab N")));
        for (key, description) in &bindings {
            assert!(!key.contains(DEFAULT_PREFIX), "{key}");
            assert!(!description.is_empty(), "{key}");
        }
    }

    #[test]
    fn uses_a_custom_prefix() {
        let mut keymap = PrefixKeymap::new(Keystroke::parse("ctrl-a").unwrap());
        assert_eq!(press(&mut keymap, DEFAULT_PREFIX), KeyOutcome::Forward);
        assert!(!keymap.is_armed());
        assert_eq!(press(&mut keymap, "ctrl-a"), KeyOutcome::Consumed);
        assert!(keymap.is_armed());
        assert_eq!(
            press(&mut keymap, "c"),
            KeyOutcome::Command(Command::NewTab)
        );
    }

    #[test]
    fn double_custom_prefix_forwards_it() {
        let mut keymap = PrefixKeymap::new(Keystroke::parse("ctrl-a").unwrap());
        press(&mut keymap, "ctrl-a");
        assert_eq!(press(&mut keymap, "ctrl-a"), KeyOutcome::Forward);
        assert!(!keymap.is_armed());
    }

    #[test]
    fn exposes_the_prefix_for_display() {
        let keymap = PrefixKeymap::new(Keystroke::parse("ctrl-a").unwrap());
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
}
