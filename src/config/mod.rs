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
