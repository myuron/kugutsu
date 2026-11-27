//! `kugutsu.lock`。各プラグインのコミットを固定する TOML ファイル（`Cargo.lock` に倣う）。
//!
//! ```toml
//! version = 1
//!
//! [plugins."catppuccin.kugutsu"]
//! url = "https://github.com/myuron/catppuccin.kugutsu.git"
//! rev = "v1.0.0"
//! commit = "def4567890abcdef1234567890abcdef12345678"
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// lockfile の形式のバージョン。
pub const VERSION: u32 = 1;

/// lockfile の中身。プラグインは名前順に並ぶ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lockfile {
    pub version: u32,
    #[serde(default)]
    pub plugins: BTreeMap<String, Entry>,
}

/// 1 つのプラグインの固定。`url` と `rev` はコミットを解決したときの spec。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    pub commit: String,
}

impl Default for Lockfile {
    fn default() -> Self {
        Self {
            version: VERSION,
            plugins: BTreeMap::new(),
        }
    }
}

impl Lockfile {
    /// TOML を読む。
    pub fn parse(text: &str) -> Result<Self, String> {
        let lockfile: Self = toml::from_str(text).map_err(|err| err.to_string())?;
        if lockfile.version != VERSION {
            return Err(format!("unsupported version {}", lockfile.version));
        }
        Ok(lockfile)
    }

    /// TOML に書き出す。
    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("lockfile is serializable")
    }
}

/// lockfile を読む。ファイルが無ければ `None`。
pub fn read(path: &Path) -> Result<Option<Lockfile>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Lockfile::parse(&text)
            .map(Some)
            .map_err(|err| format!("{}: {err}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// lockfile を書く。
pub fn write(path: &Path, lockfile: &Lockfile) -> Result<(), String> {
    std::fs::write(path, lockfile.to_toml())
        .map_err(|err| format!("cannot write {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"version = 1

[plugins."catppuccin.kugutsu"]
url = "https://github.com/catppuccin/catppuccin.kugutsu.git"
rev = "v1.0.0"
commit = "def4567890abcdef1234567890abcdef12345678"

[plugins."tokyonight.kugutsu"]
url = "https://github.com/folke/tokyonight.kugutsu.git"
commit = "abc1234567890abcdef1234567890abcdef12345"
"#;

    fn entry(url: &str, rev: Option<&str>, commit: &str) -> Entry {
        Entry {
            url: url.into(),
            rev: rev.map(Into::into),
            commit: commit.into(),
        }
    }

    #[test]
    fn parses_entries() {
        let lockfile = Lockfile::parse(SAMPLE).unwrap();
        assert_eq!(lockfile.version, 1);
        assert_eq!(
            lockfile.plugins["catppuccin.kugutsu"],
            entry(
                "https://github.com/catppuccin/catppuccin.kugutsu.git",
                Some("v1.0.0"),
                "def4567890abcdef1234567890abcdef12345678"
            )
        );
        assert_eq!(lockfile.plugins["tokyonight.kugutsu"].rev, None);
    }

    #[test]
    fn writes_entries_sorted_by_name_without_empty_rev() {
        let mut lockfile = Lockfile::default();
        lockfile.plugins.insert(
            "tokyonight.kugutsu".into(),
            entry(
                "https://github.com/folke/tokyonight.kugutsu.git",
                None,
                "abc1234567890abcdef1234567890abcdef12345",
            ),
        );
        lockfile.plugins.insert(
            "catppuccin.kugutsu".into(),
            entry(
                "https://github.com/catppuccin/catppuccin.kugutsu.git",
                Some("v1.0.0"),
                "def4567890abcdef1234567890abcdef12345678",
            ),
        );
        assert_eq!(lockfile.to_toml(), SAMPLE);
        assert_eq!(Lockfile::parse(&lockfile.to_toml()).unwrap(), lockfile);
    }

    #[test]
    fn empty_lockfile_round_trips() {
        let lockfile = Lockfile::default();
        assert_eq!(Lockfile::parse(&lockfile.to_toml()).unwrap(), lockfile);
        assert_eq!(Lockfile::parse("version = 1\n").unwrap(), lockfile);
    }

    #[test]
    fn rejects_other_versions_and_invalid_toml() {
        let err = Lockfile::parse("version = 2\n").unwrap_err();
        assert!(err.contains("unsupported version 2"), "{err}");
        assert!(Lockfile::parse("version = ").is_err());
        assert!(Lockfile::parse("version = 1\n[plugins.a]\nurl = \"x\"\n").is_err());
    }

    #[test]
    fn reads_and_writes_files() {
        let dir =
            std::env::temp_dir().join(format!("kugutsu-lockfile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kugutsu.lock");
        let _ = std::fs::remove_file(&path);
        assert_eq!(read(&path), Ok(None));
        let lockfile = Lockfile::parse(SAMPLE).unwrap();
        write(&path, &lockfile).unwrap();
        assert_eq!(read(&path), Ok(Some(lockfile)));
        assert!(
            write(
                &dir.join("missing").join("kugutsu.lock"),
                &Lockfile::default()
            )
            .is_err()
        );
    }
}
