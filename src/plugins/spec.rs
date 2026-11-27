//! `kugutsu.plugins.setup` に渡すプラグインの宣言（spec）。
//!
//! ```lua
//! kugutsu.plugins.setup({
//!   "catppuccin/catppuccin.kugutsu",                 -- GitHub の短縮形
//!   { "folke/tokyonight.kugutsu", rev = "v1.0.0" },  -- tag / branch / commit
//!   { url = "https://git.example.com/me/foo.kugutsu.git" },
//!   { dir = "~/src/my-plugin.kugutsu" },             -- 開発中のローカルプラグイン
//! })
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mlua::{Function, Table, Value};

use crate::config::lua_type_name;

/// プラグインの取得元。
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// git リポジトリ。`plugins/<name>/` に clone し、lockfile で固定する。
    Git { url: String, rev: Option<String> },
    /// ローカルのディレクトリ。いつも読み込み、lockfile には載せない。
    Dir(PathBuf),
}

/// プラグインの宣言のうち、Lua に依存しない部分（CLI も使う）。
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    /// `plugins/` の下のディレクトリ名、lockfile のキー、`kugutsu update NAME` の名前。
    pub name: String,
    pub source: Source,
}

/// 読み込み時に使う Lua の値も含めたプラグインの宣言。
#[derive(Debug, Clone)]
pub struct PluginSpec {
    pub spec: Spec,
    /// `require(main).setup(opts)` に渡す table。
    pub opts: Option<Table>,
    /// `setup` の代わりに `config(opts)` を呼ぶ関数。
    pub config: Option<Function>,
    /// `setup` を呼ぶモジュール。
    pub main: String,
}

/// パスの展開に使う場所。
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// `~` の展開先。
    pub home: Option<PathBuf>,
    /// 相対パスの基準（`init.lua` のあるディレクトリ）。
    pub config_dir: Option<PathBuf>,
}

/// エントリの table で使えるキー。
const ENTRY_KEYS: [&str; 7] = ["url", "dir", "rev", "name", "opts", "config", "main"];

/// `kugutsu.plugins.setup` の引数を検証する。入れ子のリストは平坦にする。
pub fn parse(value: &Value, context: &Context) -> Result<Vec<PluginSpec>, String> {
    let list = match value {
        Value::Table(table) => table,
        other => return Err(format!("expected a table, got {}", lua_type_name(other))),
    };
    let mut plugins = Vec::new();
    collect(list, context, &mut plugins)?;
    let mut names = HashSet::new();
    for plugin in &plugins {
        if !names.insert(plugin.spec.name.as_str()) {
            return Err(format!(
                "duplicate plugin name '{}' (use `name` to rename one)",
                plugin.spec.name
            ));
        }
    }
    Ok(plugins)
}

/// `value` がエントリなら 1 つ、リストなら中身を順に `plugins` に積む。
fn collect(table: &Table, context: &Context, plugins: &mut Vec<PluginSpec>) -> Result<(), String> {
    if is_entry(table)? {
        plugins.push(entry(table, context)?);
        return Ok(());
    }
    let len = table.raw_len();
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|err| err.to_string())?;
        let in_sequence = matches!(key, Value::Integer(n) if n >= 1 && n as usize <= len);
        if !in_sequence {
            return Err(format!(
                "expected a list of plugins, got key {}",
                key_name(&key)
            ));
        }
    }
    for index in 1..=len {
        match table
            .raw_get::<Value>(index)
            .map_err(|err| err.to_string())?
        {
            Value::String(text) => plugins.push(shorthand_entry(&text.to_string_lossy())?),
            Value::Table(inner) => collect(&inner, context, plugins)?,
            other => {
                return Err(format!(
                    "expected a string or a table, got {}",
                    lua_type_name(&other)
                ));
            }
        }
    }
    Ok(())
}

/// table が 1 つのプラグインを表すか（`url` / `dir` がある、または `{ "owner/repo", ... }`）。
/// `{ "a/b", "c/d" }` は 2 つのプラグインのリスト。
fn is_entry(table: &Table) -> Result<bool, String> {
    if !raw(table, "url")?.is_nil() || !raw(table, "dir")?.is_nil() {
        return Ok(true);
    }
    Ok(matches!(raw(table, 1)?, Value::String(_)) && raw(table, 2)?.is_nil())
}

