//! `init.lua` の評価。`kugutsu.opt` への代入を検証して [`Config`] に書き込む。

use std::cell::RefCell;
use std::rc::Rc;

use gpui::Keystroke;
use mlua::{Lua, Value};

use super::{Config, Loaded};

/// `kugutsu` グローバルを組み立てる Lua コード。`set` / `get` は Rust の関数で、
/// 不正な値ならエラーメッセージを返す。`error(..., 2)` で代入した行をエラー位置にする。
const PRELUDE: &str = r#"
local set, get = ...
local opt = setmetatable({}, {
    __newindex = function(_, key, value)
        local err = set(key, value)
        if err then error(err, 2) end
    end,
    __index = function(_, key)
        local value, err = get(key)
        if err then error(err, 2) end
        return value
    end,
})
kugutsu = { opt = opt }
"#;

/// Lua のソースを評価して設定を読み込む。`chunk_name` はエラーメッセージに出す名前。
/// エラーが起きてもそこまでに設定された値は残す。
pub fn evaluate(source: &str, chunk_name: &str) -> Loaded {
    let config = Rc::new(RefCell::new(Config::default()));
    let errors = match run(source, chunk_name, &config) {
        Ok(()) => Vec::new(),
        Err(err) => vec![error_message(&err)],
    };
    let config = config.borrow().clone();
    Loaded { config, errors }
}

fn run(source: &str, chunk_name: &str, config: &Rc<RefCell<Config>>) -> mlua::Result<()> {
    let lua = Lua::new();
    let target = Rc::clone(config);
    let set = lua.create_function(move |_, (key, value): (String, Value)| {
        Ok(set_option(&mut target.borrow_mut(), &key, &value).err())
    })?;
    let source_config = Rc::clone(config);
    let get = lua.create_function(move |lua, key: String| {
        Ok(match get_option(lua, &source_config.borrow(), &key) {
            Ok(value) => (value, None),
            Err(err) => (Value::Nil, Some(err)),
        })
    })?;
    lua.load(PRELUDE)
        .set_name("=prelude")
        .call::<()>((set, get))?;
    // `=` で始まる名前はそのままエラー位置に出る（`init.lua:3: ...`）
    lua.load(source).set_name(format!("={chunk_name}")).exec()
}

/// `kugutsu.opt.<key> = value` を検証して設定に書き込む。
fn set_option(config: &mut Config, key: &str, value: &Value) -> Result<(), String> {
    let invalid = |reason: &str| format!("kugutsu.opt.{key}: {reason}");
    match key {
        "prefix" => {
            let text = string(key, value)?;
            config.prefix = parse_keystroke(&text)
                .ok_or_else(|| invalid(&format!("invalid keystroke {text:?}")))?;
        }
        "font_family" => {
            let text = string(key, value)?;
            if text.trim().is_empty() {
                return Err(invalid("must not be empty"));
            }
            config.font_family = Some(text);
        }
        "font_size" => config.font_size = positive_number(key, value)?,
        "line_height" => config.line_height = positive_number(key, value)?,
        _ => return Err(unknown(key)),
    }
    Ok(())
}

/// `kugutsu.opt.<key>` の現在の値。
fn get_option(lua: &Lua, config: &Config, key: &str) -> Result<Value, String> {
    let string = |text: &str| {
        lua.create_string(text)
            .map(Value::String)
            .map_err(|err| err.to_string())
    };
    match key {
        "prefix" => string(&config.prefix.unparse()),
        "font_family" => config.font_family.as_deref().map_or(Ok(Value::Nil), string),
        "font_size" => Ok(Value::Number(config.font_size.into())),
        "line_height" => Ok(Value::Number(config.line_height.into())),
        _ => Err(unknown(key)),
    }
}

fn unknown(key: &str) -> String {
    format!("unknown option '{key}'")
}

fn type_error(key: &str, expected: &str, value: &Value) -> String {
    format!(
        "kugutsu.opt.{key}: expected {expected}, got {}",
        lua_type_name(value)
    )
}

/// Lua の `type()` と同じ型名（mlua は整数を `integer` と呼ぶが、Lua では `number`）。
pub(crate) fn lua_type_name(value: &Value) -> &'static str {
    match value {
        Value::Integer(_) => "number",
        _ => value.type_name(),
    }
}

fn string(key: &str, value: &Value) -> Result<String, String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(type_error(key, "string", value)),
    }
}

/// 有限で 0 より大きい数値。
fn positive_number(key: &str, value: &Value) -> Result<f32, String> {
    let number = match *value {
        Value::Integer(n) => n as f64,
        Value::Number(n) => n,
        _ => return Err(type_error(key, "number", value)),
    };
    if number.is_finite() && number > 0.0 {
        Ok(number as f32)
    } else {
        Err(format!(
            "kugutsu.opt.{key}: expected a positive number, got {number}"
        ))
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

    #[test]
    fn error_messages_have_no_traceback() {
        let loaded = eval("error('boom')");
        assert!(
            !loaded.errors[0].contains("traceback"),
            "{:?}",
            loaded.errors
        );
    }
}
