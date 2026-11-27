//! カラーテーマ。
//!
//! テーマはターミナルの配色と Kugutsu の UI の色をまとめたもの。テーマファイルは Lua で、
//! テーマの table を返す（`colors/<name>.lua` のファイル名がテーマ名になる）。
//! Kugutsu は `kugutsu-dark`（既定）と `kugutsu-light` を同梱している。

use std::path::PathBuf;
use std::sync::LazyLock;

use mlua::{Lua, Table, Value};

use crate::config;

/// 既定のテーマの名前。
pub const DEFAULT_THEME: &str = "kugutsu-dark";

/// 同梱テーマ（名前とソース）。
const BUNDLED: [(&str, &str); 2] = [
    (
        "kugutsu-dark",
        include_str!("../../runtime/colors/kugutsu-dark.lua"),
    ),
    (
        "kugutsu-light",
        include_str!("../../runtime/colors/kugutsu-light.lua"),
    ),
];

/// パース済みの同梱テーマ。同梱ファイルが正しいことはテストで確かめている。
static BUILTIN: LazyLock<Vec<Theme>> = LazyLock::new(|| {
    BUNDLED
        .iter()
        .map(|(name, source)| {
            parse(source, name).unwrap_or_else(|err| panic!("bundled theme is invalid: {err}"))
        })
        .collect()
});

/// カラーテーマ。色は 0xRRGGBB。
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub terminal: TerminalColors,
    pub ui: UiColors,
}

/// ターミナルの配色。
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalColors {
    pub foreground: u32,
    pub background: u32,
    pub cursor: u32,
    /// ANSI 16 色パレット（0-7: 通常, 8-15: 明るい色）。
    pub ansi: [u32; 16],
}

/// Kugutsu の UI の色。
#[derive(Debug, Clone, PartialEq)]
pub struct UiColors {
    /// タブバー・サイドバー・ステータスラインの背景。
    pub bar_background: u32,
    /// バーの境界線とペインの区切り線。
    pub bar_border: u32,
    /// 非アクティブな項目の文字色。
    pub inactive_foreground: u32,
    /// 分割時にアクティブなペインを囲む枠。
    pub active_pane_border: u32,
    /// prefix キーの表示。
    pub prefix_indicator: u32,
    pub agent_working: u32,
    pub agent_blocked: u32,
    pub agent_done: u32,
    /// 設定のエラー表示。
    pub error: u32,
}

impl Default for Theme {
    fn default() -> Self {
        builtin(DEFAULT_THEME)
            .expect("default theme is bundled")
            .clone()
    }
}

/// 名前から同梱テーマを引く。
pub fn builtin(name: &str) -> Option<&'static Theme> {
    BUILTIN.iter().find(|theme| theme.name == name)
}

/// 名前からテーマを探す。`dirs`（`colors/` ディレクトリ）を順に見て `<name>.lua` があれば
/// それを読み、どこにも無ければ同梱テーマを使う。
pub fn find(name: &str, dirs: &[PathBuf]) -> Result<Theme, String> {
    let unknown = || format!("unknown theme '{name}'");
    // ファイル名としてだけ使い、ほかのディレクトリを指せないようにする
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']) {
        return Err(unknown());
    }
    for dir in dirs {
        let path = dir.join(format!("{name}.lua"));
        match std::fs::read_to_string(&path) {
            Ok(source) => return parse(&source, name),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("{}: {err}", path.display())),
        }
    }
    builtin(name).cloned().ok_or_else(unknown)
}

/// テーマファイルを評価してテーマを読み込む。`name` はテーマ名で、エラーメッセージにも使う。
pub fn parse(source: &str, name: &str) -> Result<Theme, String> {
    let lua = Lua::new();
    let value: Value = lua
        .load(source)
        .set_name(format!("={name}.lua"))
        .eval()
        .map_err(|err| config::error_message(&err))?;
    read_theme(&value)
        .map(|(terminal, ui)| Theme {
            name: name.to_string(),
            terminal,
            ui,
        })
        .map_err(|err| format!("{name}: {err}"))
}

