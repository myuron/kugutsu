//! `kugutsu lock` / `update` / `install` の中身。
//!
//! | コマンド | 読む | 書く |
//! | --- | --- | --- |
//! | `lock` | spec, lockfile | lockfile（変わった・増えたものだけ解決し、spec に無いものを消す） |
//! | `update` | spec, lockfile | lockfile（指定したものを最新のコミットに解決し直す） |
//! | `install` | lockfile | plugins ディレクトリ（lockfile のコミットに合わせる） |

use std::path::Path;

use super::git::Git;
use super::lockfile::{Entry, Lockfile};
use super::spec::{Source, Spec};

/// `lock` / `update` の結果。
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// 新しい lockfile。1 つでも解決できなければ `None`（lockfile は書かない）。
    pub lockfile: Option<Lockfile>,
    /// プラグインごとの結果（1 行ずつ）。
    pub lines: Vec<String>,
}

/// `install` の結果。
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    /// プラグインごとの結果（1 行ずつ）。
    pub lines: Vec<String>,
    /// すべて成功したか。
    pub ok: bool,
}

/// どのプラグインを最新のコミットに解決し直すか。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Update<'a> {
    /// `kugutsu lock`: spec が変わったものだけ解決する。
    None,
    /// `kugutsu update`: すべて解決し直す。
    All,
    /// `kugutsu update NAME...`: 指定したものを解決し直す。
    Only(&'a [String]),
}

/// spec から新しい lockfile を作る。
pub fn resolve(specs: &[Spec], current: &Lockfile, update: Update, git: &dyn Git) -> Resolved {
    let mut lockfile = Lockfile::default();
    let mut lines = Vec::new();
    let mut ok = true;
    for spec in specs {
        let Source::Git { url, rev } = &spec.source else {
            continue;
        };
        let name = &spec.name;
        let locked = current.plugins.get(name);
        let forced = match update {
            Update::None => false,
            Update::All => true,
            Update::Only(names) => names.contains(name),
        };
        let unchanged = locked.filter(|entry| entry.url == *url && entry.rev == *rev);
        let commit = match unchanged {
            Some(entry) if !forced => entry.commit.clone(),
            _ => match git.ls_remote(url, rev.as_deref()) {
                Ok(commit) => commit,
                Err(err) => {
                    lines.push(format!("{name}  error: {err}"));
                    ok = false;
                    continue;
                }
            },
        };
        lines.push(match locked {
            None => format!("{name}  added {}", short(&commit)),
            Some(entry) if entry.commit == commit => {
                format!("{name}  up to date {}", short(&commit))
            }
            Some(entry) => format!("{name}  {} -> {}", short(&entry.commit), short(&commit)),
        });
        lockfile
            .plugins
            .insert(name.clone(), entry(url, rev.as_ref(), commit));
    }
    if let Update::Only(names) = update {
        for name in names {
            let error = match specs.iter().find(|spec| spec.name == *name) {
                None => "not in the plugin spec",
                Some(Spec {
                    source: Source::Dir(_),
                    ..
                }) => "local plugins (dir) are not locked",
                Some(_) => continue,
            };
            lines.push(format!("{name}  error: {error}"));
            ok = false;
        }
    }
    // 解決に失敗したものは「消した」とは言わない
    let in_spec = |name: &String| {
        specs
            .iter()
            .any(|spec| spec.name == *name && matches!(spec.source, Source::Git { .. }))
    };
    for name in current.plugins.keys() {
        if !lockfile.plugins.contains_key(name) && !in_spec(name) {
            lines.push(format!("{name}  removed"));
        }
    }
    Resolved {
        lockfile: ok.then_some(lockfile),
        lines,
    }
}

/// plugins ディレクトリを lockfile に合わせる。
pub fn install(lockfile: &Lockfile, plugins_dir: &Path, git: &dyn Git) -> Installed {
    let mut lines = Vec::new();
    let mut ok = true;
    for (name, entry) in &lockfile.plugins {
        let line = match install_one(&plugins_dir.join(name), entry, git) {
            Ok(status) => status,
            Err(err) => {
                ok = false;
                format!("error: {err}")
            }
        };
        lines.push(format!("{name}  {line}"));
    }
    match remove_unlocked(lockfile, plugins_dir) {
        Ok(removed) => lines.extend(removed.iter().map(|name| format!("{name}  removed"))),
        Err(err) => {
            ok = false;
            lines.push(format!("error: {err}"));
        }
    }
    Installed { lines, ok }
}

