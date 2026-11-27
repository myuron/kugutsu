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
            read_patch(source, name)
                .and_then(|patch| complete(&patch, name))
                .unwrap_or_else(|err| panic!("bundled theme is invalid: {err}"))
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
/// テーマに無い色は既定のテーマの色にする（派生の色はそれから導出する）。
pub fn parse(source: &str, name: &str) -> Result<Theme, String> {
    Ok(build(&read_patch(source, name)?, name))
}

/// テーマファイルを評価して、そこに書かれた色を読む。
fn read_patch(source: &str, name: &str) -> Result<ThemePatch, String> {
    let lua = Lua::new();
    let value: Value = lua
        .load(source)
        .set_name(format!("={name}.lua"))
        .eval()
        .map_err(|err| config::error_message(&err))?;
    patch(&value).map_err(|err| format!("{name}: {err}"))
}

/// テーマの一部の色。書かれていない色は `None`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemePatch {
    pub terminal: TerminalPatch,
    pub ui: UiPatch,
}

/// [`TerminalColors`] の一部。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerminalPatch {
    pub foreground: Option<u32>,
    pub background: Option<u32>,
    pub cursor: Option<u32>,
    pub ansi: Option<[u32; 16]>,
}

/// [`UiColors`] の各色について、[`UiPatch`] とその読み書きを作る。
macro_rules! ui_patch {
    ($($key:ident),* $(,)?) => {
        /// [`UiColors`] の一部。
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct UiPatch {
            $(pub $key: Option<u32>,)*
        }

        const UI_KEYS: &[&str] = &[$(stringify!($key)),*];

        impl UiPatch {
            fn read(table: &Table) -> Result<Self, String> {
                Ok(Self {
                    $($key: optional_color(table, "ui.", stringify!($key))?,)*
                })
            }

            fn apply_to(&self, ui: &mut UiColors) {
                $(if let Some(color) = self.$key {
                    ui.$key = color;
                })*
            }

            /// `over` にある色で上書きした patch。
            fn merge(&self, over: &Self) -> Self {
                Self {
                    $($key: over.$key.or(self.$key),)*
                }
            }

            fn complete(&self) -> Result<UiColors, String> {
                Ok(UiColors {
                    $($key: self
                        .$key
                        .ok_or_else(|| format!("ui.{}: missing", stringify!($key)))?,)*
                })
            }
        }
    };
}

ui_patch!(
    bar_background,
    bar_border,
    inactive_foreground,
    active_pane_border,
    prefix_indicator,
    agent_working,
    agent_blocked,
    agent_done,
    error,
    foreground,
    accent,
    raised_background,
    overlay_background,
    strong_border,
);

impl UiPatch {
    /// 省略した派生の色（`foreground` など）を、ほかの色から導出して埋める。
    fn derive(&mut self, terminal_foreground: Option<u32>) {
        self.foreground = self.foreground.or(terminal_foreground);
        self.accent = self.accent.or(self.active_pane_border);
        self.strong_border = self.strong_border.or(self.bar_border);
        if let (Some(background), Some(foreground)) = (self.bar_background, self.foreground) {
            self.raised_background = self
                .raised_background
                .or(Some(mix(background, foreground, RAISED_MIX)));
            self.overlay_background =
                self.overlay_background
                    .or(Some(mix(background, foreground, OVERLAY_MIX)));
        }
    }

    /// 派生の色を消した patch（ほかのテーマの土台にするとき、派生の色は導出し直す）。
    fn without_derived(&self) -> Self {
        Self {
            foreground: None,
            accent: None,
            raised_background: None,
            overlay_background: None,
            strong_border: None,
            ..self.clone()
        }
    }
}

impl TerminalPatch {
    fn merge(&self, over: &Self) -> Self {
        Self {
            foreground: over.foreground.or(self.foreground),
            background: over.background.or(self.background),
            cursor: over.cursor.or(self.cursor),
            ansi: over.ansi.or(self.ansi),
        }
    }
}

const TERMINAL_KEYS: [&str; 4] = ["foreground", "background", "cursor", "ansi"];

