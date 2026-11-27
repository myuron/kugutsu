//! プラグインの取得に使う git の操作。
//!
//! libgit2 を避けて `git` コマンドを呼ぶ（ユーザーの `git` と `~/.gitconfig` が使われる）。
//! `lock` / `update` / `install` の判断はテストで偽物に差し替えられるよう [`Git`] 越しに使う。

use std::path::Path;
use std::process::{Command, Stdio};

/// fetcher が使う git の操作。
pub trait Git {
    /// `url` の `rev`（tag / branch / コミット、`None` なら既定のブランチ）が指すコミット。
    fn ls_remote(&self, url: &str, rev: Option<&str>) -> Result<String, String>;
    /// `url` を `dest` に clone する。
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String>;
    /// `dir` に `url` のブランチとタグを取ってくる。
    fn fetch(&self, dir: &Path, url: &str) -> Result<(), String>;
    /// `dir` を `commit` に切り替える（detached HEAD）。
    fn checkout(&self, dir: &Path, commit: &str) -> Result<(), String>;
    /// `dir` の HEAD のコミット。
    fn rev_parse(&self, dir: &Path) -> Result<String, String>;
}

/// `git` コマンドを呼ぶ [`Git`]。
#[derive(Debug, Clone, Default)]
pub struct GitCli;

impl GitCli {
    /// `git args...` を実行して標準出力を返す。
    fn run(&self, dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
        let subcommand = args.first().copied().unwrap_or_default();
        let mut command = Command::new("git");
        if let Some(dir) = dir {
            command.arg("-C").arg(dir);
        }
        let output = command
            .args(args)
            // 認証のプロンプトで止まらないようにする
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output()
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => "git is not installed".to_string(),
                _ => format!("git {subcommand}: {err}"),
            })?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .unwrap_or("failed");
        Err(format!("git {subcommand}: {message}"))
    }
}

impl Git for GitCli {
    fn ls_remote(&self, url: &str, rev: Option<&str>) -> Result<String, String> {
        if let Some(commit) = rev.filter(|rev| is_commit(rev)) {
            return Ok(commit.to_ascii_lowercase());
        }
        let output = self.run(None, &["ls-remote", "--", url])?;
        find_ref(&output, rev).ok_or_else(|| match rev {
            Some(rev) => format!("rev '{rev}' not found"),
            None => "no default branch".to_string(),
        })
    }

    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String> {
        let dest = dest.to_string_lossy();
        self.run(None, &["clone", "--quiet", "--", url, &dest])
            .map(drop)
    }

    fn fetch(&self, dir: &Path, url: &str) -> Result<(), String> {
        let refspec = "+refs/heads/*:refs/remotes/origin/*";
        self.run(
            Some(dir),
            &["fetch", "--quiet", "--tags", "--", url, refspec],
        )
        .map(drop)
    }

    fn checkout(&self, dir: &Path, commit: &str) -> Result<(), String> {
        self.run(Some(dir), &["checkout", "--quiet", "--detach", commit])
            .map(drop)
    }

    fn rev_parse(&self, dir: &Path) -> Result<String, String> {
        self.run(Some(dir), &["rev-parse", "HEAD"])
            .map(|output| output.trim().to_string())
    }
}