const TERMINAL_KEYS: [&str; 4] = ["foreground", "background", "cursor", "ansi"];
const UI_KEYS: [&str; 9] = [
    "bar_background",
    "bar_border",
    "inactive_foreground",
    "active_pane_border",
    "prefix_indicator",
    "agent_working",
    "agent_blocked",
    "agent_done",
    "error",
];

fn read_theme(value: &Value) -> Result<(TerminalColors, UiColors), String> {
    let root = table(value).map_err(|reason| reason.to_string())?;
    check_keys(&root, "", &["terminal", "ui"])?;

    let terminal = table(&field(&root, "", "terminal")?).map_err(|r| format!("terminal: {r}"))?;
    check_keys(&terminal, "terminal.", &TERMINAL_KEYS)?;
    let color_of = |table: &Table, path: &str, key: &str| {
        color(&field(table, path, key)?, &format!("{path}{key}"))
    };
    let terminal_colors = TerminalColors {
        foreground: color_of(&terminal, "terminal.", "foreground")?,
        background: color_of(&terminal, "terminal.", "background")?,
        cursor: color_of(&terminal, "terminal.", "cursor")?,
        ansi: ansi(&field(&terminal, "terminal.", "ansi")?)?,
    };

    let ui = table(&field(&root, "", "ui")?).map_err(|r| format!("ui: {r}"))?;
    check_keys(&ui, "ui.", &UI_KEYS)?;
    let ui_color = |key: &str| color_of(&ui, "ui.", key);
    let ui_colors = UiColors {
        bar_background: ui_color("bar_background")?,
        bar_border: ui_color("bar_border")?,
        inactive_foreground: ui_color("inactive_foreground")?,
        active_pane_border: ui_color("active_pane_border")?,
        prefix_indicator: ui_color("prefix_indicator")?,
        agent_working: ui_color("agent_working")?,
        agent_blocked: ui_color("agent_blocked")?,
        agent_done: ui_color("agent_done")?,
        error: ui_color("error")?,
    };
    Ok((terminal_colors, ui_colors))
}

fn table(value: &Value) -> Result<Table, String> {
    match value {
        Value::Table(table) => Ok(table.clone()),
        other => Err(format!(
            "expected a table, got {}",
            config::lua_type_name(other)
        )),
    }
}

/// `table[key]`。無ければエラー。`path` はエラーに出すキーの接頭辞（`"ui."` など）。
fn field(table: &Table, path: &str, key: &str) -> Result<Value, String> {
    match table.raw_get::<Value>(key) {
        Ok(Value::Nil) => Err(format!("{path}{key}: missing")),
        Ok(value) => Ok(value),
        Err(err) => Err(format!("{path}{key}: {err}")),
    }
}

/// `table` に `allowed` 以外のキーがあればエラーにする。
fn check_keys(table: &Table, path: &str, allowed: &[&str]) -> Result<(), String> {
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|err| err.to_string())?;
        let known = match &key {
            Value::String(text) => allowed.contains(&&*text.to_string_lossy()),
            _ => false,
        };
        if !known {
            let key = match &key {
                Value::String(text) => text.to_string_lossy(),
                other => format!("{other:?}"),
            };
            return Err(format!("unknown key '{path}{key}'"));
        }
    }
    Ok(())
}

