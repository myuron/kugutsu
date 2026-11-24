//! gpui のキー入力を PTY に送るバイト列へ変換する。

use gpui::Keystroke;

/// キー入力を PTY へ書き込むバイト列に変換する。
///
/// `app_cursor` はターミナルが DECCKM (アプリケーションカーソルキーモード) の場合に true。
/// 変換対象外のキー（修飾キー単体や Super 併用など）は `None`。
pub fn keystroke_to_bytes(keystroke: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let modifiers = &keystroke.modifiers;
    if modifiers.platform {
        return None;
    }

    let bytes = if modifiers.control {
        vec![control_byte(&keystroke.key)?]
    } else if let Some(sequence) = special_key(&keystroke.key, modifiers.shift, app_cursor) {
        sequence.to_vec()
    } else {
        keystroke.key_char.as_ref()?.as_bytes().to_vec()
    };

    Some(if modifiers.alt {
        [b"\x1b".as_slice(), &bytes].concat()
    } else {
        bytes
    })
}

fn special_key(key: &str, shift: bool, app_cursor: bool) -> Option<&'static [u8]> {
    let sequence: &[u8] = match (key, app_cursor) {
        ("enter", _) => b"\r",
        ("backspace", _) => b"\x7f",
        ("tab", _) if shift => b"\x1b[Z",
        ("tab", _) => b"\t",
        ("escape", _) => b"\x1b",
        ("delete", _) => b"\x1b[3~",
        ("insert", _) => b"\x1b[2~",
        ("pageup", _) => b"\x1b[5~",
        ("pagedown", _) => b"\x1b[6~",
        ("up", false) => b"\x1b[A",
        ("down", false) => b"\x1b[B",
        ("right", false) => b"\x1b[C",
        ("left", false) => b"\x1b[D",
        ("home", false) => b"\x1b[H",
        ("end", false) => b"\x1b[F",
        ("up", true) => b"\x1bOA",
        ("down", true) => b"\x1bOB",
        ("right", true) => b"\x1bOC",
        ("left", true) => b"\x1bOD",
        ("home", true) => b"\x1bOH",
        ("end", true) => b"\x1bOF",
        _ => return None,
    };
    Some(sequence)
}

/// Ctrl+キーに対応する C0 制御文字。
fn control_byte(key: &str) -> Option<u8> {
    match key {
        "space" | "@" | "2" => Some(0x00),
        "[" => Some(0x1b),
        "\\" => Some(0x1c),
        "]" => Some(0x1d),
        "^" | "6" => Some(0x1e),
        "_" | "-" => Some(0x1f),
        "?" => Some(0x7f),
        _ => match key.as_bytes() {
            [c @ b'a'..=b'z'] => Some(c - b'a' + 1),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(source: &str) -> Option<Vec<u8>> {
        keystroke_to_bytes(&Keystroke::parse(source).unwrap(), false)
    }

    fn typed(key: &str, key_char: &str) -> Keystroke {
        let mut keystroke = Keystroke::parse(key).unwrap();
        keystroke.key_char = Some(key_char.to_string());
        keystroke
    }

    #[test]
    fn printable_characters_use_key_char() {
        assert_eq!(
            keystroke_to_bytes(&typed("a", "a"), false),
            Some(b"a".to_vec())
        );
        assert_eq!(
            keystroke_to_bytes(&typed("shift-a", "A"), false),
            Some(b"A".to_vec())
        );
        assert_eq!(
            keystroke_to_bytes(&typed("space", " "), false),
            Some(b" ".to_vec())
        );
        assert_eq!(
            keystroke_to_bytes(&typed("a", "あ"), false),
            Some("あ".as_bytes().to_vec())
        );
    }

    #[test]
    fn special_keys() {
        assert_eq!(bytes("enter"), Some(b"\r".to_vec()));
        assert_eq!(bytes("backspace"), Some(b"\x7f".to_vec()));
        assert_eq!(bytes("tab"), Some(b"\t".to_vec()));
        assert_eq!(bytes("shift-tab"), Some(b"\x1b[Z".to_vec()));
        assert_eq!(bytes("escape"), Some(b"\x1b".to_vec()));
        assert_eq!(bytes("delete"), Some(b"\x1b[3~".to_vec()));
        assert_eq!(bytes("home"), Some(b"\x1b[H".to_vec()));
        assert_eq!(bytes("end"), Some(b"\x1b[F".to_vec()));
        assert_eq!(bytes("pageup"), Some(b"\x1b[5~".to_vec()));
        assert_eq!(bytes("pagedown"), Some(b"\x1b[6~".to_vec()));
    }

    #[test]
    fn arrow_keys_in_normal_mode() {
        assert_eq!(bytes("up"), Some(b"\x1b[A".to_vec()));
        assert_eq!(bytes("down"), Some(b"\x1b[B".to_vec()));
        assert_eq!(bytes("right"), Some(b"\x1b[C".to_vec()));
        assert_eq!(bytes("left"), Some(b"\x1b[D".to_vec()));
    }

    #[test]
    fn arrow_keys_in_app_cursor_mode() {
        let up = Keystroke::parse("up").unwrap();
        assert_eq!(keystroke_to_bytes(&up, true), Some(b"\x1bOA".to_vec()));
        let left = Keystroke::parse("left").unwrap();
        assert_eq!(keystroke_to_bytes(&left, true), Some(b"\x1bOD".to_vec()));
    }

    #[test]
    fn control_letters() {
        assert_eq!(bytes("ctrl-a"), Some(vec![0x01]));
        assert_eq!(bytes("ctrl-c"), Some(vec![0x03]));
        assert_eq!(bytes("ctrl-z"), Some(vec![0x1a]));
        assert_eq!(bytes("ctrl-shift-c"), Some(vec![0x03]));
    }

    #[test]
    fn control_symbols() {
        assert_eq!(bytes("ctrl-space"), Some(vec![0x00]));
        assert_eq!(bytes("ctrl-["), Some(vec![0x1b]));
        assert_eq!(bytes("ctrl-\\"), Some(vec![0x1c]));
        assert_eq!(bytes("ctrl-]"), Some(vec![0x1d]));
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(
            keystroke_to_bytes(&typed("alt-b", "b"), false),
            Some(b"\x1bb".to_vec())
        );
        assert_eq!(bytes("alt-backspace"), Some(b"\x1b\x7f".to_vec()));
    }

    #[test]
    fn ignores_platform_modifier_and_bare_modifiers() {
        assert_eq!(keystroke_to_bytes(&typed("super-c", "c"), false), None);
        assert_eq!(bytes("shift"), None);
    }
}
