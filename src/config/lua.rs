//! `init.lua` の評価。`kugutsu.opt` への代入と `kugutsu.keymap` によるキーの割り当てを
//! 検証して [`Config`] に書き込み、`kugutsu.plugins.setup` で宣言したプラグインを読み込む。

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use gpui::Keystroke;
use mlua::{Function, Lua, Table, Value};

use super::{Config, Loaded};
use crate::plugins::loader::{self, LuaPath};
use crate::plugins::paths::Paths;
use crate::plugins::spec::{self, PluginSpec, Spec};
use crate::tabs::prefix::{Action, Command, PREFIX_PLACEHOLDER, Trigger};
use crate::theme;

/// `kugutsu` グローバルを組み立てる Lua コード。`set` / `get` などは Rust の関数で、
/// 不正な値ならエラーメッセージを返す。`error(..., 2)` で呼び出した行をエラー位置にする。
const PRELUDE: &str = r#"
local set, get, ui_set, ui_get, keymap_set, keymap_del, action, action_names, plugins_setup,
    theme_register = ...
local function check(err)
    if err then error(err, 3) end
end
-- 代入と参照を Rust の関数で検証する table（`kugutsu.opt` / `kugutsu.ui`）
local function options(set, get)
    return setmetatable({}, {
        __newindex = function(_, key, value)
            check(set(key, value))
        end,
        __index = function(_, key)
            local value, err = get(key)
            check(err)
            return value
        end,
    })
end
local opt = options(set, get)
local ui = options(ui_set, ui_get)
local keymap = {
    set = function(key, rhs, opts)
        check(keymap_set(key, rhs, opts))
    end,
    del = function(key)
        check(keymap_del(key))
    end,
}
local actions = {}
for _, name in ipairs(action_names) do
    actions[name] = function()
        check(action(name))
    end
end
actions.select_tab = function(number)
    check(action("select_tab", number))
end
local plugins = {
    setup = function(spec)
        check(plugins_setup(spec))
    end,
}
local theme = {
    register = function(t)
        check(theme_register(t))
    end,
}
kugutsu = {
    opt = opt,
    ui = ui,
    keymap = keymap,
    action = actions,
    plugins = plugins,
    theme = theme,
}
-- Lua 5.4 の require はローダーのデータ（ファイルのパス）も返すので、
-- `setup({ "a/b", require("plugins.ui") })` のように table の最後に書いても値が 1 つになるようにする
local lua_require = require
require = function(name)
    local ok, result = pcall(lua_require, name)
    if not ok then
        -- エラー位置は require を呼んだ行にする
        error(result, type(result) == "string" and 2 or 0)
    end
    return result
end
"#;

/// 評価中に Lua から書き込む状態。
#[derive(Debug, Default)]
struct State {
    config: Config,
    /// `kugutsu.opt.theme` に代入された名前。テーマはプラグインを読み込んだ後に引く。
    theme: Option<String>,
    /// `kugutsu.theme.register` で登録したテーマ（登録順）。
    registered_themes: Vec<theme::Theme>,
    /// `kugutsu.opt.theme_overrides`。テーマを引いた後に掛ける。
    theme_overrides: Option<theme::ThemePatch>,
    /// 最後の `kugutsu.plugins.setup` で宣言したプラグイン。
    plugins: Vec<PluginSpec>,
}

type SharedState = Rc<RefCell<State>>;

/// `kugutsu.action.*` が積む操作の行き先。`None` のときはキーマップの実行中ではない。
type ActionQueue = Rc<RefCell<Option<Vec<Command>>>>;

/// キーに割り当てた Lua 関数。
///
/// mlua の値は Lua 本体を弱参照でしか持たないので、`init.lua` の評価後も呼べるよう
/// `Lua` を一緒に持って生かしておく。
#[derive(Clone)]
pub struct LuaCallback {
    _lua: Lua,
    function: Function,
    queue: ActionQueue,
}

impl LuaCallback {
    /// 関数を呼び、その中で `kugutsu.action.*` が呼ばれた操作を順に返す。
    pub fn call(&self) -> Result<Vec<Command>, String> {
        let previous = self.queue.replace(Some(Vec::new()));
        let result = self.function.call::<()>(());
        let commands = std::mem::replace(&mut *self.queue.borrow_mut(), previous);
        result.map_err(|err| error_message(&err))?;
        Ok(commands.unwrap_or_default())
    }
}

impl fmt::Debug for LuaCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LuaCallback")
    }
}

impl PartialEq for LuaCallback {
    fn eq(&self, other: &Self) -> bool {
        // 同じキューなら同じ Lua の関数なので比べられる（別の Lua の値を比べると panic する）
        Rc::ptr_eq(&self.queue, &other.queue) && self.function == other.function
    }
}

/// `init.lua` を評価した Lua。評価の後もビューが持ち、gpui のタスクなどから Lua を呼び戻す。
#[derive(Clone)]
pub struct Runtime {
    lua: Rc<Lua>,
}

impl Runtime {
    /// メインスレッドで Lua を呼ぶ。エラーはスタックトレースを落としたメッセージで返す。
    pub fn call(&self, f: impl FnOnce(&Lua) -> mlua::Result<()>) -> Result<(), String> {
        f(&self.lua).map_err(|err| error_message(&err))
    }
}

impl fmt::Debug for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Runtime")
    }
}

impl PartialEq for Runtime {
    /// 同じ Lua state を指すときだけ等しい。
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.lua, &other.lua)
    }
}

/// Lua のソースを評価して設定を読み込む。`chunk_name` はエラーメッセージに出す名前。
/// エラーが起きてもそこまでに設定された値は残す。プラグインの置き場所は無いものとする。
#[cfg(test)]
pub fn evaluate(source: &str, chunk_name: &str) -> Loaded {
    evaluate_with(source, chunk_name, None)
}

/// [`evaluate`] に加えて、`paths` の `lua/` を `require` でき、宣言したプラグインを読み込む。
pub fn evaluate_with(source: &str, chunk_name: &str, paths: Option<&Paths>) -> Loaded {
    evaluate_in(source, chunk_name, paths, true).0
}