/// 1 つのプラグインを lockfile のコミットに合わせ、表示する状態を返す。
fn install_one(dir: &Path, entry: &Entry, git: &dyn Git) -> Result<String, String> {
    let commit = &entry.commit;
    match std::fs::symlink_metadata(dir) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = dir.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("{}: {err}", parent.display()))?;
            }
            git.clone_repo(&entry.url, dir)?;
            if let Err(err) = git.checkout(dir, commit) {
                // 中途半端な clone を残すと、次の install で管理下のプラグインとして扱われる
                let _ = std::fs::remove_dir_all(dir);
                return Err(err);
            }
            Ok(format!("installed {}", short(commit)))
        }
        Err(err) => Err(format!("{}: {err}", dir.display())),
        Ok(_) if !is_managed(dir) => Ok("external, skipped".to_string()),
        Ok(_) => {
            let head = git.rev_parse(dir)?;
            if head == *commit {
                return Ok(format!("up to date {}", short(commit)));
            }
            git.fetch(dir, &entry.url)?;
            git.checkout(dir, commit)?;
            Ok(format!("{} -> {}", short(&head), short(commit)))
        }
    }
}

/// `install` が管理するディレクトリか。symlink や `.git` の無いディレクトリは外から置いたもの
/// （Nix store へのリンクなど）なので触らない。
fn is_managed(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir).is_ok_and(|meta| meta.is_dir()) && dir.join(".git").exists()
}

/// lockfile に無い管理下のプラグインを消し、消した名前を返す。
fn remove_unlocked(lockfile: &Lockfile, plugins_dir: &Path) -> Result<Vec<String>, String> {
    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{}: {err}", plugins_dir.display())),
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !lockfile.plugins.contains_key(name))
        .filter(|name| is_managed(&plugins_dir.join(name)))
        .collect();
    names.sort();
    for name in &names {
        let dir = plugins_dir.join(name);
        std::fs::remove_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    Ok(names)
}

/// 表示用の短いコミット。
fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