fn raw(table: &Table, key: impl mlua::IntoLua) -> Result<Value, String> {
    table.raw_get(key).map_err(|err| err.to_string())
}

/// `"owner/repo"`。
fn shorthand_entry(text: &str) -> Result<PluginSpec, String> {
    let url = github_url(text)?;
    plugin(
        url_name(&url),
        Source::Git { url, rev: None },
        None,
        None,
        None,
    )
}

/// `"owner/repo"` を GitHub の URL にする。
fn github_url(text: &str) -> Result<String, String> {
    let valid = match text.split_once('/') {
        Some((owner, repo)) => {
            !owner.is_empty()
                && !repo.is_empty()
                && !repo.contains('/')
                && !text.contains(':')
                && !text.contains(char::is_whitespace)
        }
        None => false,
    };
    if valid {
        Ok(format!("https://github.com/{text}.git"))
    } else {
        Err(format!("expected \"owner/repo\", got {text:?}"))
    }
}

/// URL の最後の要素から `.git` を除いたもの。
fn url_name(url: &str) -> String {
    let last = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default();
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

/// `{ "owner/repo", rev = ..., ... }` などの table。
fn entry(table: &Table, context: &Context) -> Result<PluginSpec, String> {
    let shorthand = optional_string(table, 1, "[1]")?;
    let url = optional_string(table, "url", "url")?;
    let dir = optional_string(table, "dir", "dir")?;
    let label = shorthand
        .clone()
        .or_else(|| url.clone())
        .or_else(|| dir.clone())
        .unwrap_or_default();
    let error = |reason: String| format!("{label}: {reason}");

    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|err| err.to_string())?;
        let known = match &key {
            Value::Integer(1) => true,
            Value::String(text) => ENTRY_KEYS.contains(&&*text.to_string_lossy()),
            _ => false,
        };
        if !known {
            return Err(error(format!("unknown key {}", key_name(&key))));
        }
    }
    let field = |key: &str| optional_string(table, key, key).map_err(error);
    let rev = field("rev")?;
    let name = field("name")?;
    let main = field("main")?;
    let opts = match raw(table, "opts")? {
        Value::Nil => None,
        Value::Table(opts) => Some(opts),
        other => return Err(error(type_error("opts", "table", &other))),
    };
    let config = match raw(table, "config")? {
        Value::Nil => None,
        Value::Function(config) => Some(config),
        other => return Err(error(type_error("config", "function", &other))),
    };

    let (default_name, source) = match (shorthand, url, dir) {
        (Some(shorthand), None, None) => {
            let url = github_url(&shorthand).map_err(error)?;
            (url_name(&url), Source::Git { url, rev })
        }
        (None, Some(url), None) => (url_name(&url), Source::Git { url, rev }),
        (None, None, Some(dir)) => {
            if rev.is_some() {
                return Err(error("rev cannot be used with dir".into()));
            }
            let path = expand_dir(&dir, context);
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            (name, Source::Dir(path))
        }
        _ => {
            return Err(error(
                "only one of \"owner/repo\", url and dir can be given".into(),
            ));
        }
    };
    plugin(name.unwrap_or(default_name), source, opts, config, main)
}

/// 名前を検証してプラグインの宣言を作る。
fn plugin(
    name: String,
    source: Source,
    opts: Option<Table>,
    config: Option<Function>,
    main: Option<String>,
) -> Result<PluginSpec, String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(format!("invalid plugin name {name:?}"));
    }
    let main = main.unwrap_or_else(|| name.strip_suffix(".kugutsu").unwrap_or(&name).to_string());
    Ok(PluginSpec {
        spec: Spec { name, source },
        opts,
        config,
        main,
    })
}

/// 文字列か nil のフィールド。`label` はエラーに出す名前。
fn optional_string(
    table: &Table,
    key: impl mlua::IntoLua,
    label: &str,
) -> Result<Option<String>, String> {
    match raw(table, key)? {
        Value::Nil => Ok(None),
        Value::String(text) => Ok(Some(text.to_string_lossy())),
        other => Err(type_error(label, "string", &other)),
    }
}

fn type_error(key: &str, expected: &str, value: &Value) -> String {
    format!("{key}: expected {expected}, got {}", lua_type_name(value))
}

