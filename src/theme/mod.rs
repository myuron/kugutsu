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
    // 以下はテーマファイルで省略でき、省略すると上の色から導出する
    /// UI の文字色（既定: `terminal.foreground`）。
    pub foreground: u32,
    /// アクティブなペインの枠・グローなどの強調色（既定: `active_pane_border`）。
    pub accent: u32,
    /// アクティブな項目の背景（既定: `bar_background` を `foreground` へ少し寄せた色）。
    pub raised_background: u32,
    /// モーダル・which-key の背景（既定: `raised_background` よりさらに寄せた色）。
    pub overlay_background: u32,
    /// ペインやモーダルの枠（既定: `bar_border`）。
    pub strong_border: u32,
}

/// `raised_background` を省略したときに `bar_background` を `foreground` へ寄せる割合。
const RAISED_MIX: f32 = 0.06;
/// `overlay_background` を省略したときの割合。
const OVERLAY_MIX: f32 = 0.1;

/// 色 `a` と `b` を `t`（0.0 で `a`、1.0 で `b`）の割合で混ぜる。
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let channel = |shift: u32| {
        let from = ((a >> shift) & 0xff) as f32;
        let to = ((b >> shift) & 0xff) as f32;
        ((from + (to - from) * t).round() as u32 & 0xff) << shift
    };
    channel(16) | channel(8) | channel(0)
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
/// 省略できる UI の色。
const OPTIONAL_UI_KEYS: [&str; 5] = [
    "foreground",
    "accent",
    "raised_background",
    "overlay_background",
    "strong_border",
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
    let allowed: Vec<&str> = UI_KEYS.iter().chain(&OPTIONAL_UI_KEYS).copied().collect();
    check_keys(&ui, "ui.", &allowed)?;
    let ui_color = |key: &str| color_of(&ui, "ui.", key);
    // 省略されていれば `fallback` を使う
    let optional = |key: &str, fallback: u32| match ui.raw_get::<Value>(key) {
        Ok(Value::Nil) => Ok(fallback),
        Ok(value) => color(&value, &format!("ui.{key}")),
        Err(err) => Err(format!("ui.{key}: {err}")),
    };
    let bar_background = ui_color("bar_background")?;
    let bar_border = ui_color("bar_border")?;
    let active_pane_border = ui_color("active_pane_border")?;
    let foreground = optional("foreground", terminal_colors.foreground)?;
    let raised_background = optional(
        "raised_background",
        mix(bar_background, foreground, RAISED_MIX),
    )?;
    let overlay_background = optional(
        "overlay_background",
        mix(bar_background, foreground, OVERLAY_MIX),
    )?;
    let ui_colors = UiColors {
        foreground,
        accent: optional("accent", active_pane_border)?,
        raised_background,
        overlay_background,
        strong_border: optional("strong_border", bar_border)?,
        bar_background,
        bar_border,
        inactive_foreground: ui_color("inactive_foreground")?,
        active_pane_border,
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
        0x1a1b1d, 0xc26a6a, 0x7aa886, 0xd1a257, 0x7f9ab8, 0xa58bb5, 0x7fa8a8, 0xc9c9c9, 0x5a5a5a,
        0xd68282, 0x8fbf9b, 0xe0b874, 0x98b0cc, 0xbba2c9, 0x96bfbf, 0xececec,
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
    fn dark_theme_uses_the_monochrome_palette() {
        let theme = Theme::default();
        assert_eq!(theme.name, "kugutsu-dark");
        assert_eq!(
            theme.terminal,
            TerminalColors {
                foreground: 0xc9c9c9,
                background: 0x0a0a0b,
                cursor: 0xb5b5b5,
                ansi: DARK_ANSI,
            }
        );
        assert_eq!(
            theme.ui,
            UiColors {
                bar_background: 0x060606,
                bar_border: 0x1a1b1d,
                inactive_foreground: 0x808080,
                active_pane_border: 0xb5b5b5,
                prefix_indicator: 0xececec,
                agent_working: 0xb5b5b5,
                agent_blocked: 0xd1a257,
                agent_done: 0x7aa886,
                error: 0xc26a6a,
                foreground: 0xc9c9c9,
                accent: 0xb5b5b5,
                raised_background: 0x111213,
                overlay_background: 0x18191b,
                strong_border: 0x222326,
            }
        );
    }

    #[test]
    fn light_theme_is_bundled() {
        let theme = builtin("kugutsu-light").unwrap();
        assert_eq!(theme.terminal.background, 0xfafafa);
        assert_eq!(theme.terminal.foreground, 0x2a2a2b);
        assert_eq!(theme.ui.bar_background, 0xf0f0f0);
        assert_eq!(theme.ui.overlay_background, 0xffffff);
        assert!(builtin("no-such-theme").is_none());
    }

    /// 任意のキーを除いた、以前の形式のテーマファイル。
    const LEGACY: &str = r##"return {
  terminal = {
    foreground = "#d4d4d4", background = "#1e1e1e", cursor = "#aeafad",
    ansi = {
      "#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5",
      "#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#ffffff",
    },
  },
  ui = {
    bar_background = "#000000", bar_border = "#2b2b2b", inactive_foreground = "#8a8a8a",
    active_pane_border = "#569cd6", prefix_indicator = "#dcdcaa", agent_working = "#569cd6",
    agent_blocked = "#e5c07b", agent_done = "#89d185", error = "#f14c4c",
  },
}"##;

    #[test]
    fn optional_ui_keys_fall_back_to_the_required_ones() {
        let theme = parse(LEGACY, "legacy").unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(theme.ui.foreground, 0xd4d4d4);
        assert_eq!(theme.ui.accent, 0x569cd6);
        assert_eq!(theme.ui.strong_border, 0x2b2b2b);
        // バーの背景を文字色の方へ少し寄せる
        assert_eq!(
            theme.ui.raised_background,
            mix(0x000000, 0xd4d4d4, RAISED_MIX)
        );
        assert_eq!(
            theme.ui.overlay_background,
            mix(0x000000, 0xd4d4d4, OVERLAY_MIX)
        );
        assert_ne!(theme.ui.raised_background, theme.ui.bar_background);
    }

    #[test]
    fn optional_ui_keys_override_the_fallbacks() {
        let source = LEGACY.replace(
            r##"error = "#f14c4c","##,
            r##"error = "#f14c4c", accent = "#123456", raised_background = "#010203","##,
        );
        let theme = parse(&source, "legacy").unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(theme.ui.accent, 0x123456);
        assert_eq!(theme.ui.raised_background, 0x010203);
        // 指定しなかったものはフォールバックのまま
        assert_eq!(theme.ui.strong_border, 0x2b2b2b);
    }

    #[test]
    fn invalid_optional_colors_are_errors() {
        parse_err(
            &dark_with("theme.ui.accent = 1"),
            "test: ui.accent: expected a color like \"#rrggbb\", got number",
        );
    }

    #[test]
    fn mix_blends_each_channel() {
        assert_eq!(mix(0x000000, 0xffffff, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xffffff, 1.0), 0xffffff);
        assert_eq!(mix(0x000000, 0xc8c8c8, 0.5), 0x646464);
        assert_eq!(mix(0x102030, 0x102030, 0.3), 0x102030);
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
