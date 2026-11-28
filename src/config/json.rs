//! `kugutsu.json`。JSON と Lua の値を相互に変換する。
//!
//! JSON の `null` は mlua の [`Value::NULL`]（`kugutsu.json.null`）にして、配列の長さを保つ。
//! 配列には mlua の array_metatable が付き、空でも `[]` に戻る。印の無い空の table は `{}` になる。

use std::collections::{BTreeMap, HashSet};
use std::ffi::c_void;

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
///
/// mlua の serializer は NaN を `null` にし、配列とみなした table の残りのキーを捨てるので、
/// 黙って値が変わらないよう自前で変換する。
pub fn encode(lua: &Lua, value: &Value) -> Result<String, String> {
    let error = |reason: String| format!("kugutsu.json.encode: {reason}");
    let mut encoder = Encoder {
        array_metatable: lua.array_metatable(),
        path: HashSet::new(),
    };
    let json = encoder.value(value, 0).map_err(error)?;
    serde_json::to_string(&json).map_err(|err| error(err.to_string()))
}

/// これより深く入れ子になった table はエラーにする（スタックを使い切らないため）。
const MAX_DEPTH: usize = 128;

struct Encoder {
    array_metatable: Table,
    /// いま変換している table の祖先。同じ table が祖先にあれば循環している。
    path: HashSet<*const c_void>,
}

impl Encoder {
    fn value(&mut self, value: &Value, depth: usize) -> Result<serde_json::Value, String> {
        Ok(match value {
            Value::Nil => serde_json::Value::Null,
            Value::LightUserData(_) if value.is_null() => serde_json::Value::Null,
            Value::Boolean(b) => (*b).into(),
            Value::Integer(i) => (*i).into(),
            Value::Number(n) => serde_json::Number::from_f64(*n)
                .ok_or("cannot encode NaN or infinity")?
                .into(),
            Value::String(text) => text
                .to_str()
                .map_err(|_| "cannot encode a string that is not valid UTF-8")?
                .to_string()
                .into(),
            Value::Table(table) => {
                if depth >= MAX_DEPTH {
                    return Err("nested too deeply".to_string());
                }
                let pointer = table.to_pointer();
                if !self.path.insert(pointer) {
                    return Err("cannot encode a table that contains itself".to_string());
                }
                let json = self.table(table, depth + 1);
                self.path.remove(&pointer);
                json?
            }
            other => return Err(format!("cannot encode a {}", lua_type_name(other))),
        })
    }

    fn table(&mut self, table: &Table, depth: usize) -> Result<serde_json::Value, String> {
        let pairs = table
            .pairs::<Value, Value>()
            .collect::<mlua::Result<Vec<_>>>()
            .map_err(|err| err.to_string())?;
        let marked = table.metatable().as_ref() == Some(&self.array_metatable);
        let len = if marked {
            let len = table.raw_len();
            if let Some((key, _)) = pairs
                .iter()
                .find(|(key, _)| array_index(key, len).is_none())
            {
                return Err(format!(
                    "array has a key outside 1..#t: {}",
                    describe_key(key)
                ));
            }
            Some(len)
        } else {
            unmarked_array_len(&pairs)
        };
        if let Some(len) = len {
            let mut items = vec![serde_json::Value::Null; len];
            for (key, value) in &pairs {
                let index = array_index(key, len).expect("checked above");
                items[index - 1] = self.value(value, depth)?;
            }
            return Ok(items.into());
        }
        let mut entries = BTreeMap::new();
        for (key, value) in &pairs {
            let name = object_key(key)?;
            let value = self.value(value, depth)?;
            if entries.insert(name, value).is_some() {
                return Err(format!("duplicate key {}", describe_key(key)));
            }
        }
        // serde_json の Map は挿入順を保つので、並べ替えた順に入れる
        Ok(serde_json::Value::Object(entries.into_iter().collect()))
    }
}

/// 印の無い table を配列にするときの長さ。空ならオブジェクト（`{}`）にする。
/// キーがすべて正の整数で、極端に疎でなければ配列にし、隙間は `null` にする。
fn unmarked_array_len(pairs: &[(Value, Value)]) -> Option<usize> {
    let max = pairs
        .iter()
        .map(|(key, _)| match key {
            Value::Integer(i) if *i >= 1 => usize::try_from(*i).ok(),
            _ => None,
        })
        .try_fold(0, |max, index| index.map(|index| max.max(index)))?;
    (max > 0 && (max < 10 || pairs.len() * 2 >= max)).then_some(max)
}

/// `key` が長さ `len` の配列の添字（1 始まり）ならその値。
fn array_index(key: &Value, len: usize) -> Option<usize> {
    match key {
        Value::Integer(i) => usize::try_from(*i).ok().filter(|i| (1..=len).contains(i)),
        _ => None,
    }
}

/// オブジェクトのキー。数値のキーは文字列にする。
fn object_key(key: &Value) -> Result<String, String> {
    match key {
        Value::String(text) => text
            .to_str()
            .map(|text| text.to_string())
            .map_err(|_| "cannot encode a key that is not valid UTF-8".to_string()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(format!(
            "cannot encode a table key of type {}",
            lua_type_name(other)
        )),
    }
}

/// エラーメッセージに出すキー。文字列は引用符で囲む。
fn describe_key(key: &Value) -> String {
    match key {
        Value::String(text) => format!("{:?}", text.to_string_lossy()),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        other => format!("<{}>", lua_type_name(other)),
    }
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