/// プラグインを読み込まずに `init.lua` を評価し、プラグインの宣言とエラーを返す（CLI 用）。
pub fn evaluate_spec(
    source: &str,
    chunk_name: &str,
    paths: Option<&Paths>,
) -> (Vec<Spec>, Vec<String>) {
    let (loaded, specs) = evaluate_in(source, chunk_name, paths, false);
    (specs, loaded.errors)
}

fn evaluate_in(
    source: &str,
    chunk_name: &str,
    paths: Option<&Paths>,
    load_plugins: bool,
) -> (Loaded, Vec<Spec>) {
    let state = SharedState::default();
    let lua = Lua::new();
    let mut errors = Vec::new();
    let lua_path = match prepare(&lua, &state, paths) {
        Ok(lua_path) => {
            // `=` で始まる名前はそのままエラー位置に出る（`init.lua:3: ...`）
            let result = lua.load(source).set_name(format!("={chunk_name}")).exec();
            if let Err(err) = result {
                errors.push(error_message(&err));
            }
            Some(lua_path)
        }
        Err(err) => {
            errors.push(error_message(&err));
            None
        }
    };

    let plugins = std::mem::take(&mut state.borrow_mut().plugins);
    let specs = plugins.iter().map(|plugin| plugin.spec.clone()).collect();
    let mut missing_plugins = Vec::new();
    let mut plugin_color_dirs = Vec::new();
    if let (true, Some(lua_path)) = (load_plugins, &lua_path) {
        // プラグインも `kugutsu.opt` などに書き込むので、設定を取り出す前に読み込む
        let plugins_dir = paths.and_then(|paths| paths.plugins_dir.as_deref());
        let report = loader::load(&lua, &plugins, lua_path, plugins_dir);
        errors.extend(report.errors);
        missing_plugins = report.missing;
        plugin_color_dirs = report.color_dirs;
    }
    drop(plugins);

    // Lua に登録した関数は `state` を持ち続ける。割り当てた Lua 関数は Lua 本体を持つので、
    // 中身を取り出して Lua → `state` → Lua の循環参照を作らない
    let State {
        mut config,
        theme,
        registered_themes,
        theme_overrides,
        ..
    } = std::mem::take(&mut *state.borrow_mut());
    if let (true, Some(name)) = (load_plugins, theme) {
        // テーマは init.lua とプラグインの後に引くので、setup・register と opt.theme の順序は問わない
        let catalog = theme::Catalog {
            user_dir: paths.map(|paths| paths.config_dir.join("colors")),
            registered: registered_themes,
            plugin_dirs: plugin_color_dirs,
        };
        match catalog.find(&name) {
            Ok(theme) => config.theme = theme,
            Err(err) => errors.push(format!("kugutsu.opt.theme: {err}")),
        }
    }
    // テーマが見つからなくても、上書きは既定のテーマに掛ける
    if let Some(overrides) = &theme_overrides {
        config.theme.apply(overrides);
    }
    // prefix は後から変えられるので、prefix と同じキーへの直接の割り当ては最後に確かめる
    if config.keymap.del(&Trigger::Direct(config.prefix.clone())) {
        errors.push(format!(
            "kugutsu.keymap: '{}' is the prefix key and cannot be mapped without {PREFIX_PLACEHOLDER}",
            config.prefix.unparse()
        ));
    }
    let loaded = Loaded {
        config,
        errors,
        missing_plugins,
        runtime: Some(Runtime { lua: Rc::new(lua) }),
    };
    (loaded, specs)
}

/// `kugutsu` グローバルを組み立て、`package.path` を設定する。
fn prepare(lua: &Lua, state: &SharedState, paths: Option<&Paths>) -> mlua::Result<LuaPath> {
    let queue: ActionQueue = Rc::default();
    let (set, get) = option_functions(lua, state, set_option, get_option)?;
    let (ui_set, ui_get) = option_functions(lua, state, set_ui_option, get_ui_option)?;
    let target = Rc::clone(state);
    let callback_queue = Rc::clone(&queue);
    let keymap_set = lua.create_function(move |lua, (key, rhs, opts): (Value, Value, Value)| {
        let mapping = parse_mapping(lua, &callback_queue, &key, rhs, &opts);
        Ok(mapping
            .map(|(trigger, action, desc)| {
                target.borrow_mut().config.keymap.set(trigger, action, desc)
            })
            .err())
    })?;
    let target = Rc::clone(state);
    let keymap_del = lua.create_function(move |_, key: Value| {
        Ok(delete_mapping(&mut target.borrow_mut().config, &key).err())
    })?;
    let action = lua.create_function(move |_, (name, arg): (String, Value)| {
        Ok(push_action(&queue, &name, &arg).err())
    })?;
    let action_names = Command::NAMED.iter().map(|&(name, _)| name);
    let action_names = lua.create_sequence_from(action_names)?;
    let target = Rc::clone(state);
    let context = spec::Context {
        home: paths.and_then(|paths| paths.home.clone()),
        config_dir: paths.map(|paths| paths.config_dir.clone()),
    };
    let plugins_setup = lua.create_function(move |_, value: Value| {
        Ok(match spec::parse(&value, &context) {
            Ok(plugins) => {
                target.borrow_mut().plugins = plugins;
                None
            }
            Err(err) => Some(format!("kugutsu.plugins.setup: {err}")),
        })
    })?;
    let target = Rc::clone(state);
    let theme_register = lua.create_function(move |_, value: Value| {
        Ok(match theme::from_table(&value) {
            Ok(theme) => {
                target.borrow_mut().registered_themes.push(theme);
                None
            }
            Err(err) => Some(format!("kugutsu.theme.register: {err}")),
        })
    })?;
    lua.load(PRELUDE).set_name("=prelude").call::<()>((
        set,
        get,
        ui_set,
        ui_get,
        keymap_set,
        keymap_del,
        action,
        action_names,
        plugins_setup,
        theme_register,
    ))?;
    LuaPath::install(lua, paths.map(|paths| paths.config_dir.as_path()))
}

