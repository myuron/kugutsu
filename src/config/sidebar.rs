//! `kugutsu.sidebar`。右サイドバーのセクションを Lua から書き換える。
//!
//! `kugutsu.action` と違ってキューに積まず、その場で [`Sidebar`] に書き込む。

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, Table, Value};

use super::lua::lua_type_name;
use crate::sidebar::{Section, Sidebar};

/// Lua の関数とビューが共有するサイドバー。Lua の値は持たないので循環参照にならない。
pub type SharedSidebar = Rc<RefCell<Sidebar>>;

/// id だけを取る関数（`del` / `toggle` / `show` / `hide`）。
type Change = fn(&mut Sidebar, &str) -> bool;

/// `kugutsu.sidebar.*` の Rust 側の関数を名前で引ける table。
/// どれも不正な引数ならエラーメッセージを返す（PRELUDE でエラーにする）。
pub fn functions(lua: &Lua, sidebar: &SharedSidebar) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let target = Rc::clone(sidebar);
    let set = lua.create_function(move |_, (id, opts): (Value, Value)| {
        Ok(parse_section(&id, &opts)
            .map(|section| target.borrow_mut().set(section))
            .map_err(|reason| format!("kugutsu.sidebar.set: {reason}"))
            .err())
    })?;
    table.set("set", set)?;
    let changes: [(&str, Change); 4] = [
        ("del", Sidebar::del),
        ("toggle", Sidebar::toggle),
        ("show", Sidebar::show),
        ("hide", Sidebar::hide),
    ];
    for (name, change) in changes {
        let target = Rc::clone(sidebar);
        let function = lua.create_function(move |_, id: Value| {
            Ok(apply(&mut target.borrow_mut(), &id, change)
                .map_err(|reason| format!("kugutsu.sidebar.{name}: {reason}"))
                .err())
        })?;
        table.set(name, function)?;
    }
    Ok(table)
}

fn apply(sidebar: &mut Sidebar, id: &Value, change: Change) -> Result<(), String> {
    let id = section_id(id)?;
    if change(sidebar, &id) {
        Ok(())
    } else {
        Err(format!("no section '{id}'"))
    }
}

/// 空でない文字列の id。
fn section_id(id: &Value) -> Result<String, String> {
    let expected = "id: expected a non-empty string";
    match id {
        Value::String(text) if text.as_bytes().is_empty() => {
            Err(format!("{expected}, got an empty string"))
        }
        Value::String(text) => Ok(text.to_string_lossy()),
        other => Err(format!("{expected}, got {}", lua_type_name(other))),
    }
}

/// `kugutsu.sidebar.set(id, { title = ..., lines = { ... } })` の引数。
fn parse_section(id: &Value, opts: &Value) -> Result<Section, String> {
    let id = section_id(id)?;
    let error = |reason: String| format!("{id}: {reason}");
    let Value::Table(opts) = opts else {
        return Err(error(format!(
            "expected a table, got {}",
            lua_type_name(opts)
        )));
    };
    for pair in opts.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|err| error(err.to_string()))?;
        match key {
            Value::String(ref name) if matches!(&*name.as_bytes(), b"title" | b"lines") => {}
            other => return Err(error(format!("unknown key '{}'", key_text(&other)))),
        }
    }
    let title = match field(opts, "title").map_err(error)? {
        Value::String(text) => text.to_string_lossy(),
        other => {
            return Err(error(format!(
                "title: expected string, got {}",
                lua_type_name(&other)
            )));
        }
    };
    let lines = lines(&field(opts, "lines").map_err(error)?).map_err(error)?;
    Ok(Section { id, title, lines })
}

/// 必須のフィールド。
fn field(opts: &Table, name: &str) -> Result<Value, String> {
    match opts.raw_get::<Value>(name).map_err(|err| err.to_string())? {
        Value::Nil => Err(format!("{name}: missing")),
        value => Ok(value),
    }
}

/// 文字列の list。
fn lines(value: &Value) -> Result<Vec<String>, String> {
    let expected = "lines: expected a list of strings";
    let Value::Table(list) = value else {
        return Err(format!("{expected}, got {}", lua_type_name(value)));
    };
    let len = list.raw_len();
    for pair in list.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|err| err.to_string())?;
        let in_range = matches!(key, Value::Integer(i) if i >= 1 && i as usize <= len);
        if !in_range {
            return Err(format!(
                "{expected}, got a table with key '{}'",
                key_text(&key)
            ));
        }
    }
    (1..=len)
        .map(|index| match list.raw_get::<Value>(index) {
            Ok(Value::String(text)) => Ok(text.to_string_lossy()),
            Ok(other) => Err(format!(
                "lines[{index}]: expected string, got {}",
                lua_type_name(&other)
            )),
            Err(err) => Err(err.to_string()),
        })
        .collect()
}

