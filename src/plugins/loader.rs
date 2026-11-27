//! GUI 起動時のプラグインの読み込み。ディスクを読むだけで、lockfile も git も使わない。
//!
//! spec にあり、ディスクにあるプラグインを spec の順に読み込む。
//!
//! 1. `lua/` を `package.path` に足し、`colors/` をテーマの検索先に足す（全プラグイン分を先に）
//! 2. `plugin/*.lua` をファイル名順に実行する
//! 3. `config(opts)` があれば呼ぶ。無く `opts` があれば `require(main).setup(opts)` を呼ぶ
//!
//! エラーはプラグイン名を付けて報告し、そのプラグインの残りの手順は飛ばす。

use std::path::{Path, PathBuf};

use mlua::{Function, Lua, Table, Value};

use super::spec::{PluginSpec, Source};
use crate::config::error_message;

/// `package.path`。`init.lua` のあるディレクトリの `lua/` を先頭に、プラグインの `lua/` を
/// その後ろ（Lua の既定の検索先の前）に置く。
#[derive(Debug, Clone, Default)]
pub struct LuaPath {
    prefix: String,
    default: String,
}

impl LuaPath {
    /// `init.lua` を実行する前に、`config_dir/lua/` を `package.path` の先頭に足す。
    pub fn install(lua: &Lua, config_dir: Option<&Path>) -> mlua::Result<Self> {
        let default: String = package(lua)?.get("path")?;
        let this = Self {
            prefix: config_dir.map(lua_dirs).unwrap_or_default(),
            default,
        };
        this.apply(lua, &[])?;
        Ok(this)
    }

    /// プラグインのディレクトリ `dirs` の `lua/` を足す。
    fn apply(&self, lua: &Lua, dirs: &[PathBuf]) -> mlua::Result<()> {
        let plugins: String = dirs.iter().map(|dir| lua_dirs(dir)).collect();
        package(lua)?.set("path", format!("{}{plugins}{}", self.prefix, self.default))
    }
}

fn package(lua: &Lua) -> mlua::Result<Table> {
    lua.globals().get("package")
}

/// `dir/lua/` から `require` する検索パターン。
fn lua_dirs(dir: &Path) -> String {
    let lua = dir.join("lua");
    let lua = lua.display();
    format!("{lua}/?.lua;{lua}/?/init.lua;")
}

/// 読み込みの結果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// 読み込み中のエラー（`<name>: <message>`）。
    pub errors: Vec<String>,
    /// spec にあるが `plugins/` に無いプラグインの名前。
    pub missing: Vec<String>,
    /// 読み込んだプラグインの `colors/`（spec の順）。
    pub color_dirs: Vec<PathBuf>,
}

/// `plugins` を読み込む。`plugins_dir` は git のプラグインを置くディレクトリ。
pub fn load(
    lua: &Lua,
    plugins: &[PluginSpec],
    lua_path: &LuaPath,
    plugins_dir: Option<&Path>,
) -> Report {
    let mut report = Report::default();
    let mut found = Vec::new();
    for plugin in plugins {
        let name = &plugin.spec.name;
        match &plugin.spec.source {
            Source::Git { .. } => match plugins_dir.map(|dir| dir.join(name)) {
                Some(dir) if dir.is_dir() => found.push((plugin, dir)),
                _ => report.missing.push(name.clone()),
            },
            Source::Dir(dir) if dir.is_dir() => found.push((plugin, dir.clone())),
            Source::Dir(dir) => report
                .errors
                .push(format!("{name}: directory not found: {}", dir.display())),
        }
    }

    let dirs: Vec<PathBuf> = found.iter().map(|(_, dir)| dir.clone()).collect();
    if let Err(err) = lua_path.apply(lua, &dirs) {
        report
            .errors
            .push(format!("plugins: {}", error_message(&err)));
    }
    report.color_dirs = dirs.iter().map(|dir| dir.join("colors")).collect();

    for (plugin, dir) in found {
        if let Err(err) = load_plugin(lua, plugin, &dir) {
            report.errors.push(format!("{}: {err}", plugin.spec.name));
        }
    }
    report
}