type SetOption = fn(&mut State, &str, &Value) -> Result<(), String>;
type GetOption = fn(&Lua, &State, &str) -> Result<Value, String>;

/// 設定の table（`kugutsu.opt` など）の代入・参照を検証する Lua 関数の組。
/// どちらも不正なら最後の戻り値にエラーメッセージを返す。
fn option_functions(
    lua: &Lua,
    state: &SharedState,
    set: SetOption,
    get: GetOption,
) -> mlua::Result<(Function, Function)> {
    let target = Rc::clone(state);
    let setter = lua.create_function(move |_, (key, value): (String, Value)| {
        Ok(set(&mut target.borrow_mut(), &key, &value).err())
    })?;
    let source = Rc::clone(state);
    let getter = lua.create_function(move |lua, key: String| {
        Ok(match get(lua, &source.borrow(), &key) {
            Ok(value) => (value, None),
            Err(err) => (Value::Nil, Some(err)),
        })
    })?;
    Ok((setter, getter))
}

/// `kugutsu.keymap.set(key, rhs, opts)` の引数を検証する。
fn parse_mapping(
    lua: &Lua,
    queue: &ActionQueue,
    key: &Value,
    rhs: Value,
    opts: &Value,
) -> Result<(Trigger, Action, Option<String>), String> {
    let error = |reason: String| format!("kugutsu.keymap.set: {reason}");
    let trigger = parse_trigger(key).map_err(error)?;
    let action = match rhs {
        Value::String(name) => {
            let name = name.to_string_lossy();
            Command::from_name(&name)
                .map(Action::Command)
                .ok_or_else(|| error(format!("unknown action '{name}'")))?
        }
        Value::Function(function) => Action::Lua(LuaCallback {
            _lua: lua.clone(),
            function,
            queue: Rc::clone(queue),
        }),
        other => {
            return Err(error(format!(
                "action: expected string or function, got {}",
                lua_type_name(&other)
            )));
        }
    };
    let desc = match opts {
        Value::Nil => None,
        Value::Table(table) => description(table).map_err(error)?,
        other => {
            return Err(error(format!(
                "opts: expected table, got {}",
                lua_type_name(other)
            )));
        }
    };
    Ok((trigger, action, desc))
}

/// `opts.desc`。
fn description(opts: &Table) -> Result<Option<String>, String> {
    match opts.get::<Value>("desc").map_err(|err| err.to_string())? {
        Value::Nil => Ok(None),
        Value::String(text) => Ok(Some(text.to_string_lossy())),
        other => Err(format!(
            "desc: expected string, got {}",
            lua_type_name(&other)
        )),
    }
}

/// `kugutsu.keymap.del(key)`。
fn delete_mapping(config: &mut Config, key: &Value) -> Result<(), String> {
    let error = |reason: String| format!("kugutsu.keymap.del: {reason}");
    let trigger = parse_trigger(key).map_err(error)?;
    if config.keymap.del(&trigger) {
        Ok(())
    } else {
        Err(error(format!("no mapping for '{trigger}'")))
    }
}

/// `<prefix>t` や `ctrl-shift-t` のようなキーの表記。
fn parse_trigger(key: &Value) -> Result<Trigger, String> {
    match key {
        Value::String(text) => Trigger::parse(&text.to_string_lossy()),
        other => Err(format!(
            "key: expected string, got {}",
            lua_type_name(other)
        )),
    }
}

/// `kugutsu.action.<name>(arg)` の操作をキューに積む。
fn push_action(queue: &ActionQueue, name: &str, arg: &Value) -> Result<(), String> {
    let error = |reason: &str| format!("kugutsu.action.{name}: {reason}");
    let command = match name {
        "select_tab" => Command::SelectTab(tab_number(arg).map_err(|reason| error(&reason))? - 1),
        _ => Command::from_name(name).ok_or_else(|| error("unknown action"))?,
    };
    match queue.borrow_mut().as_mut() {
        Some(commands) => {
            commands.push(command);
            Ok(())
        }
        None => Err(error("can only be called from a key mapping")),
    }
}

/// `select_tab` に渡すタブの番号（1〜9）。
fn tab_number(arg: &Value) -> Result<usize, String> {
    let expected = "expected an integer between 1 and 9";
    let number = match *arg {
        Value::Integer(n) => n as f64,
        Value::Number(n) => n,
        ref other => return Err(format!("{expected}, got {}", lua_type_name(other))),
    };
    if number.fract() == 0.0 && (1.0..=9.0).contains(&number) {
        Ok(number as usize)
    } else {
        Err(format!("{expected}, got {number}"))
    }
}

/// `kugutsu.opt.<key> = value` を検証して設定に書き込む。
fn set_option(state: &mut State, key: &str, value: &Value) -> Result<(), String> {
    let path = &format!("kugutsu.opt.{key}");
    let invalid = |reason: &str| format!("{path}: {reason}");
    let config = &mut state.config;
    match key {
        "prefix" => {
            let text = string(path, value)?;
            config.prefix = parse_keystroke(&text)
                .ok_or_else(|| invalid(&format!("invalid keystroke {text:?}")))?;
        }
        "font_family" => config.font_family = Some(font_name(path, value)?),
        "ui_font_family" => config.ui_font_family = Some(font_name(path, value)?),
        "inactive_pane_opacity" => config.inactive_pane_opacity = unit_number(path, value)?,
        "font_size" => config.font_size = positive_number(path, value)?,
        "line_height" => config.line_height = positive_number(path, value)?,
        "background_opacity" => config.background_opacity = unit_number(path, value)?,
        "background_blur" => config.background_blur = boolean(path, value)?,
        // 名前だけを覚え、テーマは評価の最後に引く
        "theme" => state.theme = Some(string(path, value)?),
        "theme_overrides" => {
            state.theme_overrides = Some(theme::overrides(value).map_err(|e| invalid(&e))?)
        }
        _ => return Err(unknown(key)),
    }
    Ok(())
}

