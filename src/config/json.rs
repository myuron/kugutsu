//! `kugutsu.json`。JSON と Lua の値を相互に変換する。
//!
//! JSON の `null` は mlua の [`Value::NULL`]（`kugutsu.json.null`）にして、配列の長さを保つ。
//! 配列には mlua の array_metatable が付き、空でも `[]` に戻る。印の無い空の table は `{}` になる。

use mlua::{Lua, LuaSerdeExt, Table, Value};

use super::lua::lua_type_name;

/// `kugutsu.json.decode(text)`。
pub fn decode(lua: &Lua, text: &Value) -> Result<Value, String> {
    let error = |reason: String| format!("kugutsu.json.decode: {reason}");
    let Value::String(text) = text else {
        return Err(error(format!(
            "expected string, got {}",
            lua_type_name(text)
        )));
    };
    let json: serde_json::Value =
        serde_json::from_slice(&text.as_bytes()).map_err(|err| error(err.to_string()))?;
    lua.to_value(&json).map_err(|err| error(err.to_string()))
}

/// `kugutsu.json.encode(value)`。キーは並べ替えて、同じ table からは同じ文字列を作る。
pub fn encode(value: &Value) -> Result<String, String> {
    serde_json::to_string(&value.to_serializable().sort_keys(true))
        .map_err(|err| format!("kugutsu.json.encode: {err}"))
}

/// `kugutsu.json.array(t)`。`t` を空でも `[]` に encode する配列にして返す。
pub fn array(lua: &Lua, value: &Value) -> Result<Table, String> {
    let Value::Table(table) = value else {
        return Err(format!(
            "kugutsu.json.array: expected table, got {}",
            lua_type_name(value)
        ));
    };
    table
        .set_metatable(Some(lua.array_metatable()))
        .map_err(|err| format!("kugutsu.json.array: {err}"))?;
    Ok(table.clone())
}
