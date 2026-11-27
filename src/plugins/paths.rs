//! 設定・lockfile・プラグインの置き場所。

use std::ffi::OsString;
use std::path::PathBuf;

use crate::config;

/// lockfile の名前。`init.lua` の隣に置く。
pub const LOCKFILE_NAME: &str = "kugutsu.lock";

/// Kugutsu が読み書きする場所。
#[derive(Debug, Clone, PartialEq)]
pub struct Paths {
    /// `init.lua` のパス。
    pub init_lua: PathBuf,
    /// `init.lua` のあるディレクトリ。プラグインと同じように `lua/` と `colors/` を読む。
    pub config_dir: PathBuf,
    /// `kugutsu.lock` のパス。
    pub lockfile: PathBuf,
    /// プラグインを clone するディレクトリ。データディレクトリが決まらなければ `None`。
    pub plugins_dir: Option<PathBuf>,
    /// ホームディレクトリ（`dir = "~/..."` の展開に使う）。
    pub home: Option<PathBuf>,
}

impl Paths {
    /// `init_lua` を設定ファイルとする場所。
    pub fn new(init_lua: PathBuf, plugins_dir: Option<PathBuf>, home: Option<PathBuf>) -> Self {
        let config_dir = init_lua.parent().map(PathBuf::from).unwrap_or_default();
        Self {
            lockfile: config_dir.join(LOCKFILE_NAME),
            config_dir,
            init_lua,
            plugins_dir,
            home,
        }
    }
}

/// 環境変数から場所を決める。`var` は環境変数を読む関数（`std::env::var_os` など）。
///
/// `init.lua` の場所は [`config::config_path`] と同じ。プラグインは
/// `$XDG_DATA_HOME/kugutsu/plugins`、無ければ `$HOME/.local/share/kugutsu/plugins` に置く。
/// `init.lua` の場所が決まらなければ `None`。
pub fn resolve(var: impl Fn(&str) -> Option<OsString>) -> Option<Paths> {
    let non_empty = |name: &str| {
        var(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let init_lua = config::config_path(&var)?;
    let home = non_empty("HOME");
    let data_home = non_empty("XDG_DATA_HOME")
        .or_else(|| home.as_ref().map(|home| home.join(".local").join("share")));
    let plugins_dir = data_home.map(|data| data.join("kugutsu").join("plugins"));
    Some(Paths::new(init_lua, plugins_dir, home))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_with(vars: &[(&str, &str)]) -> Option<Paths> {
        resolve(|name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        })
    }

    #[test]
    fn uses_xdg_directories() {
        let paths = resolve_with(&[
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_DATA_HOME", "/xdg/data"),
            ("HOME", "/home/me"),
        ])
        .unwrap();
        assert_eq!(
            paths,
            Paths {
                init_lua: "/xdg/config/kugutsu/init.lua".into(),
                config_dir: "/xdg/config/kugutsu".into(),
                lockfile: "/xdg/config/kugutsu/kugutsu.lock".into(),
                plugins_dir: Some("/xdg/data/kugutsu/plugins".into()),
                home: Some("/home/me".into()),
            }
        );
    }

    #[test]
    fn falls_back_to_home() {
        let paths = resolve_with(&[("HOME", "/home/me"), ("XDG_DATA_HOME", "")]).unwrap();
        assert_eq!(paths.config_dir, PathBuf::from("/home/me/.config/kugutsu"));
        assert_eq!(
            paths.plugins_dir,
            Some(PathBuf::from("/home/me/.local/share/kugutsu/plugins"))
        );
    }

    #[test]
    fn lockfile_lives_next_to_kugutsu_config() {
        let paths = resolve_with(&[
            ("KUGUTSU_CONFIG", "/repo/examples/init.lua"),
            ("HOME", "/home/me"),
        ])
        .unwrap();
        assert_eq!(paths.config_dir, PathBuf::from("/repo/examples"));
        assert_eq!(paths.lockfile, PathBuf::from("/repo/examples/kugutsu.lock"));
    }

    #[test]
    fn plugins_dir_is_none_without_a_data_directory() {
        let paths = resolve_with(&[("KUGUTSU_CONFIG", "/repo/init.lua")]).unwrap();
        assert_eq!(paths.plugins_dir, None);
        assert_eq!(paths.home, None);
    }

    #[test]
    fn none_without_a_config_location() {
        assert_eq!(resolve_with(&[]), None);
    }
}
