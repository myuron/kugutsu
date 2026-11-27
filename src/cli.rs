//! コマンドライン。引数が無ければ GUI を起動し、サブコマンドでプラグインを管理する。
//!
//! lockfile と plugins ディレクトリを変えるのはこのサブコマンドだけ（GUI は読むだけ）。

use std::io::Write;

use clap::{Parser, Subcommand};

use crate::config;
use crate::plugins::fetcher::{self, Update};
use crate::plugins::git::Git;
use crate::plugins::lockfile::{self, Lockfile};
use crate::plugins::paths::Paths;

/// Kugutsu の引数。
#[derive(Debug, PartialEq, Parser)]
#[command(name = "kugutsu", version, about = "Agentic Development Environment")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// プラグインを管理するサブコマンド。
#[derive(Debug, PartialEq, Subcommand)]
pub enum Command {
    /// Resolve the plugin spec in init.lua into kugutsu.lock
    Lock,
    /// Move plugins in kugutsu.lock to their latest commits (all if no NAME is given)
    Update {
        /// Plugins to update
        names: Vec<String>,
    },
    /// Clone and check out the plugins locked in kugutsu.lock
    Install,
}

/// サブコマンドを実行する。結果は `out` に、エラーは `err` に書く。成功したら `true`。
pub fn run(
    command: &Command,
    paths: &Paths,
    git: &dyn Git,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> bool {
    let result = match command {
        Command::Lock => lock(paths, Update::None, git, out),
        Command::Update { names } if names.is_empty() => lock(paths, Update::All, git, out),
        Command::Update { names } => lock(paths, Update::Only(names), git, out),
        Command::Install => install(paths, git, out),
    };
    match result {
        Ok(ok) => ok,
        Err(errors) => {
            for error in errors {
                let _ = writeln!(err, "{error}");
            }
            false
        }
    }
}

/// `lock` / `update`。すべて解決できたときだけ lockfile を書く。
fn lock(
    paths: &Paths,
    update: Update,
    git: &dyn Git,
    out: &mut dyn Write,
) -> Result<bool, Vec<String>> {
    let specs = config::load_spec(paths)?;
    let current = lockfile::read(&paths.lockfile).map_err(|err| vec![err])?;
    let resolved = fetcher::resolve(
        &specs,
        current.as_ref().unwrap_or(&Lockfile::default()),
        update,
        git,
    );
    print_lines(out, &resolved.lines);
    let Some(lockfile) = resolved.lockfile else {
        return Ok(false);
    };
    if current.as_ref() != Some(&lockfile) {
        lockfile::write(&paths.lockfile, &lockfile).map_err(|err| vec![err])?;
    }
    Ok(true)
}

/// `install`。
fn install(paths: &Paths, git: &dyn Git, out: &mut dyn Write) -> Result<bool, Vec<String>> {
    let plugins_dir = paths.plugins_dir.as_ref().ok_or_else(|| {
        vec!["cannot find the data directory: set XDG_DATA_HOME or HOME".to_string()]
    })?;
    let lockfile = lockfile::read(&paths.lockfile)
        .map_err(|err| vec![err])?
        .ok_or_else(|| {
            vec![format!(
                "{} not found: run `kugutsu lock` first",
                paths.lockfile.display()
            )]
        })?;
    let installed = fetcher::install(&lockfile, plugins_dir, git);
    print_lines(out, &installed.lines);
    Ok(installed.ok)
}

fn print_lines(out: &mut dyn Write, lines: &[String]) {
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::plugins::git::GitCli;
    use crate::plugins::git::tests::Remote;

    #[test]
    fn parses_subcommands() {
        let parse = |args: &[&str]| Cli::try_parse_from(args).map(|cli| cli.command);
        assert_eq!(parse(&["kugutsu"]).unwrap(), None);
        assert_eq!(parse(&["kugutsu", "lock"]).unwrap(), Some(Command::Lock));
        assert_eq!(
            parse(&["kugutsu", "install"]).unwrap(),
            Some(Command::Install)
        );
        assert_eq!(
            parse(&["kugutsu", "update"]).unwrap(),
            Some(Command::Update { names: Vec::new() })
        );
        assert_eq!(
            parse(&["kugutsu", "update", "a", "b"]).unwrap(),
            Some(Command::Update {
                names: vec!["a".into(), "b".into()]
            })
        );
        assert!(parse(&["kugutsu", "nope"]).is_err());
        assert!(parse(&["kugutsu", "lock", "extra"]).is_err());
    }

    /// 一時ディレクトリの設定と plugins ディレクトリ、プラグインの bare リポジトリ。
    struct Env {
        remote: Remote,
        paths: Paths,
    }

    impl Env {
        fn new(name: &str) -> Self {
            let remote = Remote::new(&format!("cli-{name}"));
            let root = remote.root.clone();
            fs::create_dir_all(root.join("config")).unwrap();
            let paths = Paths::new(
                root.join("config/init.lua"),
                Some(root.join("data/plugins")),
                Some(root.clone()),
            );
            Self { remote, paths }
        }

        fn write_init_lua(&self, source: &str) {
            fs::write(&self.paths.init_lua, source).unwrap();
        }

        /// プラグイン `p` を宣言する init.lua。
        fn declare(&self, rev: Option<&str>) {
            let rev = rev
                .map(|rev| format!(", rev = {rev:?}"))
                .unwrap_or_default();
            self.write_init_lua(&format!(
                "kugutsu.plugins.setup({{ {{ url = {:?}, name = \"p\"{rev} }} }})",
                self.remote.url()
            ));
        }

        /// サブコマンドを実行し、(成否, 標準出力, 標準エラー) を返す。
        fn run(&self, command: Command) -> (bool, String, String) {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let ok = run(&command, &self.paths, &GitCli, &mut out, &mut err);
            (
                ok,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        }

        fn lockfile(&self) -> Option<Lockfile> {
            lockfile::read(&self.paths.lockfile).unwrap()
        }

        fn plugin_dir(&self) -> PathBuf {
            self.paths.plugins_dir.as_ref().unwrap().join("p")
        }
    }

    fn head(dir: &std::path::Path) -> String {
        GitCli.rev_parse(dir).unwrap()
    }

    #[test]
    fn lock_install_and_update_follow_the_remote() {
        let env = Env::new("flow");
        let first = env.remote.commit("plugin v1");
        env.declare(None);

        // lock は lockfile だけを書き、clone しない
        let (ok, out, err) = env.run(Command::Lock);
        assert!(ok, "{err}");
        assert_eq!(out, format!("p  added {}\n", &first[..7]));
        assert_eq!(env.lockfile().unwrap().plugins["p"].commit, first);
        assert!(!env.plugin_dir().exists());

        let (ok, out, _) = env.run(Command::Install);
        assert!(ok);
        assert_eq!(out, format!("p  installed {}\n", &first[..7]));
        assert_eq!(head(&env.plugin_dir()), first);

        // 新しいコミットがあっても lock は動かさない
        let second = env.remote.commit("plugin v2");
        let (ok, out, _) = env.run(Command::Lock);
        assert!(ok);
        assert_eq!(out, format!("p  up to date {}\n", &first[..7]));

        let (ok, out, _) = env.run(Command::Update { names: Vec::new() });
        assert!(ok);
        assert_eq!(out, format!("p  {} -> {}\n", &first[..7], &second[..7]));
        assert_eq!(env.lockfile().unwrap().plugins["p"].commit, second);
        // install するまで plugins ディレクトリは変わらない
        assert_eq!(head(&env.plugin_dir()), first);

        let (ok, _, _) = env.run(Command::Install);
        assert!(ok);
        assert_eq!(head(&env.plugin_dir()), second);

        // spec から消すと lock が lockfile から消し、install がディレクトリを消す
        env.write_init_lua("");
        let (ok, out, _) = env.run(Command::Lock);
        assert!(ok);
        assert_eq!(out, "p  removed\n");
        let (ok, out, _) = env.run(Command::Install);
        assert!(ok);
        assert_eq!(out, "p  removed\n");
        assert!(!env.plugin_dir().exists());
    }

    #[test]
    fn failures_leave_the_lockfile_unchanged() {
        let env = Env::new("failure");
        env.declare(None);
        assert!(env.run(Command::Lock).0);
        let before = fs::read_to_string(&env.paths.lockfile).unwrap();

        env.declare(Some("no-such-tag"));
        let (ok, out, _) = env.run(Command::Lock);
        assert!(!ok);
        assert_eq!(out, "p  error: rev 'no-such-tag' not found\n");
        assert_eq!(fs::read_to_string(&env.paths.lockfile).unwrap(), before);
    }

    #[test]
    fn init_lua_errors_are_printed() {
        let env = Env::new("init-error");
        env.write_init_lua("kugutsu.plugins.setup({ 'nope' })");
        let (ok, out, err) = env.run(Command::Lock);
        assert!(!ok);
        assert_eq!(out, "");
        assert_eq!(
            err,
            "init.lua:1: kugutsu.plugins.setup: expected \"owner/repo\", got \"nope\"\n"
        );
        assert_eq!(env.lockfile(), None);
    }

    #[test]
    fn install_needs_a_lockfile() {
        let env = Env::new("no-lockfile");
        let (ok, _, err) = env.run(Command::Install);
        assert!(!ok);
        assert!(err.contains("run `kugutsu lock` first"), "{err}");
    }

    #[test]
    fn unwritable_lockfile_is_an_error() {
        let env = Env::new("read-only");
        env.declare(None);
        // lockfile の場所にディレクトリがあると書けない
        fs::create_dir_all(&env.paths.lockfile).unwrap();
        let (ok, _, err) = env.run(Command::Update { names: Vec::new() });
        assert!(!ok);
        assert!(err.contains("kugutsu.lock"), "{err}");
    }

    #[test]
    fn install_without_a_data_directory_is_an_error() {
        let mut env = Env::new("no-data-dir");
        env.paths.plugins_dir = None;
        let (ok, _, err) = env.run(Command::Install);
        assert!(!ok);
        assert!(err.contains("XDG_DATA_HOME"), "{err}");
    }
}