/// `"#rrggbb"` を 0xRRGGBB にする。
fn color(value: &Value, path: &str) -> Result<u32, String> {
    let invalid = |got: String| format!("{path}: expected a color like \"#rrggbb\", got {got}");
    let Value::String(text) = value else {
        return Err(invalid(config::lua_type_name(value).to_string()));
    };
    let text = text.to_string_lossy();
    text.strip_prefix('#')
        .filter(|hex| hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .ok_or_else(|| invalid(format!("{text:?}")))
}

fn ansi(value: &Value) -> Result<[u32; 16], String> {
    let table = table(value).map_err(|r| format!("terminal.ansi: {r}"))?;
    let len = table.raw_len();
    if len != 16 || table.pairs::<Value, Value>().count() != 16 {
        return Err(format!("terminal.ansi: expected 16 colors, got {len}"));
    }
    let mut colors = [0; 16];
    for (index, slot) in colors.iter_mut().enumerate() {
        let value: Value = table.raw_get(index + 1).map_err(|err| err.to_string())?;
        *slot = color(&value, &format!("terminal.ansi[{}]", index + 1))?;
    }
    Ok(colors)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARK_ANSI: [u32; 16] = [
        0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666,
        0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
    ];

    /// `kugutsu-dark` の table を `edit` で書き換えたテーマファイル。
    fn dark_with(edit: &str) -> String {
        let source = BUNDLED[0].1.replacen("return {", "local theme = {", 1);
        format!("{source}\n{edit}\nreturn theme")
    }

    fn parse_err(source: &str, expected: &str) {
        let err = parse(source, "test").unwrap_err();
        assert!(
            err.contains(expected),
            "{err:?} does not contain {expected:?}"
        );
    }

    #[test]
    fn bundled_themes_parse() {
        for (name, source) in BUNDLED {
            let theme = parse(source, name).unwrap_or_else(|err| panic!("{err}"));
            assert_eq!(theme.name, name);
        }
    }

    #[test]
    fn dark_theme_keeps_the_original_colors() {
        let theme = Theme::default();
        assert_eq!(theme.name, "kugutsu-dark");
        assert_eq!(
            theme.terminal,
            TerminalColors {
                foreground: 0xd4d4d4,
                background: 0x1e1e1e,
                cursor: 0xaeafad,
                ansi: DARK_ANSI,
            }
        );
        assert_eq!(
            theme.ui,
            UiColors {
                bar_background: 0x181818,
                bar_border: 0x2b2b2b,
                inactive_foreground: 0x8a8a8a,
                active_pane_border: 0x569cd6,
                prefix_indicator: 0xdcdcaa,
                agent_working: 0x569cd6,
                agent_blocked: 0xe5c07b,
                agent_done: 0x89d185,
                error: 0xf14c4c,
            }
        );
    }

    #[test]
    fn light_theme_is_bundled() {
        let theme = builtin("kugutsu-light").unwrap();
        assert_eq!(theme.terminal.background, 0xffffff);
        assert_eq!(theme.terminal.foreground, 0x333333);
        assert_eq!(theme.ui.bar_background, 0xf3f3f3);
        assert!(builtin("no-such-theme").is_none());
    }

    #[test]
    fn colors_are_case_insensitive_hex() {
        let theme = parse(&dark_with(r##"theme.terminal.cursor = "#ABCdef""##), "test").unwrap();
        assert_eq!(theme.terminal.cursor, 0xabcdef);
    }

    #[test]
    fn invalid_colors_are_errors() {
        for color in [
            r#""red""#,
            r##""#fff""##,
            r##""#gggggg""##,
            r##""#1234567""##,
        ] {
            parse_err(
                &dark_with(&format!("theme.ui.bar_border = {color}")),
                "test: ui.bar_border: expected a color like \"#rrggbb\"",
            );
        }
        parse_err(
            &dark_with("theme.terminal.ansi[3] = 1"),
            "test: terminal.ansi[3]: expected a color like \"#rrggbb\", got number",
        );
    }

    #[test]
    fn missing_keys_are_errors() {
        parse_err(
            &dark_with("theme.ui.agent_done = nil"),
            "ui.agent_done: missing",
        );
        parse_err(&dark_with("theme.terminal = nil"), "terminal: missing");
    }

    #[test]
    fn ansi_needs_sixteen_colors() {
        parse_err(
            &dark_with("theme.terminal.ansi[16] = nil"),
            "terminal.ansi: expected 16 colors, got 15",
        );
        parse_err(
            &dark_with(r##"theme.terminal.ansi[17] = "#000000""##),
            "terminal.ansi: expected 16 colors, got 17",
        );
    }

    #[test]
    fn unknown_keys_are_errors() {
        parse_err(&dark_with("theme.foo = 1"), "test: unknown key 'foo'");
        parse_err(
            &dark_with(r##"theme.ui.foo = "#000000""##),
            "test: unknown key 'ui.foo'",
        );
    }

    #[test]
    fn non_table_values_are_errors() {
        parse_err("return 1", "test: expected a table, got number");
        parse_err(
            &dark_with("theme.ui = 1"),
            "ui: expected a table, got number",
        );
    }

    #[test]
    fn lua_errors_are_reported() {
        parse_err("error('boom')", "test.lua:1: boom");
        parse_err("this is not lua", "test.lua:1:");
    }
}
