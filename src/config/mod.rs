//! ユーザー設定（`init.lua`）。
//!
//! `init.lua` は起動時と `reload_config` のたびに新しい Lua state で評価して [`Config`] に
//! 読み込む。Lua を使うのはこのモジュールだけで、ほかのコードは [`Config`] だけを見る。

mod json;
mod lua;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gpui::{Keystroke, Rgba, WindowBackgroundAppearance, rgb};

use crate::plugins::paths::Paths;
use crate::plugins::spec::Spec;
use crate::tabs::prefix::{DEFAULT_PREFIX, Keymap};
use crate::theme::Theme;

#[cfg(test)]
pub use lua::evaluate;
pub use lua::{LuaCallback, evaluate_spec, evaluate_with};
pub(crate) use lua::{error_message, lua_type_name};

/// 設定ファイルの名前（エラー表示にも使う）。
pub const FILE_NAME: &str = "init.lua";

/// Kugutsu の設定。
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// prefix キー。
    pub prefix: Keystroke,
    /// ターミナルのフォント。`None` ならインストール済みの等幅フォントから選ぶ。
    pub font_family: Option<String>,
    /// ターミナルのフォントサイズ（px）。
    pub font_size: f32,
    /// フォントサイズに対する行の高さの比。
    pub line_height: f32,
    /// キーの割り当て。
    pub keymap: Keymap,
    /// カラーテーマ。
    pub theme: Theme,
    /// 背景の不透明度（0.0〜1.0）。ターミナルの既定の背景と UI のバーに掛ける。
    pub background_opacity: f32,
    /// 透過したウィンドウの後ろをぼかすようコンポジタに頼むか。
    pub background_blur: bool,
    /// UI（バー・サイドバーなど）のフォント。`None` ならインストール済みのフォントから選ぶ。
    pub ui_font_family: Option<String>,
    /// 分割時に非アクティブなペインに掛ける不透明度（0.0〜1.0、1.0 で薄くしない）。
    pub inactive_pane_opacity: f32,
    /// `kugutsu.ui` で設定する見た目。
    pub ui: UiConfig,
}

/// `kugutsu.ui` で設定する見た目。
#[derive(Debug, Clone, PartialEq)]
pub struct UiConfig {
    /// workspace ライン・タブバーの各項目の幅（px）。両方の段で揃える。
    pub item_width: f32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { item_width: 150.0 }
    }
}

impl Config {
    /// ウィンドウの背景の種類。不透明度が 1 なら blur を指定していても不透明にする。
    pub fn window_background(&self) -> WindowBackgroundAppearance {
        if self.background_opacity >= 1.0 {
            WindowBackgroundAppearance::Opaque
        } else if self.background_blur {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Transparent
        }
    }

    /// 背景色 `color` に背景の不透明度を掛けた色。
    pub fn translucent(&self, color: u32) -> Rgba {
        Rgba {
            a: self.background_opacity,
            ..rgb(color)
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: Keystroke::parse(DEFAULT_PREFIX).expect("default prefix is a valid keystroke"),
            font_family: None,
            font_size: 14.0,
            line_height: 1.3,
            keymap: Keymap::default(),
            theme: Theme::default(),
            background_opacity: 1.0,
            background_blur: false,
            ui_font_family: None,
            inactive_pane_opacity: 0.85,
            ui: UiConfig::default(),
        }
    }
}

/// 読み込んだ設定と、読み込み中に起きたエラー。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Loaded {
    pub config: Config,
    pub errors: Vec<String>,
    /// spec にあるが、まだインストールされていないプラグインの名前。
    pub missing_plugins: Vec<String>,
}

/// 設定ファイルのパスを直接指定する環境変数。
pub const CONFIG_ENV: &str = "KUGUTSU_CONFIG";

