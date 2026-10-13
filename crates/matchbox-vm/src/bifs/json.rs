use crate::types::{BxVM, BxValue};
use serde_json::Value as JsonValue;
use std::collections::HashSet;

pub fn json_deserialize(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("deserializeJSON() expects 1 argument".to_string());
    }
    let json_str = normalize_json_input(vm.to_string(args[0]));

    let json_val: JsonValue =
        serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    if args
        .get(1)
        .is_some_and(|value| value.is_bool() && !value.as_bool())
    {
        if let Some(query) = json_to_query(vm, &json_val) {
            return Ok(query);
        }
    }

    Ok(json_to_bx(vm, json_val))
}

pub fn json_serialize(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("serializeJSON() expects 1 argument".to_string());
    }
    let format = args
        .get(1)
        .map(|value| vm.to_string(*value).to_ascii_lowercase())
        .unwrap_or_else(|| "row".to_string());
    let json_val = query_to_json(vm, args[0], &format).unwrap_or_else(|| bx_to_json(vm, args[0]));

    let json_str = serde_json::to_string(&json_val)
        .map_err(|e| format!("Failed to serialize to JSON: {}", e))?;

    let s_id = vm.string_new(json_str);
    Ok(BxValue::new_ptr(s_id))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn query_to_json(vm: &dyn BxVM, value: BxValue, format: &str) -> Option<JsonValue> {
    let id = value.as_gc_id()?;
    let query = vm.native_object_query_result(id)?;
    let columns = query
        .columns
        .iter()
        .map(|column| JsonValue::String(column.name.clone()))
        .collect::<Vec<_>>();
    let rows = query
        .rows
        .iter()
        .map(|row| JsonValue::Array(row.iter().cloned().map(sql_to_json).collect::<Vec<_>>()))
        .collect::<Vec<_>>();

    match format {
        "column" | "true" | "yes" => {
            let mut data = serde_json::Map::new();
            for (column_index, column) in query.columns.iter().enumerate() {
                let values = query
                    .rows
                    .iter()
                    .map(|row| {
                        row.get(column_index)
                            .cloned()
                            .map(sql_to_json)
                            .unwrap_or(JsonValue::Null)
                    })
                    .collect();
                data.insert(column.name.clone(), JsonValue::Array(values));
            }
            Some(JsonValue::Object(
                [
                    ("rowCount".to_string(), JsonValue::from(query.rows.len())),
                    ("columns".to_string(), JsonValue::Array(columns)),
                    ("data".to_string(), JsonValue::Object(data)),
                ]
                .into_iter()
                .collect(),
            ))
        }
        "struct" => Some(JsonValue::Array(
            query
                .rows
                .iter()
                .map(|row| {
                    let mut object = serde_json::Map::new();
                    for (index, column) in query.columns.iter().enumerate() {
                        object.insert(
                            column.name.clone(),
                            row.get(index)
                                .cloned()
                                .map(sql_to_json)
                                .unwrap_or(JsonValue::Null),
                        );
                    }
                    JsonValue::Object(object)
                })
                .collect(),
        )),
        _ => Some(JsonValue::Object(
            [
                ("columns".to_string(), JsonValue::Array(columns)),
                ("data".to_string(), JsonValue::Array(rows)),
            ]
            .into_iter()
            .collect(),
        )),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn json_to_query(vm: &mut dyn BxVM, value: &JsonValue) -> Option<BxValue> {
    use crate::datasource::{
        BxQuery,
        traits::{QueryColumn, QueryColumnType, QueryResult},
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    let object = value.as_object()?;
    let columns = object.get("columns")?.as_array()?;
    let data = object.get("data")?.as_object()?;
    let column_names = columns
        .iter()
        .map(JsonValue::as_str)
        .collect::<Option<Vec<_>>>()?;
    let row_count = object
        .get("rowCount")
        .and_then(JsonValue::as_u64)
        .map(|count| count as usize)
        .unwrap_or_else(|| {
            column_names
                .iter()
                .filter_map(|name| data.get(*name).and_then(JsonValue::as_array).map(Vec::len))
                .max()
                .unwrap_or(0)
        });
    let columns = column_names
        .iter()
        .map(|name| QueryColumn {
            name: (*name).to_string(),
            col_type: QueryColumnType::Other("json".to_string()),
        })
        .collect::<Vec<_>>();
    let rows = (0..row_count)
        .map(|row| {
            column_names
                .iter()
                .map(|name| {
                    data.get(*name)
                        .and_then(JsonValue::as_array)
                        .and_then(|values| values.get(row))
                        .and_then(json_to_sql)
                        .unwrap_or(crate::datasource::traits::SqlValue::Null)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let query = BxQuery::from_result(QueryResult { columns, rows });
    let id = vm.native_object_new(Rc::new(RefCell::new(query)));
    Some(BxValue::new_ptr(id))
}

#[cfg(target_arch = "wasm32")]
fn json_to_query(_vm: &mut dyn BxVM, _value: &JsonValue) -> Option<BxValue> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn json_to_sql(value: &JsonValue) -> Option<crate::datasource::traits::SqlValue> {
    use crate::datasource::traits::SqlValue;

    Some(match value {
        JsonValue::Null => SqlValue::Null,
        JsonValue::Bool(value) => SqlValue::Bool(*value),
        JsonValue::Number(value) => value
            .as_i64()
            .map(SqlValue::Int)
            .or_else(|| value.as_f64().map(SqlValue::Float))?,
        JsonValue::String(value) => SqlValue::Text(value.clone()),
        _ => return None,
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn query_to_json(_vm: &dyn BxVM, _value: BxValue, _format: &str) -> Option<JsonValue> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn sql_to_json(value: crate::datasource::traits::SqlValue) -> JsonValue {
    use crate::datasource::traits::SqlValue;

    match value {
        SqlValue::Null => JsonValue::Null,
        SqlValue::Bool(value) => JsonValue::Bool(value),
        SqlValue::Int(value) => JsonValue::from(value),
        SqlValue::Float(value) => serde_json::Number::from_f64(value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        SqlValue::Text(value) => JsonValue::String(value),
        SqlValue::Bytes(value) => {
            JsonValue::Array(value.into_iter().map(JsonValue::from).collect())
        }
    }
}

pub fn is_json(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("isJSON() expects 1 argument".to_string());
    }
    let json_str = normalize_json_input(vm.to_string(args[0]));
    Ok(BxValue::new_bool(
        serde_json::from_str::<JsonValue>(&json_str).is_ok(),
    ))
}

pub fn load_properties(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("loadProperties() expects 1 argument".to_string());
    }
    let content = vm.to_string(args[0]);

    let struct_id = vm.struct_new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            let k = key.trim();
            let v = value.trim();
            let v_id = vm.string_new(v.to_string());
            vm.struct_set(struct_id, k, BxValue::new_ptr(v_id));
        }
    }

    Ok(BxValue::new_ptr(struct_id))
}

fn json_to_bx(vm: &mut dyn BxVM, val: JsonValue) -> BxValue {
    match val {
        JsonValue::Null => BxValue::new_null(),
        JsonValue::Bool(b) => BxValue::new_bool(b),
        JsonValue::Number(n) => BxValue::new_number(n.as_f64().unwrap_or(0.0)),
        JsonValue::String(s) => BxValue::new_ptr(vm.string_new(s)),
        JsonValue::Array(arr) => {
            let id = vm.array_new();
            for item in arr {
                let bx_item = json_to_bx(vm, item);
                vm.array_push(id, bx_item);
            }
            BxValue::new_ptr(id)
        }
        JsonValue::Object(obj) => {
            let id = vm.struct_new();
            for (key, value) in obj {
                let bx_val = json_to_bx(vm, value);
                vm.struct_set(id, &key, bx_val);
            }
            BxValue::new_ptr(id)
        }
    }
}

fn bx_to_json(vm: &dyn BxVM, val: BxValue) -> JsonValue {
    bx_to_json_inner(vm, val, &mut HashSet::new())
}

fn bx_to_json_inner(vm: &dyn BxVM, val: BxValue, seen: &mut HashSet<usize>) -> JsonValue {
    if val.is_null() {
        JsonValue::Null
    } else if val.is_bool() {
        JsonValue::Bool(val.as_bool())
    } else if val.is_number() {
        let number = val.as_number();
        if number.is_finite()
            && number.fract() == 0.0
            && number >= i64::MIN as f64
            && number <= i64::MAX as f64
        {
            JsonValue::Number(serde_json::Number::from(number as i64))
        } else {
            JsonValue::Number(serde_json::Number::from_f64(number).unwrap())
        }
    } else if vm
        .type_name_from_value(val)
        .is_some_and(|name| name.eq_ignore_ascii_case("datetime"))
    {
        let value = vm.to_string(val);
        let value = if let Some(prefix) = value.strip_suffix('Z') {
            if let Some(dot) = prefix.rfind('.') {
                let fraction = &prefix[dot + 1..];
                if fraction.chars().all(|ch| ch == '0') {
                    format!("{}Z", &prefix[..dot])
                } else {
                    format!("{}{}Z", &prefix[..dot + 1], fraction.trim_end_matches('0'))
                }
            } else {
                value
            }
        } else {
            value
        };
        JsonValue::String(value.to_string())
    } else if vm.is_struct_value(val) {
        if let Some(id) = val.as_gc_id() {
            if !seen.insert(id) {
                return JsonValue::Null;
            }
            let mut map = serde_json::Map::new();
            for key in vm.struct_key_array(id) {
                let item = vm.struct_get(id, &key);
                map.insert(key, bx_to_json_inner(vm, item, seen));
            }
            seen.remove(&id);
            JsonValue::Object(map)
        } else {
            JsonValue::Null
        }
    } else if vm.is_array_value(val) {
        if let Some(id) = val.as_gc_id() {
            if !seen.insert(id) {
                return JsonValue::Null;
            }
            let mut vec = Vec::new();
            for i in 0..vm.array_len(id) {
                let item = vm.array_get(id, i);
                vec.push(bx_to_json_inner(vm, item, seen));
            }
            seen.remove(&id);
            JsonValue::Array(vec)
        } else {
            JsonValue::Null
        }
    } else if let Some(id) = val.as_gc_id() {
        JsonValue::String(vm.to_string(BxValue::new_ptr(id)))
    } else {
        JsonValue::Null
    }
}

fn normalize_json_input(input: String) -> String {
    input.trim_start_matches('\u{feff}').to_string()
}