/// 40 桁の 16 進数（コミットの SHA-1）か。
pub fn is_commit(text: &str) -> bool {
    text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// `git ls-remote` の出力から `rev` のコミットを探す。annotated tag は指すコミットにする。
fn find_ref(output: &str, rev: Option<&str>) -> Option<String> {
    let lookup = |name: &str| {
        output.lines().find_map(|line| {
            let (commit, reference) = line.split_once('\t')?;
            (reference == name).then(|| commit.to_string())
        })
    };
    match rev {
        None => lookup("HEAD"),
        Some(rev) => [
            format!("refs/tags/{rev}^{{}}"),
            format!("refs/tags/{rev}"),
            format!("refs/heads/{rev}"),
        ]
        .iter()
        .find_map(|name| lookup(name)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    const LS_REMOTE: &str = "\
1111111111111111111111111111111111111111\tHEAD
1111111111111111111111111111111111111111\trefs/heads/main
2222222222222222222222222222222222222222\trefs/heads/dev
3333333333333333333333333333333333333333\trefs/tags/v1
4444444444444444444444444444444444444444\trefs/tags/v2
5555555555555555555555555555555555555555\trefs/tags/v2^{}
";

    #[test]
    fn finds_refs_in_ls_remote_output() {
        let find = |rev| find_ref(LS_REMOTE, rev);
        assert_eq!(
            find(None).as_deref(),
            Some("1111111111111111111111111111111111111111")
        );
        assert_eq!(
            find(Some("dev")).as_deref(),
            Some("2222222222222222222222222222222222222222")
        );
        assert_eq!(
            find(Some("v1")).as_deref(),
            Some("3333333333333333333333333333333333333333")
        );
        // annotated tag は tag オブジェクトではなくコミットを指す
        assert_eq!(
            find(Some("v2")).as_deref(),
            Some("5555555555555555555555555555555555555555")
        );
        assert_eq!(find(Some("nope")), None);
        assert_eq!(find(Some("refs/heads/main")), None);
        assert_eq!(find_ref("", None), None);
    }

    #[test]
    fn recognizes_full_commits() {
        assert!(is_commit("abcdef1234567890abcdef1234567890abcdef12"));
        assert!(!is_commit("abcdef1"));
        assert!(!is_commit("ghijkl1234567890abcdef1234567890abcdef12"));
    }

    /// 本物の git でテストするための、一時ディレクトリの bare リポジトリ。
    pub(crate) struct Remote {
        pub root: PathBuf,
        work: PathBuf,
    }

    /// テスト用に `git` を実行する（ユーザーの設定は読まない）。
    pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
            .args([
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    impl Remote {
        pub(crate) fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("kugutsu-git-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let work = root.join("work");
            fs::create_dir_all(&work).unwrap();
            git(&work, &["init", "--quiet"]);
            let remote = Self { root, work };
            remote.commit("first");
            git(
                &remote.root,
                &["clone", "--quiet", "--bare", "work", "remote.git"],
            );
            remote
        }

        /// `file://` の URL。
        pub(crate) fn url(&self) -> String {
            format!("file://{}", self.root.join("remote.git").display())
        }

        /// コミットして bare リポジトリに push し、コミットを返す。
        pub(crate) fn commit(&self, message: &str) -> String {
            fs::write(self.work.join("file.txt"), message).unwrap();
            git(&self.work, &["add", "file.txt"]);
            git(&self.work, &["commit", "--quiet", "-m", message]);
            if self.root.join("remote.git").exists() {
                git(
                    &self.work,
                    &["push", "--quiet", "--tags", &self.url(), "HEAD:main"],
                );
            }
            git(&self.work, &["rev-parse", "HEAD"])
        }

        /// `name` のタグを付けて push する。`annotated` なら annotated tag。
        pub(crate) fn tag(&self, name: &str, annotated: bool) {
            if annotated {
                git(&self.work, &["tag", "-a", "-m", name, name]);
            } else {
                git(&self.work, &["tag", name]);
            }
            git(&self.work, &["push", "--quiet", &self.url(), name]);
        }

        /// `branch` を今のコミットで作って push する。
        pub(crate) fn branch(&self, branch: &str) {
            git(
                &self.work,
                &["push", "--quiet", &self.url(), &format!("HEAD:{branch}")],
            );
        }
    }

    #[test]
    fn ls_remote_resolves_branches_and_tags() {
        let remote = Remote::new("ls-remote");
        let first = git(&remote.work, &["rev-parse", "HEAD"]);
        remote.tag("v1", false);
        remote.tag("v2", true);
        remote.branch("stable");
        let second = remote.commit("second");
        let git = GitCli;
        let url = remote.url();
        assert_eq!(git.ls_remote(&url, None), Ok(second.clone()));
        assert_eq!(git.ls_remote(&url, Some("main")), Ok(second));
        assert_eq!(git.ls_remote(&url, Some("stable")), Ok(first.clone()));
        assert_eq!(git.ls_remote(&url, Some("v1")), Ok(first.clone()));
        assert_eq!(git.ls_remote(&url, Some("v2")), Ok(first.clone()));
        // 完全なコミットはそのまま使う
        assert_eq!(git.ls_remote(&url, Some(&first)), Ok(first));
        let err = git.ls_remote(&url, Some("nope")).unwrap_err();
        assert!(err.contains("'nope' not found"), "{err}");
        let missing = format!("file://{}", remote.root.join("missing.git").display());
        assert!(git.ls_remote(&missing, None).is_err());
    }

    #[test]
    fn clones_fetches_and_checks_out_commits() {
        let remote = Remote::new("clone");
        let first = git(&remote.work, &["rev-parse", "HEAD"]);
        let second = remote.commit("second");
        let git_cli = GitCli;
        let dest = remote.root.join("plugins").join("plugin");
        git_cli.clone_repo(&remote.url(), &dest).unwrap();
        assert_eq!(git_cli.rev_parse(&dest), Ok(second.clone()));
        git_cli.checkout(&dest, &first).unwrap();
        assert_eq!(git_cli.rev_parse(&dest), Ok(first));
        assert_eq!(fs::read_to_string(dest.join("file.txt")).unwrap(), "first");

        let third = remote.commit("third");
        assert!(git_cli.checkout(&dest, &third).is_err());
        git_cli.fetch(&dest, &remote.url()).unwrap();
        git_cli.checkout(&dest, &third).unwrap();
        assert_eq!(git_cli.rev_parse(&dest), Ok(third));
    }

    #[test]
    fn failures_are_reported() {
        let remote = Remote::new("failures");
        let git_cli = GitCli;
        let missing = format!("file://{}", remote.root.join("missing.git").display());
        let err = git_cli
            .clone_repo(&missing, &remote.root.join("dest"))
            .unwrap_err();
        assert!(err.starts_with("git clone: "), "{err}");
        assert!(git_cli.rev_parse(&remote.root.join("nowhere")).is_err());
    }
}