/// 1 つのプラグインの `plugin/*.lua` を実行し、`config` / `setup` を呼ぶ。
fn load_plugin(lua: &Lua, plugin: &PluginSpec, dir: &Path) -> Result<(), String> {
    for script in scripts(&dir.join("plugin"))? {
        let source = std::fs::read_to_string(&script)
            .map_err(|err| format!("{}: {err}", script.display()))?;
        let file = script.file_name().unwrap_or_default().to_string_lossy();
        lua.load(source)
            .set_name(format!("=plugin/{file}"))
            .exec()
            .map_err(|err| error_message(&err))?;
    }
    let opts = plugin.opts.clone().map_or(Value::Nil, Value::Table);
    if let Some(config) = &plugin.config {
        return config.call::<()>(opts).map_err(|err| error_message(&err));
    }
    if opts.is_nil() {
        return Ok(());
    }
    let require: Function = lua
        .globals()
        .get("require")
        .map_err(|err| err.to_string())?;
    let module: Value = require
        .call(plugin.main.as_str())
        .map_err(|err| error_message(&err))?;
    let setup = match module {
        Value::Table(module) => module
            .get::<Value>("setup")
            .map_err(|err| err.to_string())?,
        _ => Value::Nil,
    };
    match setup {
        Value::Function(setup) => setup.call::<()>(opts).map_err(|err| error_message(&err)),
        _ => Err(format!("module '{}' has no setup function", plugin.main)),
    }
}

