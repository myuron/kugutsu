//! ユーザー設定（`init.lua`）。
//!
//! `init.lua` は起動時に一度だけ評価して [`Config`] に読み込む。Lua を使うのはこのモジュール
//! だけで、ほかのコードは [`Config`] だけを見る。

mod lua;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gpui::Keystroke;

use crate::tabs::prefix::DEFAULT_PREFIX;

pub use lua::evaluate;

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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: Keystroke::parse(DEFAULT_PREFIX).expect("default prefix is a valid keystroke"),
            font_family: None,
            font_size: 14.0,
            line_height: 1.3,
        }
    }
}

/// 読み込んだ設定と、読み込み中に起きたエラー。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Loaded {
    pub config: Config,
    pub errors: Vec<String>,
}

/// 設定ファイルの場所。`$XDG_CONFIG_HOME/kugutsu/init.lua`、無ければ
/// `$HOME/.config/kugutsu/init.lua`。どちらも無ければ `None`。
pub fn config_path(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let non_empty = |dir: Option<OsString>| dir.filter(|dir| !dir.is_empty()).map(PathBuf::from);
    let config_home =
        non_empty(xdg_config_home).or_else(|| non_empty(home).map(|home| home.join(".config")))?;
    Some(config_home.join("kugutsu").join(FILE_NAME))
}

/// 設定ファイルを読み込む。ファイルが無ければ既定値を返す。
pub fn load(path: &Path) -> Loaded {
    match std::fs::read_to_string(path) {
        Ok(source) => evaluate(&source, FILE_NAME),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Loaded::default(),
        Err(err) => Loaded {
            config: Config::default(),
            errors: vec![format!("{}: {err}", path.display())],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_prefers_xdg_config_home() {
        assert_eq!(
            config_path(Some("/xdg".into()), Some("/home/me".into())),
            Some(PathBuf::from("/xdg/kugutsu/init.lua"))
        );
    }

    #[test]
    fn config_path_falls_back_to_home() {
        assert_eq!(
            config_path(None, Some("/home/me".into())),
            Some(PathBuf::from("/home/me/.config/kugutsu/init.lua"))
        );
    }

    #[test]
    fn config_path_ignores_empty_xdg_config_home() {
        assert_eq!(
            config_path(Some("".into()), Some("/home/me".into())),
            Some(PathBuf::from("/home/me/.config/kugutsu/init.lua"))
        );
    }

    #[test]
    fn config_path_is_none_without_any_base() {
        assert_eq!(config_path(None, None), None);
        assert_eq!(config_path(Some("".into()), Some("".into())), None);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kugutsu-config-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_means_defaults_without_errors() {
        let path = temp_dir("missing").join("init.lua");
        assert_eq!(load(&path), Loaded::default());
    }

    #[test]
    fn loads_options_from_file() {
        let path = temp_dir("file").join("init.lua");
        std::fs::write(&path, "kugutsu.opt.font_size = 18\n").unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(loaded.config.font_size, 18.0);
    }

    #[test]
    fn unreadable_file_is_reported() {
        // ディレクトリはファイルとして読めない
        let path = temp_dir("unreadable");
        let loaded = load(&path);
        assert_eq!(loaded.config, Config::default());
        assert_eq!(loaded.errors.len(), 1);
    }
}