/// `kugutsu.opt.<key>` の現在の値。
fn get_option(lua: &Lua, state: &State, key: &str) -> Result<Value, String> {
    let config = &state.config;
    let string = |text: &str| {
        lua.create_string(text)
            .map(Value::String)
            .map_err(|err| err.to_string())
    };
    match key {
        "prefix" => string(&config.prefix.unparse()),
        "font_family" => config.font_family.as_deref().map_or(Ok(Value::Nil), string),
        "ui_font_family" => config
            .ui_font_family
            .as_deref()
            .map_or(Ok(Value::Nil), string),
        "inactive_pane_opacity" => Ok(Value::Number(config.inactive_pane_opacity.into())),
        "font_size" => Ok(Value::Number(config.font_size.into())),
        "line_height" => Ok(Value::Number(config.line_height.into())),
        "theme" => string(state.theme.as_deref().unwrap_or(&config.theme.name)),
        "theme_overrides" => match &state.theme_overrides {
            None => Ok(Value::Nil),
            Some(overrides) => overrides
                .to_lua(lua)
                .map(Value::Table)
                .map_err(|err| err.to_string()),
        },
        "background_opacity" => Ok(Value::Number(config.background_opacity.into())),
        "background_blur" => Ok(Value::Boolean(config.background_blur)),
        _ => Err(unknown(key)),
    }
}

/// `kugutsu.ui.<key> = value` を検証して設定に書き込む。
fn set_ui_option(state: &mut State, key: &str, value: &Value) -> Result<(), String> {
    let path = &format!("kugutsu.ui.{key}");
    let ui = &mut state.config.ui;
    match key {
        "item_width" => ui.item_width = positive_number(path, value)?,
        _ => return Err(unknown_ui(key)),
    }
    Ok(())
}

/// `kugutsu.ui.<key>` の現在の値。
fn get_ui_option(_lua: &Lua, state: &State, key: &str) -> Result<Value, String> {
    let ui = &state.config.ui;
    match key {
        "item_width" => Ok(Value::Number(ui.item_width.into())),
        _ => Err(unknown_ui(key)),
    }
}

fn unknown(key: &str) -> String {
    format!("unknown option '{key}'")
}

fn unknown_ui(key: &str) -> String {
    format!("unknown ui option '{key}'")
}

fn type_error(path: &str, expected: &str, value: &Value) -> String {
    format!("{path}: expected {expected}, got {}", lua_type_name(value))
}

/// Lua の `type()` と同じ型名（mlua は整数を `integer` と呼ぶが、Lua では `number`）。
pub(crate) fn lua_type_name(value: &Value) -> &'static str {
    match value {
        Value::Integer(_) => "number",
        _ => value.type_name(),
    }
}

fn string(path: &str, value: &Value) -> Result<String, String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(type_error(path, "string", value)),
    }
}

/// 空白だけではないフォント名。
fn font_name(path: &str, value: &Value) -> Result<String, String> {
    let text = string(path, value)?;
    if text.trim().is_empty() {
        return Err(format!("{path}: must not be empty"));
    }
    Ok(text)
}

fn boolean(path: &str, value: &Value) -> Result<bool, String> {
    match value {
        Value::Boolean(flag) => Ok(*flag),
        _ => Err(type_error(path, "boolean", value)),
    }
}

/// 0 以上 1 以下の数値。
fn unit_number(path: &str, value: &Value) -> Result<f32, String> {
    let number = number(path, value)?;
    if (0.0..=1.0).contains(&number) {
        Ok(number as f32)
    } else {
        Err(format!(
            "{path}: expected a number between 0 and 1, got {number}"
        ))
    }
}

fn number(path: &str, value: &Value) -> Result<f64, String> {
    match *value {
        Value::Integer(n) => Ok(n as f64),
        Value::Number(n) => Ok(n),
        _ => Err(type_error(path, "number", value)),
    }
}

/// 有限で 0 より大きい数値。
fn positive_number(path: &str, value: &Value) -> Result<f32, String> {
    let number = number(path, value)?;
    if number.is_finite() && number > 0.0 {
        Ok(number as f32)
    } else {
        Err(format!("{path}: expected a positive number, got {number}"))
    }
}

fn parse_keystroke(text: &str) -> Option<Keystroke> {
    Keystroke::parse(text)
        .ok()
        .filter(|keystroke| !keystroke.key.is_empty())
}