fn key_name(key: &Value) -> String {
    match key {
        Value::String(text) => format!("'{}'", text.to_string_lossy()),
        Value::Integer(n) => n.to_string(),
        other => lua_type_name(other).to_string(),
    }
}

/// `dir` の展開。`~` はホームに、相対パスは `init.lua` のあるディレクトリを基準にする。
fn expand_dir(text: &str, context: &Context) -> PathBuf {
    let path = match (text.strip_prefix('~'), &context.home) {
        (Some(""), Some(home)) => return home.clone(),
        (Some(rest), Some(home)) if rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(text),
    };
    match &context.config_dir {
        Some(base) if Path::new(&path).is_relative() => base.join(path),
        _ => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    fn context() -> Context {
        Context {
            home: Some("/home/me".into()),
            config_dir: Some("/home/me/.config/kugutsu".into()),
        }
    }

    /// Lua の式 `source` を評価して `parse` に渡す。
    fn parse_lua(source: &str) -> Result<Vec<PluginSpec>, String> {
        parse_in(&Lua::new(), source)
    }

    /// `lua` の中で評価する。`opts` / `config` を使うテストは `lua` を生かしておく。
    fn parse_in(lua: &Lua, source: &str) -> Result<Vec<PluginSpec>, String> {
        let value: Value = lua.load(format!("return {source}")).eval().unwrap();
        parse(&value, &context())
    }

    fn specs(source: &str) -> Vec<Spec> {
        parse_lua(source)
            .unwrap_or_else(|err| panic!("{source}: {err}"))
            .into_iter()
            .map(|plugin| plugin.spec)
            .collect()
    }

    fn git(name: &str, url: &str, rev: Option<&str>) -> Spec {
        Spec {
            name: name.into(),
            source: Source::Git {
                url: url.into(),
                rev: rev.map(Into::into),
            },
        }
    }

    fn parse_err(source: &str, expected: &str) {
        let err = parse_lua(source).unwrap_err();
        assert!(
            err.contains(expected),
            "{source}: {err:?} does not contain {expected:?}"
        );
    }

    #[test]
    fn expands_github_shorthand() {
        assert_eq!(
            specs(r#"{ "catppuccin/catppuccin.kugutsu" }"#),
            vec![git(
                "catppuccin.kugutsu",
                "https://github.com/catppuccin/catppuccin.kugutsu.git",
                None
            )]
        );
    }

    #[test]
    fn reads_every_kind_of_entry() {
        assert_eq!(
            specs(
                r#"{
                "a/one.kugutsu",
                { "folke/tokyonight.kugutsu", rev = "v1.0.0" },
                { url = "https://git.example.com/me/foo.kugutsu.git" },
                { url = "file:///srv/git/bar.kugutsu/" },
                { "bob/theme.kugutsu", name = "bob-theme.kugutsu" },
                { dir = "~/src/my-plugin.kugutsu" },
                { dir = "/opt/abs.kugutsu", name = "abs" },
                { dir = "local/rel.kugutsu" },
            }"#
            ),
            vec![
                git("one.kugutsu", "https://github.com/a/one.kugutsu.git", None),
                git(
                    "tokyonight.kugutsu",
                    "https://github.com/folke/tokyonight.kugutsu.git",
                    Some("v1.0.0")
                ),
                git(
                    "foo.kugutsu",
                    "https://git.example.com/me/foo.kugutsu.git",
                    None
                ),
                git("bar.kugutsu", "file:///srv/git/bar.kugutsu/", None),
                git(
                    "bob-theme.kugutsu",
                    "https://github.com/bob/theme.kugutsu.git",
                    None
                ),
                Spec {
                    name: "my-plugin.kugutsu".into(),
                    source: Source::Dir("/home/me/src/my-plugin.kugutsu".into()),
                },
                Spec {
                    name: "abs".into(),
                    source: Source::Dir("/opt/abs.kugutsu".into()),
                },
                Spec {
                    name: "rel.kugutsu".into(),
                    source: Source::Dir("/home/me/.config/kugutsu/local/rel.kugutsu".into()),
                },
            ]
        );
    }

    #[test]
    fn flattens_nested_lists() {
        let names: Vec<String> = specs(
            r#"{
                { { "a/one" }, { { "b/two", rev = "main" } } },
                {},
                "c/three",
                { "d/four", "e/five" },
            }"#,
        )
        .into_iter()
        .map(|spec| spec.name)
        .collect();
        assert_eq!(names, ["one", "two", "three", "four", "five"]);
    }

    #[test]
    fn reads_opts_config_and_main() {
        let lua = Lua::new();
        let plugins = parse_in(
            &lua,
            r#"{
                { "bob/notes.kugutsu", opts = { width = 80 } },
                { "carol/notify.kugutsu", config = function(opts) end },
                { "dave/foo-kugutsu", main = "foo", opts = {} },
            }"#,
        )
        .unwrap();
        let notes = &plugins[0];
        assert_eq!(notes.main, "notes");
        assert_eq!(
            notes.opts.as_ref().unwrap().get::<i64>("width").unwrap(),
            80
        );
        assert!(notes.config.is_none());
        let notify = &plugins[1];
        assert_eq!(notify.main, "notify");
        assert!(notify.opts.is_none());
        assert!(notify.config.is_some());
        assert_eq!(plugins[2].main, "foo");
    }

    #[test]
    fn main_defaults_to_the_name_without_the_kugutsu_suffix() {
        let plugins = parse_lua(r#"{ "a/plain", { "b/x.kugutsu", name = "y.kugutsu" } }"#).unwrap();
        assert_eq!(plugins[0].main, "plain");
        assert_eq!(plugins[1].main, "y");
    }

    #[test]
    fn duplicate_names_are_errors() {
        parse_err(
            r#"{ "a/foo.kugutsu", "b/foo.kugutsu" }"#,
            "duplicate plugin name 'foo.kugutsu'",
        );
        parse_err(
            r#"{ "a/foo", { dir = "/x/foo" } }"#,
            "duplicate plugin name 'foo'",
        );
        assert_eq!(specs(r#"{ "a/foo", { "b/foo", name = "foo2" } }"#).len(), 2);
    }

    #[test]
    fn invalid_entries_are_errors() {
        let cases = [
            ("1", "expected a table, got number"),
            ("{ 1 }", "expected a string or a table, got number"),
            (r#"{ "foo" }"#, "expected \"owner/repo\", got \"foo\""),
            (r#"{ "a/b/c" }"#, "expected \"owner/repo\", got \"a/b/c\""),
            (r#"{ "/b" }"#, "expected \"owner/repo\""),
            (r#"{ "https://x/y" }"#, "expected \"owner/repo\""),
            (r#"{ { "a/b", opt = {} } }"#, "a/b: unknown key 'opt'"),
            (r#"{ { "a/b", [3] = 1 } }"#, "a/b: unknown key 3"),
            (r#"{ { "a/b", url = "https://x/y.git" } }"#, "only one of"),
            (r#"{ { url = "x", dir = "/y" } }"#, "only one of"),
            (
                r#"{ { dir = "/x", rev = "main" } }"#,
                "rev cannot be used with dir",
            ),
            (
                r#"{ { "a/b", rev = 1 } }"#,
                "a/b: rev: expected string, got number",
            ),
            (
                r#"{ { "a/b", opts = 1 } }"#,
                "a/b: opts: expected table, got number",
            ),
            (
                r#"{ { "a/b", config = {} } }"#,
                "a/b: config: expected function, got table",
            ),
            (
                r#"{ { "a/b", main = true } }"#,
                "a/b: main: expected string, got boolean",
            ),
            (r#"{ { "a/b", name = "" } }"#, "invalid plugin name \"\""),
            (
                r#"{ { "a/b", name = "x/y" } }"#,
                "invalid plugin name \"x/y\"",
            ),
            (
                r#"{ { "a/b", name = ".." } }"#,
                "invalid plugin name \"..\"",
            ),
            (r#"{ { url = "" } }"#, "invalid plugin name \"\""),
            (
                r#"{ foo = 1 }"#,
                "expected a list of plugins, got key 'foo'",
            ),
        ];
        for (source, expected) in cases {
            parse_err(source, expected);
        }
    }

    #[test]
    fn dir_without_context_is_kept_as_is() {
        let lua = Lua::new();
        let value: Value = lua
            .load(r#"return { { dir = "~/x" }, { dir = "rel" } }"#)
            .eval()
            .unwrap();
        let plugins = parse(&value, &Context::default()).unwrap();
        assert_eq!(plugins[0].spec.source, Source::Dir("~/x".into()));
        assert_eq!(plugins[1].spec.source, Source::Dir("rel".into()));
    }
}