/// spec の git の部分を lockfile の項目にする。
fn entry(url: &str, rev: Option<&String>, commit: String) -> Entry {
    Entry {
        url: url.to_string(),
        rev: rev.cloned(),
        commit,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    const C1: &str = "1111111111111111111111111111111111111111";
    const C2: &str = "2222222222222222222222222222222222222222";
    const C3: &str = "3333333333333333333333333333333333333333";

    /// 呼び出しを記録する偽の git。clone したディレクトリの `.git/HEAD` にコミットを書く。
    #[derive(Default)]
    struct FakeGit {
        /// `(url, rev)` が指すコミット。
        refs: HashMap<(String, Option<String>), String>,
        /// 失敗させる URL。
        broken_urls: HashSet<String>,
        /// checkout に失敗するコミット。
        broken_commits: HashSet<String>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeGit {
        fn with_ref(mut self, url: &str, rev: Option<&str>, commit: &str) -> Self {
            self.refs
                .insert((url.into(), rev.map(Into::into)), commit.into());
            self
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }

        fn record(&self, call: String) {
            self.calls.borrow_mut().push(call);
        }

        fn check_url(&self, url: &str) -> Result<(), String> {
            if self.broken_urls.contains(url) {
                Err(format!("cannot reach {url}"))
            } else {
                Ok(())
            }
        }
    }

    impl Git for FakeGit {
        fn ls_remote(&self, url: &str, rev: Option<&str>) -> Result<String, String> {
            self.record(format!("ls-remote {url} {rev:?}"));
            self.check_url(url)?;
            self.refs
                .get(&(url.to_string(), rev.map(Into::into)))
                .cloned()
                .ok_or_else(|| "rev not found".to_string())
        }

        fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String> {
            self.record(format!("clone {url}"));
            self.check_url(url)?;
            fs::create_dir_all(dest.join(".git")).unwrap();
            fs::write(dest.join(".git/HEAD"), C3).unwrap();
            Ok(())
        }

        fn fetch(&self, dir: &Path, url: &str) -> Result<(), String> {
            self.record(format!(
                "fetch {} {url}",
                dir.file_name().unwrap().display()
            ));
            self.check_url(url)
        }

        fn checkout(&self, dir: &Path, commit: &str) -> Result<(), String> {
            let name = dir.file_name().unwrap().display();
            self.record(format!("checkout {name} {}", short(commit)));
            if self.broken_commits.contains(commit) {
                return Err(format!("no commit {commit}"));
            }
            fs::write(dir.join(".git/HEAD"), commit).unwrap();
            Ok(())
        }

        fn rev_parse(&self, dir: &Path) -> Result<String, String> {
            fs::read_to_string(dir.join(".git/HEAD")).map_err(|err| err.to_string())
        }
    }

    fn url(name: &str) -> String {
        format!("https://example.com/{name}.git")
    }

    fn git_spec(name: &str, rev: Option<&str>) -> Spec {
        Spec {
            name: name.into(),
            source: Source::Git {
                url: url(name),
                rev: rev.map(Into::into),
            },
        }
    }

    fn lockfile(entries: &[(&str, Option<&str>, &str)]) -> Lockfile {
        let mut lockfile = Lockfile::default();
        for (name, rev, commit) in entries {
            lockfile.plugins.insert(
                name.to_string(),
                Entry {
                    url: url(name),
                    rev: rev.map(Into::into),
                    commit: commit.to_string(),
                },
            );
        }
        lockfile
    }

    #[test]
    fn lock_adds_new_plugins_and_skips_dir_plugins() {
        let git =
            FakeGit::default()
                .with_ref(&url("a"), None, C1)
                .with_ref(&url("b"), Some("v1"), C2);
        let specs = [
            git_spec("a", None),
            git_spec("b", Some("v1")),
            Spec {
                name: "local".into(),
                source: Source::Dir("/src/local".into()),
            },
        ];
        let resolved = resolve(&specs, &Lockfile::default(), Update::None, &git);
        assert_eq!(
            resolved.lockfile,
            Some(lockfile(&[("a", None, C1), ("b", Some("v1"), C2)]))
        );
        assert_eq!(resolved.lines, ["a  added 1111111", "b  added 2222222"]);
    }

    #[test]
    fn lock_keeps_unchanged_entries_without_git() {
        let git = FakeGit::default();
        let current = lockfile(&[("a", Some("main"), C1)]);
        let resolved = resolve(&[git_spec("a", Some("main"))], &current, Update::None, &git);
        assert_eq!(resolved.lockfile, Some(current));
        assert_eq!(resolved.lines, ["a  up to date 1111111"]);
        assert_eq!(git.calls(), Vec::<String>::new());
    }

    #[test]
    fn lock_resolves_entries_whose_url_or_rev_changed() {
        let git = FakeGit::default()
            .with_ref(&url("a"), Some("v2"), C2)
            .with_ref(&url("b"), None, C3);
        let mut current = lockfile(&[("a", Some("v1"), C1), ("b", None, C1)]);
        current.plugins.get_mut("b").unwrap().url = "https://old.example.com/b.git".into();
        let resolved = resolve(
            &[git_spec("a", Some("v2")), git_spec("b", None)],
            &current,
            Update::None,
            &git,
        );
        assert_eq!(
            resolved.lockfile,
            Some(lockfile(&[("a", Some("v2"), C2), ("b", None, C3)]))
        );
        assert_eq!(
            resolved.lines,
            ["a  1111111 -> 2222222", "b  1111111 -> 3333333"]
        );
    }

    #[test]
    fn lock_removes_entries_not_in_the_spec() {
        let git = FakeGit::default();
        let current = lockfile(&[("a", None, C1), ("gone", None, C2)]);
        let resolved = resolve(&[git_spec("a", None)], &current, Update::None, &git);
        assert_eq!(resolved.lockfile, Some(lockfile(&[("a", None, C1)])));
        assert_eq!(resolved.lines, ["a  up to date 1111111", "gone  removed"]);
    }

    #[test]
    fn nothing_is_written_when_a_plugin_fails() {
        let mut git = FakeGit::default().with_ref(&url("b"), None, C2);
        git.broken_urls.insert(url("a"));
        let resolved = resolve(
            &[
                git_spec("a", None),
                git_spec("b", None),
                git_spec("c", None),
            ],
            &Lockfile::default(),
            Update::None,
            &git,
        );
        assert_eq!(resolved.lockfile, None);
        assert_eq!(
            resolved.lines,
            [
                format!("a  error: cannot reach {}", url("a")),
                "b  added 2222222".to_string(),
                "c  error: rev not found".to_string(),
            ]
        );
    }

    #[test]
    fn update_resolves_every_plugin_again() {
        let git =
            FakeGit::default()
                .with_ref(&url("a"), None, C2)
                .with_ref(&url("b"), Some("main"), C1);
        let current = lockfile(&[("a", None, C1), ("b", Some("main"), C1)]);
        let resolved = resolve(
            &[git_spec("a", None), git_spec("b", Some("main"))],
            &current,
            Update::All,
            &git,
        );
        assert_eq!(
            resolved.lockfile,
            Some(lockfile(&[("a", None, C2), ("b", Some("main"), C1)]))
        );
        assert_eq!(
            resolved.lines,
            ["a  1111111 -> 2222222", "b  up to date 1111111"]
        );
    }

    #[test]
    fn update_with_names_resolves_only_those() {
        let git = FakeGit::default().with_ref(&url("b"), None, C2);
        let current = lockfile(&[("a", None, C1), ("b", None, C1)]);
        let names = ["b".to_string()];
        let resolved = resolve(
            &[git_spec("a", None), git_spec("b", None)],
            &current,
            Update::Only(&names),
            &git,
        );
        assert_eq!(
            resolved.lockfile,
            Some(lockfile(&[("a", None, C1), ("b", None, C2)]))
        );
        assert_eq!(git.calls(), [format!("ls-remote {} None", url("b"))]);
    }

    #[test]
    fn update_of_unknown_or_local_plugins_is_an_error() {
        let git = FakeGit::default();
        let current = lockfile(&[("a", None, C1)]);
        let names = ["nope".to_string(), "local".to_string()];
        let specs = [
            git_spec("a", None),
            Spec {
                name: "local".into(),
                source: Source::Dir("/src/local".into()),
            },
        ];
        let resolved = resolve(&specs, &current, Update::Only(&names), &git);
        assert_eq!(resolved.lockfile, None);
        assert_eq!(
            resolved.lines,
            [
                "a  up to date 1111111",
                "nope  error: not in the plugin spec",
                "local  error: local plugins (dir) are not locked",
            ]
        );
    }

    /// テストごとの plugins ディレクトリ。
    fn plugins_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kugutsu-fetcher-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// `dir/name` を、HEAD が `commit` の管理下のプラグインとして作る。
    fn managed(dir: &Path, name: &str, commit: &str) {
        fs::create_dir_all(dir.join(name).join(".git")).unwrap();
        fs::write(dir.join(name).join(".git/HEAD"), commit).unwrap();
    }

    #[test]
    fn install_clones_missing_plugins_at_the_locked_commit() {
        let git = FakeGit::default();
        let dir = plugins_dir("clone");
        let installed = install(&lockfile(&[("a", None, C1)]), &dir, &git);
        assert!(installed.ok);
        assert_eq!(installed.lines, ["a  installed 1111111"]);
        assert_eq!(
            git.calls(),
            [format!("clone {}", url("a")), "checkout a 1111111".into()]
        );
        assert_eq!(git.rev_parse(&dir.join("a")), Ok(C1.to_string()));
    }

    #[test]
    fn install_checks_out_plugins_at_another_commit() {
        let git = FakeGit::default();
        let dir = plugins_dir("checkout");
        managed(&dir, "a", C1);
        managed(&dir, "b", C2);
        let installed = install(&lockfile(&[("a", None, C1), ("b", None, C3)]), &dir, &git);
        assert!(installed.ok);
        assert_eq!(
            installed.lines,
            ["a  up to date 1111111", "b  2222222 -> 3333333"]
        );
        assert_eq!(
            git.calls(),
            [format!("fetch b {}", url("b")), "checkout b 3333333".into()]
        );
    }

    #[test]
    fn install_skips_external_plugins() {
        let git = FakeGit::default();
        let dir = plugins_dir("external");
        fs::create_dir_all(dir.join("plain")).unwrap();
        let store = plugins_dir("external-store");
        fs::create_dir_all(&store).unwrap();
        std::os::unix::fs::symlink(&store, dir.join("linked")).unwrap();
        let installed = install(
            &lockfile(&[("plain", None, C1), ("linked", None, C1)]),
            &dir,
            &git,
        );
        assert!(installed.ok);
        assert_eq!(
            installed.lines,
            ["linked  external, skipped", "plain  external, skipped"]
        );
        assert_eq!(git.calls(), Vec::<String>::new());
    }

    #[test]
    fn install_removes_managed_plugins_not_in_the_lockfile() {
        let git = FakeGit::default();
        let dir = plugins_dir("remove");
        managed(&dir, "a", C1);
        managed(&dir, "old", C1);
        fs::create_dir_all(dir.join("external")).unwrap();
        fs::write(dir.join("notes.txt"), "keep").unwrap();
        let installed = install(&lockfile(&[("a", None, C1)]), &dir, &git);
        assert!(installed.ok);
        assert_eq!(installed.lines, ["a  up to date 1111111", "old  removed"]);
        assert!(!dir.join("old").exists());
        assert!(dir.join("external").exists());
        assert!(dir.join("notes.txt").exists());
    }

    #[test]
    fn install_continues_after_a_failure() {
        let mut git = FakeGit::default();
        git.broken_urls.insert(url("a"));
        git.broken_commits.insert(C2.to_string());
        let dir = plugins_dir("failure");
        let installed = install(
            &lockfile(&[("a", None, C1), ("b", None, C2), ("c", None, C1)]),
            &dir,
            &git,
        );
        assert!(!installed.ok);
        assert_eq!(
            installed.lines,
            [
                format!("a  error: cannot reach {}", url("a")),
                format!("b  error: no commit {C2}"),
                "c  installed 1111111".to_string(),
            ]
        );
        // checkout に失敗した clone は残さない
        assert!(!dir.join("b").exists());
    }

    #[test]
    fn install_with_an_empty_lockfile_and_no_directory_does_nothing() {
        let git = FakeGit::default();
        let installed = install(&Lockfile::default(), &plugins_dir("empty"), &git);
        assert_eq!(
            installed,
            Installed {
                lines: Vec::new(),
                ok: true
            }
        );
    }
}
