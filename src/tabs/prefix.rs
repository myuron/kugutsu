//! tmux 風の prefix キーによるタブ操作のキーマップ。
//!
//! prefix を押すと次の 1 キーをタブ操作として解釈する。prefix を 2 回押すと prefix 自体を
//! ターミナルへ送る。割り当ての無いキーは捨てる。

use gpui::Keystroke;

/// prefix キー（gpui のキーストローク表記）。
pub const PREFIX: &str = "ctrl-b";

/// タブに対する操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabCommand {
    New,
    Close,
    Next,
    Prev,
    /// 0 始まりの位置のタブへ切り替える。
    Select(usize),
}

/// キー入力をどう扱うか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// ターミナルへそのまま送る。
    Forward,
    /// キーマップが消費した（ターミナルへは送らない）。
    Consumed,
    Command(TabCommand),
}

#[derive(Debug, Default)]
pub struct PrefixKeymap {
    armed: bool,
}

impl PrefixKeymap {
    pub fn handle(&mut self, keystroke: &Keystroke) -> KeyOutcome {
        let is_prefix = is_prefix(keystroke);
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

/// prefix の後に押されたキーに対応するタブ操作。
fn command_for(keystroke: &Keystroke) -> Option<TabCommand> {
    if keystroke.modifiers.modified() {
        return None;
    }
    let command = match keystroke.key.as_str() {
        "c" => TabCommand::New,
        "x" => TabCommand::Close,
        "n" => TabCommand::Next,
        "p" => TabCommand::Prev,
        key => match key.parse::<usize>() {
            Ok(number @ 1..=9) => TabCommand::Select(number - 1),
            _ => return None,
        },
    };
    Some(command)
}

fn is_prefix(keystroke: &Keystroke) -> bool {
    let prefix = Keystroke::parse(PREFIX).expect("PREFIX is a valid keystroke");
    keystroke.key == prefix.key && keystroke.modifiers == prefix.modifiers
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
        assert_eq!(press(&mut keymap, PREFIX), KeyOutcome::Consumed);
        assert!(keymap.is_armed());
    }

    #[test]
    fn maps_keys_after_prefix_to_commands() {
        let cases = [
            ("c", TabCommand::New),
            ("x", TabCommand::Close),
            ("n", TabCommand::Next),
            ("p", TabCommand::Prev),
            ("1", TabCommand::Select(0)),
            ("3", TabCommand::Select(2)),
            ("9", TabCommand::Select(8)),
        ];
        for (key, command) in cases {
            let mut keymap = PrefixKeymap::default();
            press(&mut keymap, PREFIX);
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
        press(&mut keymap, PREFIX);
        assert_eq!(press(&mut keymap, PREFIX), KeyOutcome::Forward);
        assert!(!keymap.is_armed());
    }

    #[test]
    fn unknown_key_after_prefix_is_discarded() {
        let mut keymap = PrefixKeymap::default();
        press(&mut keymap, PREFIX);
        assert_eq!(press(&mut keymap, "z"), KeyOutcome::Consumed);
        assert!(!keymap.is_armed());
        assert_eq!(press(&mut keymap, "c"), KeyOutcome::Forward);
    }

    #[test]
    fn escape_cancels_prefix() {
        let mut keymap = PrefixKeymap::default();
        press(&mut keymap, PREFIX);
        assert_eq!(press(&mut keymap, "escape"), KeyOutcome::Consumed);
        assert!(!keymap.is_armed());
    }

    #[test]
    fn modified_keys_are_not_commands() {
        for key in ["ctrl-c", "alt-n", "shift-x", "0"] {
            let mut keymap = PrefixKeymap::default();
            press(&mut keymap, PREFIX);
            assert_eq!(press(&mut keymap, key), KeyOutcome::Consumed, "{key}");
        }
    }
}
