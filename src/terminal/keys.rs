//! gpui のキー入力を PTY に送るバイト列へ変換する（xterm 互換）。

use gpui::{Keystroke, Modifiers};

/// キー入力を PTY へ書き込むバイト列に変換する。
///
/// `app_cursor` はターミナルが DECCKM (アプリケーションカーソルキーモード) の場合に true。
/// 変換対象外のキー（修飾キー単体や Super 併用など）は `None`。
pub fn keystroke_to_bytes(keystroke: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let modifiers = &keystroke.modifiers;
    if modifiers.platform {
        return None;
    }
    let key = keystroke.key.as_str();

    if let Some(sequence) = csi_key(key, modifiers, app_cursor) {
        return Some(sequence);
    }

    let bytes = match key {
        "enter" => vec![b'\r'],
        "tab" if modifiers.shift => b"\x1b[Z".to_vec(),
        "tab" => vec![b'\t'],
        "escape" => vec![0x1b],
        "backspace" if modifiers.control => vec![0x08],
        "backspace" => vec![0x7f],
        _ if modifiers.control => vec![control_byte(key)?],
        _ => keystroke.key_char.as_ref()?.as_bytes().to_vec(),
    };

    Some(if modifiers.alt {
        [b"\x1b".as_slice(), &bytes].concat()
    } else {
        bytes
    })
}

/// xterm の修飾パラメータ（1 + Shift(1) + Alt(2) + Ctrl(4)）。修飾なしは `None`。
fn modifier_param(modifiers: &Modifiers) -> Option<u8> {
    let param = 1
        + u8::from(modifiers.shift)
        + 2 * u8::from(modifiers.alt)
        + 4 * u8::from(modifiers.control);
    (param > 1).then_some(param)
}

/// カーソルキー・編集キー・ファンクションキーの CSI / SS3 シーケンス。
fn csi_key(key: &str, modifiers: &Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // 末尾文字で区別するキー: CSI 1;m X / SS3 X / CSI X
    let letter = match key {
        "up" => Some(b'A'),
        "down" => Some(b'B'),
        "right" => Some(b'C'),
        "left" => Some(b'D'),
        "home" => Some(b'H'),
        "end" => Some(b'F'),
        "f1" => Some(b'P'),
        "f2" => Some(b'Q'),
        "f3" => Some(b'R'),
        "f4" => Some(b'S'),
        _ => None,
    };
    if let Some(letter) = letter {
        let is_function_key = key.starts_with('f');
        return Some(match modifier_param(modifiers) {
            Some(param) => format!("\x1b[1;{param}{}", letter as char).into_bytes(),
            None if is_function_key || app_cursor => vec![0x1b, b'O', letter],
            None => vec![0x1b, b'[', letter],
        });
    }

    // 番号で区別するキー: CSI n ~ / CSI n;m ~
    let number = match key {
        "insert" => 2,
        "delete" => 3,
        "pageup" => 5,
        "pagedown" => 6,
        "f5" => 15,
        "f6" => 17,
        "f7" => 18,
        "f8" => 19,
        "f9" => 20,
        "f10" => 21,
        "f11" => 23,
        "f12" => 24,
        _ => return None,
    };
    Some(
        match modifier_param(modifiers) {
            Some(param) => format!("\x1b[{number};{param}~"),
            None => format!("\x1b[{number}~"),
        }
        .into_bytes(),
    )
}

/// Ctrl+キーに対応する C0 制御文字。
fn control_byte(key: &str) -> Option<u8> {
    match key {
        "space" | "@" | "2" => Some(0x00),
        "[" | "3" => Some(0x1b),
        "\\" | "4" => Some(0x1c),
        "]" | "5" => Some(0x1d),
        "^" | "6" => Some(0x1e),
        "_" | "-" | "/" | "7" => Some(0x1f),
        "?" | "8" => Some(0x7f),
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

    #[test]
    fn modified_cursor_keys_use_xterm_parameters() {
        assert_eq!(bytes("shift-up"), Some(b"\x1b[1;2A".to_vec()));
        assert_eq!(bytes("alt-left"), Some(b"\x1b[1;3D".to_vec()));
        assert_eq!(bytes("ctrl-left"), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(bytes("ctrl-right"), Some(b"\x1b[1;5C".to_vec()));
        assert_eq!(bytes("ctrl-shift-home"), Some(b"\x1b[1;6H".to_vec()));
        assert_eq!(bytes("ctrl-delete"), Some(b"\x1b[3;5~".to_vec()));
        assert_eq!(bytes("shift-pageup"), Some(b"\x1b[5;2~".to_vec()));
    }

    #[test]
    fn modified_cursor_keys_ignore_app_cursor_mode() {
        let ctrl_up = Keystroke::parse("ctrl-up").unwrap();
        assert_eq!(
            keystroke_to_bytes(&ctrl_up, true),
            Some(b"\x1b[1;5A".to_vec())
        );
    }

    #[test]
    fn home_end_in_app_cursor_mode() {
        let home = Keystroke::parse("home").unwrap();
        assert_eq!(keystroke_to_bytes(&home, true), Some(b"\x1bOH".to_vec()));
        let end = Keystroke::parse("end").unwrap();
        assert_eq!(keystroke_to_bytes(&end, true), Some(b"\x1bOF".to_vec()));
    }

    #[test]
    fn insert_key() {
        assert_eq!(bytes("insert"), Some(b"\x1b[2~".to_vec()));
    }

    #[test]
    fn function_keys() {
        assert_eq!(bytes("f1"), Some(b"\x1bOP".to_vec()));
        assert_eq!(bytes("f4"), Some(b"\x1bOS".to_vec()));
        assert_eq!(bytes("f5"), Some(b"\x1b[15~".to_vec()));
        assert_eq!(bytes("f10"), Some(b"\x1b[21~".to_vec()));
        assert_eq!(bytes("f12"), Some(b"\x1b[24~".to_vec()));
        assert_eq!(bytes("shift-f1"), Some(b"\x1b[1;2P".to_vec()));
        assert_eq!(bytes("ctrl-f5"), Some(b"\x1b[15;5~".to_vec()));
    }

    #[test]
    fn control_with_editing_keys() {
        assert_eq!(bytes("ctrl-backspace"), Some(vec![0x08]));
        assert_eq!(bytes("ctrl-enter"), Some(b"\r".to_vec()));
        assert_eq!(bytes("ctrl-tab"), Some(b"\t".to_vec()));
    }

    #[test]
    fn control_digits_and_slash() {
        assert_eq!(bytes("ctrl-/"), Some(vec![0x1f]));
        assert_eq!(bytes("ctrl-3"), Some(vec![0x1b]));
        assert_eq!(bytes("ctrl-4"), Some(vec![0x1c]));
        assert_eq!(bytes("ctrl-5"), Some(vec![0x1d]));
        assert_eq!(bytes("ctrl-7"), Some(vec![0x1f]));
        assert_eq!(bytes("ctrl-8"), Some(vec![0x7f]));
    }

    #[test]
    fn control_alt_prefixes_escape() {
        assert_eq!(bytes("ctrl-alt-a"), Some(vec![0x1b, 0x01]));
    }

    #[test]
    fn unknown_control_combination_sends_nothing() {
        assert_eq!(bytes("ctrl-f13"), None);
    }
}