/// UI に出すエラーメッセージ。スタックトレースは落とす。
pub(crate) fn error_message(err: &mlua::Error) -> String {
    let text = match err {
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        mlua::Error::RuntimeError(message) => message.clone(),
        other => other.to_string(),
    };
    text.split("\nstack traceback:")
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UiConfig;
    use crate::tabs::prefix::{Action, Binding, Keymap, Trigger};
    use crate::theme::Theme;
    use gpui::Keystroke;

    fn eval(source: &str) -> Loaded {
        evaluate(source, "init.lua")
    }

    fn eval_ok(source: &str) -> Config {
        let loaded = eval(source);
        assert_eq!(loaded.errors, Vec::<String>::new(), "{source}");
        loaded.config
    }

    /// エラーが 1 つだけ起き、そのメッセージに `expected` が含まれることを確かめる。
    fn eval_err(source: &str, expected: &str) -> Config {
        let loaded = eval(source);
        assert_eq!(loaded.errors.len(), 1, "{source}: {:?}", loaded.errors);
        assert!(
            loaded.errors[0].contains(expected),
            "{source}: {:?} does not contain {expected:?}",
            loaded.errors[0]
        );
        loaded.config
    }

    #[test]
    fn empty_source_means_defaults() {
        assert_eq!(eval_ok(""), Config::default());
    }

    #[test]
    fn sets_every_option() {
        let config = eval_ok(
            r#"
            kugutsu.opt.prefix = "ctrl-a"
            kugutsu.opt.font_family = "Fira Code"
            kugutsu.opt.font_size = 16.5
            kugutsu.opt.line_height = 1.5
            "#,
        );
        assert_eq!(
            config,
            Config {
                prefix: Keystroke::parse("ctrl-a").unwrap(),
                font_family: Some("Fira Code".into()),
                font_size: 16.5,
                line_height: 1.5,
                ..Config::default()
            }
        );
    }

    #[test]
    fn accepts_integers_for_numbers() {
        let config = eval_ok("kugutsu.opt.font_size = 18\nkugutsu.opt.line_height = 2");
        assert_eq!(config.font_size, 18.0);
        assert_eq!(config.line_height, 2.0);
    }

    #[test]
    fn reads_current_values() {
        let config = eval_ok("kugutsu.opt.font_size = kugutsu.opt.font_size + 2");
        assert_eq!(config.font_size, 16.0);
        let config = eval_ok(
            r#"assert(kugutsu.opt.prefix == "ctrl-b")
            assert(kugutsu.opt.font_family == nil)"#,
        );
        assert_eq!(config, Config::default());
    }

    #[test]
    fn unknown_option_is_an_error() {
        eval_err("kugutsu.opt.foo = 1", "unknown option 'foo'");
        eval_err("local x = kugutsu.opt.foo", "unknown option 'foo'");
    }

    #[test]
    fn wrong_type_is_an_error() {
        let config = eval_err(
            r#"kugutsu.opt.font_size = "big""#,
            "kugutsu.opt.font_size: expected number, got string",
        );
        assert_eq!(config, Config::default());
        eval_err(
            "kugutsu.opt.prefix = 1",
            "kugutsu.opt.prefix: expected string, got number",
        );
        eval_err(
            "kugutsu.opt.font_family = true",
            "kugutsu.opt.font_family: expected string, got boolean",
        );
    }

    #[test]
    fn invalid_values_are_errors() {
        eval_err(r#"kugutsu.opt.prefix = "ctrl-""#, "kugutsu.opt.prefix");
        eval_err(r#"kugutsu.opt.prefix = """#, "kugutsu.opt.prefix");
        eval_err(r#"kugutsu.opt.font_family = """#, "kugutsu.opt.font_family");
        eval_err("kugutsu.opt.font_size = 0", "kugutsu.opt.font_size");
        eval_err("kugutsu.opt.font_size = -3", "kugutsu.opt.font_size");
        eval_err("kugutsu.opt.line_height = 0/0", "kugutsu.opt.line_height");
        eval_err(
            "kugutsu.opt.line_height = math.huge",
            "kugutsu.opt.line_height",
        );
    }

    #[test]
    fn selects_a_bundled_theme() {
        let config = eval_ok(r#"kugutsu.opt.theme = "kugutsu-light""#);
        assert_eq!(
            &config.theme,
            crate::theme::builtin("kugutsu-light").unwrap()
        );
        eval_ok(r#"assert(kugutsu.opt.theme == "kugutsu-dark")"#);
        eval_ok(
            r#"kugutsu.opt.theme = "kugutsu-light"
            assert(kugutsu.opt.theme == "kugutsu-light")"#,
        );
    }

    #[test]
    fn unknown_theme_is_an_error() {
        let loaded = eval(r#"kugutsu.opt.theme = "nope""#);
        assert_eq!(
            loaded.errors,
            vec!["kugutsu.opt.theme: unknown theme 'nope'".to_string()]
        );
        assert_eq!(loaded.config.theme, Theme::default());
        // init.lua の評価中は代入した名前が読める
        let loaded = eval(
            r#"kugutsu.opt.theme = "nope"
            assert(kugutsu.opt.theme == "nope")"#,
        );
        assert_eq!(
            loaded.errors,
            vec!["kugutsu.opt.theme: unknown theme 'nope'".to_string()]
        );
        eval_err(
            "kugutsu.opt.theme = 1",
            "kugutsu.opt.theme: expected string, got number",
        );
    }

    #[test]
    fn selects_a_registered_theme() {
        // register と opt.theme の順序は問わない
        let config = eval_ok(
            r##"kugutsu.opt.theme = "mine"
            kugutsu.theme.register({ name = "mine", ui = { error = "#010203" } })"##,
        );
        assert_eq!(config.theme.name, "mine");
        assert_eq!(config.theme.ui.error, 0x010203);
        // 書いていない色は既定のテーマから
        assert_eq!(config.theme.terminal, Theme::default().terminal);
    }

    #[test]
    fn later_registration_wins_and_shadows_bundled_themes() {
        let config = eval_ok(
            r##"kugutsu.theme.register({ name = "kugutsu-light", ui = { error = "#000001" } })
            kugutsu.theme.register({ name = "kugutsu-light", ui = { error = "#000002" } })
            kugutsu.opt.theme = "kugutsu-light""##,
        );
        assert_eq!(config.theme.ui.error, 0x000002);
    }

    #[test]
    fn invalid_registrations_are_errors() {
        for (source, expected) in [
            ("kugutsu.theme.register(1)", "expected a table, got number"),
            ("kugutsu.theme.register({})", "name: missing"),
            (
                "kugutsu.theme.register({ name = 1 })",
                "name: expected string, got number",
            ),
            (
                r#"kugutsu.theme.register({ name = "" })"#,
                "name: must not be empty",
            ),
            (
                r#"kugutsu.theme.register({ name = "x", ui = { error = "red" } })"#,
                "x: ui.error: expected a color",
            ),
        ] {
            let config = eval_err(source, "init.lua:1: kugutsu.theme.register: ");
            eval_err(source, expected);
            assert_eq!(config.theme, Theme::default());
        }
        // 登録に失敗したテーマは選べない
        let loaded = eval(
            r#"pcall(kugutsu.theme.register, { name = "x", ui = { error = "red" } })
            kugutsu.opt.theme = "x""#,
        );
        assert_eq!(
            loaded.errors,
            vec!["kugutsu.opt.theme: unknown theme 'x'".to_string()]
        );
    }

    #[test]
    fn theme_overrides_apply_on_top_of_the_selected_theme() {
        let config = eval_ok(
            r##"kugutsu.opt.theme_overrides = {
                terminal = { cursor = "#010203" },
                ui = { active_pane_border = "#040506" },
            }
            kugutsu.opt.theme = "kugutsu-light""##,
        );
        let light = crate::theme::builtin("kugutsu-light").unwrap();
        assert_eq!(config.theme.name, "kugutsu-light");
        assert_eq!(config.theme.terminal.cursor, 0x010203);
        assert_eq!(config.theme.ui.active_pane_border, 0x040506);
        assert_eq!(config.theme.terminal.background, light.terminal.background);
        assert_eq!(config.theme.ui.bar_background, light.ui.bar_background);

        // テーマを選ばなければ既定のテーマに掛かる。最後の代入が有効
        let config = eval_ok(
            r##"kugutsu.opt.theme_overrides = { ui = { error = "#000001" } }
            kugutsu.opt.theme_overrides = { ui = { agent_done = "#000002" } }"##,
        );
        assert_eq!(config.theme.ui.agent_done, 0x000002);
        assert_eq!(config.theme.ui.error, Theme::default().ui.error);
    }

    #[test]
    fn theme_overrides_apply_even_if_the_theme_is_unknown() {
        let loaded = eval(
            r##"kugutsu.opt.theme = "nope"
            kugutsu.opt.theme_overrides = { ui = { error = "#000001" } }"##,
        );
        assert_eq!(loaded.errors.len(), 1);
        assert_eq!(loaded.config.theme.name, "kugutsu-dark");
        assert_eq!(loaded.config.theme.ui.error, 0x000001);
    }

    #[test]
    fn invalid_theme_overrides_are_errors() {
        for (value, expected) in [
            (
                "1",
                "kugutsu.opt.theme_overrides: expected a table, got number",
            ),
            (
                r#"{ ui = { error = "red" } }"#,
                "kugutsu.opt.theme_overrides: ui.error: expected a color",
            ),
            (
                "{ foo = {} }",
                "kugutsu.opt.theme_overrides: unknown key 'foo'",
            ),
            (
                r#"{ name = "x" }"#,
                "kugutsu.opt.theme_overrides: unknown key 'name'",
            ),
        ] {
            let config = eval_err(&format!("kugutsu.opt.theme_overrides = {value}"), expected);
            assert_eq!(config.theme, Theme::default());
        }
    }

    #[test]
    fn theme_overrides_can_be_read_back() {
        eval_ok("assert(kugutsu.opt.theme_overrides == nil)");
        eval_ok(
            r##"kugutsu.opt.theme_overrides = {
                terminal = { cursor = "#ABCDEF", ansi = { "#000000", "#000001", "#000002", "#000003",
                    "#000004", "#000005", "#000006", "#000007", "#000008", "#000009", "#00000a",
                    "#00000b", "#00000c", "#00000d", "#00000e", "#00000f" } },
                ui = { error = "#010203" },
            }
            local o = kugutsu.opt.theme_overrides
            assert(o.terminal.cursor == "#abcdef")
            assert(o.terminal.foreground == nil)
            assert(#o.terminal.ansi == 16 and o.terminal.ansi[16] == "#00000f")
            assert(o.ui.error == "#010203")
            assert(o.ui.bar_border == nil)
            -- 読み出した table を代入し直せる
            kugutsu.opt.theme_overrides = o"##,
        );
    }

    #[test]
    fn sets_background_transparency() {
        let config =
            eval_ok("kugutsu.opt.background_opacity = 0.85\nkugutsu.opt.background_blur = true");
        assert_eq!(config.background_opacity, 0.85);
        assert!(config.background_blur);
        assert_eq!(
            eval_ok("kugutsu.opt.background_opacity = 0").background_opacity,
            0.0
        );
        assert_eq!(
            eval_ok("kugutsu.opt.background_opacity = 1").background_opacity,
            1.0
        );
        eval_ok(
            "assert(kugutsu.opt.background_opacity == 1)\nassert(kugutsu.opt.background_blur == false)",
        );
    }

    #[test]
    fn sets_ui_font_and_inactive_pane_opacity() {
        let config = eval_ok(
            r#"kugutsu.opt.ui_font_family = "Inter"
            kugutsu.opt.inactive_pane_opacity = 0.6"#,
        );
        assert_eq!(config.ui_font_family.as_deref(), Some("Inter"));
        assert_eq!(config.inactive_pane_opacity, 0.6);
        eval_ok(
            r#"assert(kugutsu.opt.ui_font_family == nil)
            assert(math.abs(kugutsu.opt.inactive_pane_opacity - 0.85) < 1e-6)"#,
        );
        eval_err(
            r#"kugutsu.opt.ui_font_family = " ""#,
            "kugutsu.opt.ui_font_family: must not be empty",
        );
        eval_err(
            "kugutsu.opt.inactive_pane_opacity = 2",
            "kugutsu.opt.inactive_pane_opacity: expected a number between 0 and 1",
        );
    }

    #[test]
    fn sets_ui_item_width() {
        let config = eval_ok("kugutsu.ui.item_width = 200");
        assert_eq!(config.ui.item_width, 200.0);
        eval_ok("assert(kugutsu.ui.item_width == 150)");
        eval_ok(
            "kugutsu.ui.item_width = kugutsu.ui.item_width + 10\nassert(kugutsu.ui.item_width == 160)",
        );
    }

    #[test]
    fn invalid_ui_options_are_errors() {
        let config = eval_err(
            "kugutsu.ui.item_width = 0",
            "kugutsu.ui.item_width: expected a positive number, got 0",
        );
        assert_eq!(config.ui, UiConfig::default());
        eval_err(
            "kugutsu.ui.item_width = 'wide'",
            "kugutsu.ui.item_width: expected number, got string",
        );
        eval_err("kugutsu.ui.foo = 1", "unknown ui option 'foo'");
        eval_err("local x = kugutsu.ui.foo", "unknown ui option 'foo'");
    }

    #[test]
    fn invalid_background_transparency_is_an_error() {
        for value in ["1.5", "-0.1", "0/0", "math.huge"] {
            let config = eval_err(
                &format!("kugutsu.opt.background_opacity = {value}"),
                "kugutsu.opt.background_opacity: expected a number between 0 and 1",
            );
            assert_eq!(config.background_opacity, 1.0);
        }
        eval_err(
            "kugutsu.opt.background_opacity = 'half'",
            "kugutsu.opt.background_opacity: expected number, got string",
        );
        eval_err(
            "kugutsu.opt.background_blur = 1",
            "kugutsu.opt.background_blur: expected boolean, got number",
        );
    }

    #[test]
    fn errors_point_at_the_line_in_init_lua() {
        eval_err(
            "\n\nkugutsu.opt.font_size = 'x'",
            "init.lua:3: kugutsu.opt.font_size",
        );
    }

    #[test]
    fn values_set_before_an_error_still_apply() {
        let config = eval_err(
            "kugutsu.opt.font_size = 20\nkugutsu.opt.foo = 1\nkugutsu.opt.line_height = 2",
            "unknown option 'foo'",
        );
        assert_eq!(config.font_size, 20.0);
        assert_eq!(config.line_height, Config::default().line_height);
    }

    #[test]
    fn runtime_errors_are_reported() {
        let config = eval_err(
            "kugutsu.opt.font_size = 20\nerror('boom')",
            "init.lua:2: boom",
        );
        assert_eq!(config.font_size, 20.0);
    }

    #[test]
    fn syntax_errors_are_reported() {
        let config = eval_err("kugutsu.opt.font_size = 20\nthis is not lua", "init.lua:2:");
        assert_eq!(config, Config::default());
    }

    fn trigger(text: &str) -> Trigger {
        Trigger::parse(text).unwrap()
    }

    fn binding<'a>(config: &'a Config, key: &str) -> Option<&'a Binding> {
        config.keymap.lookup(&trigger(key))
    }

    fn lua_callback(config: &Config, key: &str) -> LuaCallback {
        match &binding(config, key).expect(key).action {
            Action::Lua(callback) => callback.clone(),
            other => panic!("{key} is not a Lua function: {other:?}"),
        }
    }

    #[test]
    fn default_keymap_is_used_without_mappings() {
        assert_eq!(eval_ok("").keymap, Keymap::default());
    }

    #[test]
    fn maps_a_key_to_a_named_action_with_a_description() {
        let config =
            eval_ok(r#"kugutsu.keymap.set("<prefix>t", "new_tab", { desc = "Open a tab" })"#);
        let binding = binding(&config, "<prefix>t").unwrap();
        assert_eq!(binding.action, Action::Command(Command::NewTab));
        assert_eq!(binding.desc, "Open a tab");
    }

    #[test]
    fn description_defaults_to_the_action_description() {
        let config = eval_ok(
            r#"kugutsu.keymap.set("<prefix>|", "split_horizontal")
            kugutsu.keymap.set("ctrl-shift-t", "new_tab", {})"#,
        );
        assert_eq!(
            binding(&config, "<prefix>|").unwrap().desc,
            "Split pane left / right"
        );
        assert_eq!(binding(&config, "ctrl-shift-t").unwrap().desc, "New tab");
        let config = eval_ok(r#"kugutsu.keymap.set("<prefix>w", function() end)"#);
        assert_eq!(binding(&config, "<prefix>w").unwrap().desc, "Lua function");
    }

    #[test]
    fn del_removes_a_mapping() {
        let config = eval_ok(r#"kugutsu.keymap.del("<prefix>x")"#);
        assert_eq!(binding(&config, "<prefix>x"), None);
        assert!(binding(&config, "<prefix>c").is_some());
    }

    #[test]
    fn invalid_mappings_are_errors() {
        let cases = [
            (
                r#"kugutsu.keymap.set(1, "new_tab")"#,
                "kugutsu.keymap.set: key: expected string, got number",
            ),
            (
                r#"kugutsu.keymap.set("<prefix>", "new_tab")"#,
                r#"kugutsu.keymap.set: invalid key "<prefix>""#,
            ),
            (
                r#"kugutsu.keymap.set("ctrl-", "new_tab")"#,
                r#"kugutsu.keymap.set: invalid key "ctrl-""#,
            ),
            (
                r#"kugutsu.keymap.set("<prefix>t", "nope")"#,
                "kugutsu.keymap.set: unknown action 'nope'",
            ),
            (
                r#"kugutsu.keymap.set("<prefix>t", 1)"#,
                "kugutsu.keymap.set: action: expected string or function, got number",
            ),
            (
                r#"kugutsu.keymap.set("<prefix>t", "new_tab", "x")"#,
                "kugutsu.keymap.set: opts: expected table, got string",
            ),
            (
                r#"kugutsu.keymap.set("<prefix>t", "new_tab", { desc = 1 })"#,
                "kugutsu.keymap.set: desc: expected string, got number",
            ),
            (
                r#"kugutsu.keymap.del("<prefix>z")"#,
                "kugutsu.keymap.del: no mapping for '<prefix>z'",
            ),
            (
                r#"kugutsu.keymap.del(true)"#,
                "kugutsu.keymap.del: key: expected string, got boolean",
            ),
        ];
        for (source, expected) in cases {
            let config = eval_err(source, &format!("init.lua:1: {expected}"));
            assert_eq!(config.keymap, Keymap::default(), "{source}");
        }
    }

    #[test]
    fn direct_mapping_of_the_prefix_is_an_error() {
        let config = eval_err(
            r#"kugutsu.keymap.set("ctrl-a", "new_tab")
            kugutsu.opt.prefix = "ctrl-a""#,
            "kugutsu.keymap: 'ctrl-a' is the prefix key and cannot be mapped without <prefix>",
        );
        assert_eq!(binding(&config, "ctrl-a"), None);
        assert_eq!(config.prefix, Keystroke::parse("ctrl-a").unwrap());
    }

    #[test]
    fn lua_function_returns_the_actions_it_calls() {
        let config = eval_ok(
            r#"kugutsu.keymap.set("<prefix>w", function()
                kugutsu.action.new_tab()
                kugutsu.action.split_horizontal()
                kugutsu.action.select_tab(3)
            end, { desc = "Tab with two panes" })"#,
        );
        assert_eq!(
            binding(&config, "<prefix>w").unwrap().desc,
            "Tab with two panes"
        );
        let callback = lua_callback(&config, "<prefix>w");
        let expected = vec![
            Command::NewTab,
            Command::Split(crate::panes::Axis::Horizontal),
            Command::SelectTab(2),
        ];
        assert_eq!(callback.call(), Ok(expected.clone()));
        // 何度呼んでも同じ
        assert_eq!(callback.call(), Ok(expected));
    }

    #[test]
    fn every_named_action_is_callable_from_lua() {
        for &(name, command) in Command::NAMED {
            let config = eval_ok(&format!(
                r#"kugutsu.keymap.set("<prefix>w", function() kugutsu.action.{name}() end)"#
            ));
            assert_eq!(
                lua_callback(&config, "<prefix>w").call(),
                Ok(vec![command]),
                "{name}"
            );
        }
    }

    #[test]
    fn lua_function_keeps_its_upvalues() {
        let config = eval_ok(
            r#"local count = 0
            kugutsu.keymap.set("<prefix>w", function()
                count = count + 1
                for _ = 1, count do kugutsu.action.next_tab() end
            end)"#,
        );
        let callback = lua_callback(&config, "<prefix>w");
        assert_eq!(callback.call(), Ok(vec![Command::NextTab]));
        assert_eq!(
            callback.call(),
            Ok(vec![Command::NextTab, Command::NextTab])
        );
    }

    #[test]
    fn actions_outside_a_mapping_are_errors() {
        eval_err(
            "kugutsu.action.new_tab()",
            "init.lua:1: kugutsu.action.new_tab: can only be called from a key mapping",
        );
    }

    #[test]
    fn errors_in_a_lua_function_are_returned() {
        let config = eval_ok(
            r#"kugutsu.keymap.set("<prefix>w", function()
                kugutsu.action.new_tab()
                error("boom")
            end)"#,
        );
        let callback = lua_callback(&config, "<prefix>w");
        let err = callback.call().unwrap_err();
        assert!(err.contains("init.lua:3: boom"), "{err}");
        assert!(!err.contains("traceback"), "{err}");

        for (arg, shown) in [("0", "0"), ("10", "10"), ("1.5", "1.5"), ("'x'", "string")] {
            let config = eval_ok(&format!(
                r#"kugutsu.keymap.set("<prefix>w", function() kugutsu.action.select_tab({arg}) end)"#
            ));
            let err = lua_callback(&config, "<prefix>w").call().unwrap_err();
            assert!(
                err.contains(
                    "init.lua:1: kugutsu.action.select_tab: expected an integer between 1 and 9"
                ),
                "{arg}: {err}"
            );
            assert!(err.contains(shown), "{arg}: {err}");
        }
    }

    fn spec_names(source: &str) -> (Vec<String>, Vec<String>) {
        let (specs, errors) = evaluate_spec(source, "init.lua", None);
        (specs.into_iter().map(|spec| spec.name).collect(), errors)
    }

    #[test]
    fn plugins_setup_declares_plugins() {
        let (names, errors) = spec_names(
            r#"kugutsu.plugins.setup({ "a/one.kugutsu", { "b/two.kugutsu", rev = "v1" } })"#,
        );
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(names, ["one.kugutsu", "two.kugutsu"]);
        assert_eq!(spec_names("").0, Vec::<String>::new());
    }

    #[test]
    fn last_plugins_setup_wins() {
        let (names, _) = spec_names(
            r#"kugutsu.plugins.setup({ "a/one" })
            kugutsu.plugins.setup({ "b/two" })"#,
        );
        assert_eq!(names, ["two"]);
    }

    #[test]
    fn plugins_setup_errors_point_at_the_call() {
        let (names, errors) = spec_names(
            r#"kugutsu.plugins.setup({ "a/one" })
            kugutsu.plugins.setup({ { "a/b", opt = {} } })"#,
        );
        assert_eq!(names, ["one"]);
        assert_eq!(
            errors,
            vec!["init.lua:2: kugutsu.plugins.setup: a/b: unknown key 'opt'".to_string()]
        );
    }

    #[test]
    fn require_returns_one_value_and_reports_the_calling_line() {
        let (names, errors) = spec_names(
            r#"package.preload["plugins.ui"] = function() return { "a/one" } end
            kugutsu.plugins.setup({ "b/two", require("plugins.ui") })"#,
        );
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(names, ["two", "one"]);
        let loaded = eval("\nrequire('nope')");
        assert!(
            loaded.errors[0].starts_with("init.lua:2: module 'nope' not found"),
            "{:?}",
            loaded.errors
        );
    }

    #[test]
    fn evaluate_spec_does_not_resolve_the_theme() {
        let (_, errors) = evaluate_spec(r#"kugutsu.opt.theme = "from-a-plugin""#, "init.lua", None);
        assert_eq!(errors, Vec::<String>::new());
    }

    #[test]
    fn error_messages_have_no_traceback() {
        let loaded = eval("error('boom')");
        assert!(
            !loaded.errors[0].contains("traceback"),
            "{:?}",
            loaded.errors
        );
    }

    #[test]
    fn runtime_outlives_init_lua_evaluation() {
        let loaded = eval("x = 41");
        let runtime = loaded.runtime.expect("runtime");
        let mut x = 0;
        runtime
            .call(|lua| {
                x = lua.globals().get("x")?;
                Ok(())
            })
            .unwrap();
        assert_eq!(x, 41);
    }

    #[test]
    fn runtime_call_errors_have_no_traceback() {
        let runtime = eval("").runtime.expect("runtime");
        let err = runtime
            .call(|lua| lua.load("error('boom')").set_name("=cb").exec())
            .unwrap_err();
        assert_eq!(err, "cb:1: boom");
    }
}