/// `dir` の `*.lua` をファイル名順に。`dir` が無ければ空。
fn scripts(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("{}: {err}", dir.display())),
    };
    let mut scripts: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "lua") && path.is_file())
        .collect();
    scripts.sort();
    Ok(scripts)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::config::{Loaded, evaluate_with};
    use crate::plugins::paths::Paths;

    /// テストごとの一時ディレクトリ（`config/` と `data/plugins/` を持つ）。
    struct Env {
        root: PathBuf,
    }

    impl Env {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("kugutsu-loader-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("config")).unwrap();
            fs::create_dir_all(root.join("data/plugins")).unwrap();
            Self { root }
        }

        fn paths(&self) -> Paths {
            Paths::new(
                self.root.join("config/init.lua"),
                Some(self.root.join("data/plugins")),
                Some(self.root.clone()),
            )
        }

        /// `root` からの相対パス `path` にファイルを書く。
        fn write(&self, path: &str, contents: &str) {
            let path = self.root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        fn eval(&self, init_lua: &str) -> Loaded {
            evaluate_with(init_lua, "init.lua", Some(&self.paths()))
        }
    }

    /// 読み込んだ順を `kugutsu.opt.font_family` に積んで記録する。
    fn record(tag: &str) -> String {
        format!("kugutsu.opt.font_family = (kugutsu.opt.font_family or '') .. '{tag};'")
    }

    fn order(loaded: &Loaded) -> String {
        loaded.config.font_family.clone().unwrap_or_default()
    }

    #[test]
    fn runs_plugin_scripts_in_spec_and_file_name_order() {
        let env = Env::new("order");
        env.write("data/plugins/b/plugin/2.lua", &record("b2"));
        env.write("data/plugins/b/plugin/1.lua", &record("b1"));
        env.write("data/plugins/b/plugin/readme.txt", "not lua");
        env.write("data/plugins/a/plugin/x.lua", &record("a"));
        let loaded = env.eval(r#"kugutsu.plugins.setup({ "o/b", "o/a" })"#);
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(order(&loaded), "b1;b2;a;");
        assert_eq!(loaded.missing_plugins, Vec::<String>::new());
    }

    #[test]
    fn plugins_load_after_init_lua() {
        let env = Env::new("after");
        env.write("data/plugins/a/plugin/a.lua", &record("plugin"));
        let loaded = env.eval(&format!(
            r#"kugutsu.plugins.setup({{ "o/a" }})
            {}"#,
            record("init")
        ));
        assert_eq!(order(&loaded), "init;plugin;");
    }

    #[test]
    fn missing_plugins_are_reported_not_loaded() {
        let env = Env::new("missing");
        env.write("data/plugins/here/plugin/x.lua", &record("here"));
        let loaded = env.eval(r#"kugutsu.plugins.setup({ "o/gone", "o/here", "o/also-gone" })"#);
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(loaded.missing_plugins, ["gone", "also-gone"]);
        assert_eq!(order(&loaded), "here;");
    }

    #[test]
    fn plugins_not_in_the_spec_are_not_loaded() {
        let env = Env::new("unlisted");
        env.write("data/plugins/other/plugin/x.lua", &record("other"));
        let loaded = env.eval("");
        assert_eq!(order(&loaded), "");
    }

    #[test]
    fn requires_modules_from_plugins_and_the_config_dir() {
        let env = Env::new("require");
        env.write("config/lua/mine.lua", "return { tag = 'mine' }");
        env.write("config/lua/plugins/ui.lua", "return { 'o/notes.kugutsu' }");
        env.write(
            "data/plugins/notes.kugutsu/lua/notes/init.lua",
            "return { tag = 'notes' }",
        );
        env.write(
            "data/plugins/notes.kugutsu/plugin/notes.lua",
            "kugutsu.opt.font_family = require('notes').tag",
        );
        let loaded = env.eval(
            r#"assert(require("mine").tag == "mine")
            kugutsu.plugins.setup({ require("plugins.ui") })"#,
        );
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(order(&loaded), "notes");
    }

    #[test]
    fn config_dir_modules_shadow_plugin_modules() {
        let env = Env::new("shadow");
        env.write("config/lua/notes.lua", "return { tag = 'user' }");
        env.write(
            "data/plugins/notes/lua/notes.lua",
            "return { tag = 'plugin' }",
        );
        env.write(
            "data/plugins/notes/plugin/x.lua",
            "kugutsu.opt.font_family = require('notes').tag",
        );
        let loaded = env.eval(r#"kugutsu.plugins.setup({ "o/notes" })"#);
        assert_eq!(order(&loaded), "user");
    }

    #[test]
    fn calls_setup_with_opts() {
        let env = Env::new("setup");
        env.write(
            "data/plugins/notes.kugutsu/lua/notes.lua",
            "return { setup = function(opts) kugutsu.opt.font_size = opts.size end }",
        );
        env.write(
            "data/plugins/foo-kugutsu/lua/foo.lua",
            "return { setup = function(opts) kugutsu.opt.line_height = opts.lh end }",
        );
        let loaded = env.eval(
            r#"kugutsu.plugins.setup({
                { "o/notes.kugutsu", opts = { size = 20 } },
                { "o/foo-kugutsu", main = "foo", opts = { lh = 2 } },
            })"#,
        );
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(loaded.config.font_size, 20.0);
        assert_eq!(loaded.config.line_height, 2.0);
    }

    #[test]
    fn config_is_called_instead_of_setup() {
        let env = Env::new("config");
        env.write(
            "data/plugins/notify/lua/notify.lua",
            "return { setup = function() error('setup must not be called') end }",
        );
        let loaded = env.eval(
            r#"kugutsu.plugins.setup({
                { "o/notify", opts = { size = 19 }, config = function(opts)
                    kugutsu.opt.font_size = opts.size
                end },
            })"#,
        );
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(loaded.config.font_size, 19.0);
    }

    #[test]
    fn config_without_opts_gets_nil_and_setup_is_not_called_without_opts() {
        let env = Env::new("no-opts");
        env.write(
            "data/plugins/a/lua/a.lua",
            "return { setup = function() error('setup must not be called') end }",
        );
        let loaded = env.eval(
            r#"kugutsu.plugins.setup({
                "o/a",
                { "o/b", config = function(opts) assert(opts == nil); kugutsu.opt.font_size = 30 end },
            })"#,
        );
        assert_eq!(loaded.missing_plugins, ["b"]);
        assert_eq!(loaded.config.font_size, 14.0);
        env.write("data/plugins/b/plugin/.keep.lua", "");
        let loaded = env.eval(
            r#"kugutsu.plugins.setup({
                "o/a",
                { "o/b", config = function(opts) assert(opts == nil); kugutsu.opt.font_size = 30 end },
            })"#,
        );
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(loaded.config.font_size, 30.0);
    }

    #[test]
    fn errors_skip_the_rest_of_the_plugin_only() {
        let env = Env::new("errors");
        env.write("data/plugins/bad/plugin/1.lua", "\nerror('boom')");
        env.write("data/plugins/bad/plugin/2.lua", &record("bad2"));
        env.write(
            "data/plugins/bad/lua/bad.lua",
            "return { setup = function() kugutsu.opt.font_size = 99 end }",
        );
        env.write("data/plugins/good/plugin/x.lua", &record("good"));
        let loaded = env.eval(r#"kugutsu.plugins.setup({ { "o/bad", opts = {} }, "o/good" })"#);
        assert_eq!(loaded.errors, vec!["bad: plugin/1.lua:2: boom".to_string()]);
        assert_eq!(order(&loaded), "good;");
        assert_eq!(loaded.config.font_size, 14.0);
    }

    #[test]
    fn opts_without_a_setup_function_is_an_error() {
        let env = Env::new("no-setup");
        env.write("data/plugins/a/lua/a.lua", "return {}");
        env.write("data/plugins/b/lua/b.lua", "return 1");
        let loaded =
            env.eval(r#"kugutsu.plugins.setup({ { "o/a", opts = {} }, { "o/b", opts = {} } })"#);
        assert_eq!(
            loaded.errors,
            vec![
                "a: module 'a' has no setup function".to_string(),
                "b: module 'b' has no setup function".to_string(),
            ]
        );
        let loaded = env.eval(r#"kugutsu.plugins.setup({ { "o/a", main = "nope", opts = {} } })"#);
        assert_eq!(loaded.errors.len(), 1);
        assert!(
            loaded.errors[0].starts_with("a: module 'nope' not found"),
            "{:?}",
            loaded.errors
        );
    }

    #[test]
    fn dir_plugins_are_loaded_from_their_directory() {
        let env = Env::new("dir");
        env.write("src/dev.kugutsu/plugin/x.lua", &record("dev"));
        let loaded = env.eval(
            r#"kugutsu.plugins.setup({ { dir = "~/src/dev.kugutsu" }, { dir = "~/src/none" } })"#,
        );
        assert_eq!(order(&loaded), "dev;");
        assert_eq!(loaded.missing_plugins, Vec::<String>::new());
        assert_eq!(loaded.errors.len(), 1);
        assert!(
            loaded.errors[0].starts_with("none: directory not found: "),
            "{:?}",
            loaded.errors
        );
    }

    #[test]
    fn external_symlinked_plugins_are_loaded() {
        let env = Env::new("symlink");
        env.write("store/ext/plugin/x.lua", &record("ext"));
        std::os::unix::fs::symlink(
            env.root.join("store/ext"),
            env.root.join("data/plugins/ext"),
        )
        .unwrap();
        let loaded = env.eval(r#"kugutsu.plugins.setup({ "o/ext" })"#);
        assert_eq!(order(&loaded), "ext;");
    }

    /// `kugutsu-dark` を元に、ターミナルの背景色だけ変えたテーマ。
    fn theme_source(background: &str) -> String {
        let dark = include_str!("../../runtime/colors/kugutsu-dark.lua");
        dark.replacen(
            r##"background = "#0a0a0b""##,
            &format!(r##"background = "{background}""##),
            1,
        )
    }

    #[test]
    fn themes_are_looked_up_in_user_then_plugin_then_bundled_colors() {
        let env = Env::new("themes");
        env.write("data/plugins/p1/colors/both.lua", &theme_source("#000001"));
        env.write("data/plugins/p2/colors/both.lua", &theme_source("#000002"));
        env.write(
            "data/plugins/p2/colors/second.lua",
            &theme_source("#000003"),
        );
        env.write(
            "data/plugins/p2/colors/kugutsu-light.lua",
            &theme_source("#000004"),
        );
        env.write("config/colors/mine.lua", &theme_source("#000005"));
        env.write("config/colors/both.lua", &theme_source("#000006"));
        let background = |theme: &str| {
            let loaded = env.eval(&format!(
                r#"kugutsu.opt.theme = "{theme}"
                kugutsu.plugins.setup({{ "o/p1", "o/p2" }})"#
            ));
            assert_eq!(loaded.errors, Vec::<String>::new(), "{theme}");
            assert_eq!(loaded.config.theme.name, theme);
            loaded.config.theme.terminal.background
        };
        assert_eq!(background("mine"), 0x000005);
        assert_eq!(background("both"), 0x000006);
        assert_eq!(background("second"), 0x000003);
        // プラグインのテーマは同梱テーマより優先する
        assert_eq!(background("kugutsu-light"), 0x000004);
        assert_eq!(background("kugutsu-dark"), 0x0a0a0b);
    }

    #[test]
    fn invalid_or_unknown_themes_fall_back_to_the_default() {
        let env = Env::new("bad-theme");
        env.write("config/colors/broken.lua", "return 1");
        let loaded = env.eval(r#"kugutsu.opt.theme = "broken""#);
        assert_eq!(
            loaded.errors,
            vec!["kugutsu.opt.theme: broken: expected a table, got number".to_string()]
        );
        assert_eq!(loaded.config.theme, crate::theme::Theme::default());
        let loaded = env.eval(r#"kugutsu.opt.theme = "../config/colors/broken""#);
        assert_eq!(
            loaded.errors,
            vec!["kugutsu.opt.theme: unknown theme '../config/colors/broken'".to_string()]
        );
    }

    #[test]
    fn a_plugin_can_set_the_theme_it_ships() {
        let env = Env::new("plugin-theme");
        env.write("data/plugins/t/colors/t.lua", &theme_source("#000007"));
        env.write("data/plugins/t/plugin/t.lua", "kugutsu.opt.theme = 't'");
        let loaded = env.eval(r#"kugutsu.plugins.setup({ "o/t" })"#);
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(loaded.config.theme.terminal.background, 0x000007);
    }

    #[test]
    fn plugins_survive_an_error_later_in_init_lua() {
        let env = Env::new("init-error");
        env.write("data/plugins/a/plugin/x.lua", &record("a"));
        let loaded = env.eval(
            r#"kugutsu.plugins.setup({ "o/a" })
            error("boom")"#,
        );
        assert_eq!(loaded.errors, vec!["init.lua:2: boom".to_string()]);
        assert_eq!(order(&loaded), "a;");
    }

    #[test]
    fn report_without_a_plugins_dir_marks_git_plugins_missing() {
        let lua = Lua::new();
        let value: Value = lua.load(r#"return { "o/a" }"#).eval().unwrap();
        let plugins = crate::plugins::spec::parse(&value, &Default::default()).unwrap();
        let lua_path = LuaPath::install(&lua, None).unwrap();
        let report = load(&lua, &plugins, &lua_path, None);
        assert_eq!(report.missing, ["a"]);
    }
}