/// エラーメッセージに出す table のキー。
fn key_text(key: &Value) -> String {
    match key {
        Value::String(text) => text.to_string_lossy(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        other => lua_type_name(other).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::config::{Loaded, evaluate};
    use crate::sidebar::Section;

    fn eval(source: &str) -> Loaded {
        evaluate(source, "init.lua")
    }

    fn sections(loaded: &Loaded) -> Vec<Section> {
        let runtime = loaded.runtime.as_ref().expect("runtime");
        runtime.sidebar().borrow().visible().cloned().collect()
    }

    fn section(id: &str, title: &str, lines: &[&str]) -> Section {
        Section {
            id: id.into(),
            title: title.into(),
            lines: lines.iter().map(|line| line.to_string()).collect(),
        }
    }

    #[test]
    fn set_adds_and_replaces_sections() {
        let loaded = eval(
            r#"kugutsu.sidebar.set("usage", { title = "Usage", lines = { "$1.00" } })
            kugutsu.sidebar.set("git", { title = "Git", lines = {} })
            kugutsu.sidebar.set("usage", { title = "Usage", lines = { "$2.00", "today" } })"#,
        );
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(
            sections(&loaded),
            [
                section("usage", "Usage", &["$2.00", "today"]),
                section("git", "Git", &[]),
            ]
        );
    }

    #[test]
    fn del_toggle_show_and_hide_change_the_sections() {
        let loaded = eval(
            r#"kugutsu.sidebar.set("a", { title = "A", lines = {} })
            kugutsu.sidebar.set("b", { title = "B", lines = {} })
            kugutsu.sidebar.set("c", { title = "C", lines = {} })
            kugutsu.sidebar.del("a")
            kugutsu.sidebar.toggle("b")
            kugutsu.sidebar.hide("c")
            kugutsu.sidebar.show("c")"#,
        );
        assert_eq!(loaded.errors, Vec::<String>::new());
        assert_eq!(sections(&loaded), [section("c", "C", &[])]);
        let runtime = loaded.runtime.as_ref().unwrap();
        assert_eq!(
            runtime.sidebar().borrow().hidden_ids(),
            HashSet::from(["b".to_string()])
        );
    }

    #[test]
    fn can_be_called_from_a_mapped_function() {
        let loaded = eval(
            r#"kugutsu.keymap.set("<prefix>w", function()
                kugutsu.sidebar.set("a", { title = "A", lines = { "x" } })
            end)"#,
        );
        assert_eq!(sections(&loaded), []);
        let trigger = crate::tabs::prefix::Trigger::parse("<prefix>w").unwrap();
        let crate::tabs::prefix::Action::Lua(callback) =
            &loaded.config.keymap.lookup(&trigger).unwrap().action
        else {
            panic!("not a Lua function");
        };
        assert_eq!(callback.call(), Ok(vec![]));
        assert_eq!(sections(&loaded), [section("a", "A", &["x"])]);
    }

    /// エラーが 1 つだけ起き、`init.lua:1: {expected}` を含み、セクションは変わらない。
    fn assert_error(source: &str, expected: &str) {
        let source =
            format!(r#"kugutsu.sidebar.set("keep", {{ title = "Keep", lines = {{}} }}) {source}"#);
        let loaded = eval(&source);
        assert_eq!(loaded.errors.len(), 1, "{source}: {:?}", loaded.errors);
        let expected = format!("init.lua:1: {expected}");
        assert!(
            loaded.errors[0].contains(&expected),
            "{source}: {:?} does not contain {expected:?}",
            loaded.errors[0]
        );
        assert_eq!(
            sections(&loaded),
            [section("keep", "Keep", &[])],
            "{source}"
        );
    }

    #[test]
    fn invalid_sections_are_errors() {
        for (source, expected) in [
            (
                r#"kugutsu.sidebar.set(1, { title = "A", lines = {} })"#,
                "kugutsu.sidebar.set: id: expected a non-empty string, got number",
            ),
            (
                r#"kugutsu.sidebar.set("", { title = "A", lines = {} })"#,
                "kugutsu.sidebar.set: id: expected a non-empty string, got an empty string",
            ),
            (
                r#"kugutsu.sidebar.set("a", "x")"#,
                "kugutsu.sidebar.set: a: expected a table, got string",
            ),
            (
                r#"kugutsu.sidebar.set("a", { lines = {} })"#,
                "kugutsu.sidebar.set: a: title: missing",
            ),
            (
                r#"kugutsu.sidebar.set("a", { title = 1, lines = {} })"#,
                "kugutsu.sidebar.set: a: title: expected string, got number",
            ),
            (
                r#"kugutsu.sidebar.set("a", { title = "A" })"#,
                "kugutsu.sidebar.set: a: lines: missing",
            ),
            (
                r#"kugutsu.sidebar.set("a", { title = "A", lines = "x" })"#,
                "kugutsu.sidebar.set: a: lines: expected a list of strings, got string",
            ),
            (
                r#"kugutsu.sidebar.set("a", { title = "A", lines = { "x", 2 } })"#,
                "kugutsu.sidebar.set: a: lines[2]: expected string, got number",
            ),
            (
                r#"kugutsu.sidebar.set("a", { title = "A", lines = { "x", n = "y" } })"#,
                "kugutsu.sidebar.set: a: lines: expected a list of strings, got a table with key 'n'",
            ),
            (
                r#"kugutsu.sidebar.set("a", { title = "A", lines = {}, color = "red" })"#,
                "kugutsu.sidebar.set: a: unknown key 'color'",
            ),
            (
                r#"kugutsu.sidebar.del("nope")"#,
                "kugutsu.sidebar.del: no section 'nope'",
            ),
            (
                r#"kugutsu.sidebar.toggle("nope")"#,
                "kugutsu.sidebar.toggle: no section 'nope'",
            ),
            (
                r#"kugutsu.sidebar.show("nope")"#,
                "kugutsu.sidebar.show: no section 'nope'",
            ),
            (
                r#"kugutsu.sidebar.hide(1)"#,
                "kugutsu.sidebar.hide: id: expected a non-empty string, got number",
            ),
        ] {
            assert_error(source, expected);
        }
    }
}