/// 既定のテーマの色（派生の色を除く）。部分的なテーマの土台にする。
static DEFAULT_PATCH: LazyLock<ThemePatch> = LazyLock::new(|| {
    let source = BUNDLED
        .iter()
        .find(|(name, _)| *name == DEFAULT_THEME)
        .expect("default theme is bundled")
        .1;
    let patch = read_patch(source, DEFAULT_THEME)
        .unwrap_or_else(|err| panic!("bundled theme is invalid: {err}"));
    ThemePatch {
        terminal: patch.terminal,
        ui: patch.ui.without_derived(),
    }
});

/// テーマに書かれた色 `patch` からテーマを作る。無い色は既定のテーマの色にし、
/// 派生の色はそれから導出する。
fn build(patch: &ThemePatch, name: &str) -> Theme {
    let merged = ThemePatch {
        terminal: DEFAULT_PATCH.terminal.merge(&patch.terminal),
        ui: DEFAULT_PATCH.ui.merge(&patch.ui),
    };
    complete(&merged, name).expect("the default theme defines every required color")
}

impl Theme {
    /// `patch` にある色だけを上書きする。
    pub fn apply(&mut self, patch: &ThemePatch) {
        let terminal = &patch.terminal;
        let target = &mut self.terminal;
        for (slot, color) in [
            (&mut target.foreground, terminal.foreground),
            (&mut target.background, terminal.background),
            (&mut target.cursor, terminal.cursor),
        ] {
            if let Some(color) = color {
                *slot = color;
            }
        }
        if let Some(ansi) = terminal.ansi {
            target.ansi = ansi;
        }
        patch.ui.apply_to(&mut self.ui);
    }
}

/// 必須の色が揃った `patch` をテーマにする。派生の色は省略されていれば導出する。
fn complete(patch: &ThemePatch, name: &str) -> Result<Theme, String> {
    let missing = |key: &str| format!("{name}: terminal.{key}: missing");
    let terminal = &patch.terminal;
    let mut ui = patch.ui.clone();
    ui.derive(terminal.foreground);
    Ok(Theme {
        name: name.to_string(),
        terminal: TerminalColors {
            foreground: terminal.foreground.ok_or_else(|| missing("foreground"))?,
            background: terminal.background.ok_or_else(|| missing("background"))?,
            cursor: terminal.cursor.ok_or_else(|| missing("cursor"))?,
            ansi: terminal.ansi.ok_or_else(|| missing("ansi"))?,
        },
        ui: ui.complete().map_err(|err| format!("{name}: {err}"))?,
    })
}

/// テーマの table（`{ name = ..., terminal = { ... }, ui = { ... } }`）から色を読む。
/// どのキーも省略できる。`name` は文字列であることだけを確かめる。
pub fn patch(value: &Value) -> Result<ThemePatch, String> {
    let root = table(value)?;
    check_keys(&root, "", &["name", "terminal", "ui"])?;
    match optional(&root, "", "name")? {
        None | Some(Value::String(_)) => {}
        Some(other) => {
            return Err(format!(
                "name: expected string, got {}",
                config::lua_type_name(&other)
            ));
        }
    }

    let terminal = match optional(&root, "", "terminal")? {
        None => TerminalPatch::default(),
        Some(value) => {
            let terminal = table(&value).map_err(|r| format!("terminal: {r}"))?;
            check_keys(&terminal, "terminal.", &TERMINAL_KEYS)?;
            let color_of = |key| optional_color(&terminal, "terminal.", key);
            TerminalPatch {
                foreground: color_of("foreground")?,
                background: color_of("background")?,
                cursor: color_of("cursor")?,
                ansi: optional(&terminal, "terminal.", "ansi")?
                    .map(|value| ansi(&value))
                    .transpose()?,
            }
        }
    };

    let ui = match optional(&root, "", "ui")? {
        None => UiPatch::default(),
        Some(value) => {
            let ui = table(&value).map_err(|r| format!("ui: {r}"))?;
            check_keys(&ui, "ui.", UI_KEYS)?;
            UiPatch::read(&ui)?
        }
    };
    Ok(ThemePatch { terminal, ui })
}