/// 設定ファイルの場所。`var` は環境変数を読む関数（`std::env::var_os` など）。
///
/// `$KUGUTSU_CONFIG`、`$XDG_CONFIG_HOME/kugutsu/init.lua`、`$HOME/.config/kugutsu/init.lua`
/// の順に、空でない環境変数から決める。どれも無ければ `None`。
pub fn config_path(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let non_empty = |name: &str| {
        var(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    if let Some(path) = non_empty(CONFIG_ENV) {
        return Some(path);
    }
    let config_home = non_empty("XDG_CONFIG_HOME")
        .or_else(|| non_empty("HOME").map(|home| home.join(".config")))?;
    Some(config_home.join("kugutsu").join(FILE_NAME))
}

/// 設定ファイルを読み込み、宣言したプラグインを読み込む。ファイルが無ければ既定値を返す。
pub fn load(paths: &Paths) -> Loaded {
    match read(&paths.init_lua) {
        Ok(source) => evaluate_with(&source, FILE_NAME, Some(paths)),
        Err(err) => Loaded {
            errors: vec![err],
            ..Loaded::default()
        },
    }
}

/// 設定ファイルからプラグインの宣言だけを読む（CLI 用）。`init.lua` にエラーがあれば `Err`。
pub fn load_spec(paths: &Paths) -> Result<Vec<Spec>, Vec<String>> {
    let source = read(&paths.init_lua).map_err(|err| vec![err])?;
    let (specs, errors) = evaluate_spec(&source, FILE_NAME, Some(paths));
    if errors.is_empty() {
        Ok(specs)
    } else {
        Err(errors)
    }
}

/// 設定ファイルの中身。ファイルが無ければ空。
fn read(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(source) => Ok(source),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `vars` だけが設定された環境で `config_path` を呼ぶ。
    fn path_with(vars: &[(&str, &str)]) -> Option<PathBuf> {
        config_path(|name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        })
    }

    #[test]
    fn config_path_prefers_kugutsu_config() {
        assert_eq!(
            path_with(&[
                ("KUGUTSU_CONFIG", "/repo/examples/init.lua"),
                ("XDG_CONFIG_HOME", "/xdg"),
                ("HOME", "/home/me"),
            ]),
            Some(PathBuf::from("/repo/examples/init.lua"))
        );
    }

    #[test]
    fn config_path_ignores_empty_kugutsu_config() {
        assert_eq!(
            path_with(&[("KUGUTSU_CONFIG", ""), ("XDG_CONFIG_HOME", "/xdg")]),
            Some(PathBuf::from("/xdg/kugutsu/init.lua"))
        );
    }

    #[test]
    fn config_path_uses_xdg_config_home() {
        assert_eq!(
            path_with(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/me")]),
            Some(PathBuf::from("/xdg/kugutsu/init.lua"))
        );
    }

    #[test]
    fn config_path_falls_back_to_home() {
        assert_eq!(
            path_with(&[("HOME", "/home/me")]),
            Some(PathBuf::from("/home/me/.config/kugutsu/init.lua"))
        );
    }

    #[test]
    fn config_path_ignores_empty_xdg_config_home() {
        assert_eq!(
            path_with(&[("XDG_CONFIG_HOME", ""), ("HOME", "/home/me")]),
            Some(PathBuf::from("/home/me/.config/kugutsu/init.lua"))
        );
    }

    #[test]
    fn config_path_is_none_without_any_base() {
        assert_eq!(path_with(&[]), None);
        assert_eq!(path_with(&[("XDG_CONFIG_HOME", ""), ("HOME", "")]), None);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kugutsu-config-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn window_background_follows_opacity_and_blur() {
        let with = |background_opacity, background_blur| Config {
            background_opacity,
            background_blur,
            ..Config::default()
        };
        use WindowBackgroundAppearance::{Blurred, Opaque, Transparent};
        assert_eq!(Config::default().window_background(), Opaque);
        assert_eq!(with(1.0, true).window_background(), Opaque);
        assert_eq!(with(0.85, false).window_background(), Transparent);
        assert_eq!(with(0.85, true).window_background(), Blurred);
    }

    #[test]
    fn translucent_applies_the_background_opacity() {
        let config = Config {
            background_opacity: 0.5,
            ..Config::default()
        };
        assert_eq!(
            config.translucent(0x336699),
            Rgba {
                a: 0.5,
                ..gpui::rgb(0x336699)
            }
        );
        assert_eq!(Config::default().translucent(0x336699), gpui::rgb(0x336699));
    }

    #[test]
    fn missing_file_means_defaults_without_errors() {
        let path = temp_dir("missing").join("init.lua");
        assert_eq!(
            load(&Paths::new(path.clone(), None, None)),
            Loaded::default()
        );
    }

    #[test]
    fn loads_options_from_file() {
        let path = temp_dir("file").join("init.lua");
        std::fs::write(&path, "kugutsu.opt.font_size = 18\n").unwrap();
        let loaded = load(&Paths::new(path.clone(), None, None));
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(loaded.config.font_size, 18.0);
    }

    #[test]
    fn unreadable_file_is_reported() {
        // ディレクトリはファイルとして読めない
        let path = temp_dir("unreadable");
        let loaded = load(&Paths::new(path.clone(), None, None));
        assert_eq!(loaded.config, Config::default());
        assert_eq!(loaded.errors.len(), 1);
    }
}