/// `table[key]` の色。無ければ `None`。
fn optional_color(table: &Table, path: &str, key: &str) -> Result<Option<u32>, String> {
    optional(table, path, key)?
        .map(|value| color(&value, &format!("{path}{key}")))
        .transpose()
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

/// `table[key]`。無ければ `None`。`path` はエラーに出すキーの接頭辞（`"ui."` など）。
fn optional(table: &Table, path: &str, key: &str) -> Result<Option<Value>, String> {
    match table.raw_get::<Value>(key) {
        Ok(Value::Nil) => Ok(None),
        Ok(value) => Ok(Some(value)),
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
    fn missing_keys_fall_back_to_the_default_theme() {
        let light = BUNDLED[1].1.replacen("return {", "local theme = {", 1);
        let source =
            format!("{light}\ntheme.ui.agent_done = nil\ntheme.terminal = nil\nreturn theme");
        let theme = parse(&source, "partial").unwrap();
        let light = builtin("kugutsu-light").unwrap();
        let dark = Theme::default();
        assert_eq!(theme.name, "partial");
        assert_eq!(theme.terminal, dark.terminal);
        assert_eq!(theme.ui.agent_done, dark.ui.agent_done);
        assert_eq!(theme.ui.bar_background, light.ui.bar_background);

        let theme = parse("return {}", "empty").unwrap();
        assert_eq!(theme.name, "empty");
        assert_eq!(theme.terminal, dark.terminal);
        assert_eq!(theme.ui.bar_background, dark.ui.bar_background);
    }

    #[test]
    fn partial_themes_derive_the_optional_colors_from_their_own_colors() {
        // 既定のテーマの派生の色ではなく、補った後の色から導出する
        let theme = parse(
            r##"return { ui = { bar_background = "#000000", bar_border = "#111111" } }"##,
            "partial",
        )
        .unwrap();
        let dark = Theme::default();
        assert_eq!(theme.ui.strong_border, 0x111111);
        assert_eq!(theme.ui.foreground, dark.terminal.foreground);
        assert_eq!(theme.ui.accent, dark.ui.active_pane_border);
        assert_eq!(
            theme.ui.raised_background,
            mix(0x000000, dark.terminal.foreground, RAISED_MIX)
        );
        assert_eq!(
            theme.ui.overlay_background,
            mix(0x000000, dark.terminal.foreground, OVERLAY_MIX)
        );
    }

    #[test]
    fn a_theme_may_name_itself() {
        let theme = parse(r#"return { name = "other" }"#, "file").unwrap();
        // ファイルのテーマ名はファイル名で決まる
        assert_eq!(theme.name, "file");
        parse_err(
            "return { name = 1 }",
            "test: name: expected string, got number",
        );
    }

    #[test]
    fn apply_overrides_only_the_given_colors() {
        let lua = Lua::new();
        let value: Value = lua
            .load(r##"return { terminal = { cursor = "#010203" }, ui = { error = "#040506" } }"##)
            .eval()
            .unwrap();
        let patch = patch(&value).unwrap();
        let mut theme = builtin("kugutsu-light").unwrap().clone();
        let before = theme.clone();
        theme.apply(&patch);
        assert_eq!(theme.terminal.cursor, 0x010203);
        assert_eq!(theme.ui.error, 0x040506);
        assert_eq!(theme.terminal.background, before.terminal.background);
        assert_eq!(theme.ui.bar_background, before.ui.bar_background);
        assert_eq!(theme.name, "kugutsu-light");
    }

    #[test]
    fn bundled_themes_define_every_key() {
        let lua = Lua::new();
        for (name, source) in BUNDLED {
            let value: Value = lua.load(source).eval().unwrap();
            complete(&patch(&value).unwrap(), name).unwrap_or_else(|err| panic!("{err}"));
        }
        assert_eq!(
            complete(&ThemePatch::default(), "p").unwrap_err(),
            "p: terminal.foreground: missing"
        );
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
