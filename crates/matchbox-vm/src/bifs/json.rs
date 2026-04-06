use crate::types::{BxVM, BxValue};
use serde_json::Value as JsonValue;

pub fn json_deserialize(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("JSONdeserialize() expects 1 argument".to_string());
    }
    let json_str = vm.to_string(args[0]);

    let json_val: JsonValue =
        serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    Ok(json_to_bx(vm, json_val))
}

pub fn json_serialize(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("JSONserialize() expects 1 argument".to_string());
    }
    let json_val = bx_to_json(vm, args[0]);

    let json_str = serde_json::to_string(&json_val)
        .map_err(|e| format!("Failed to serialize to JSON: {}", e))?;

    let s_id = vm.string_new(json_str);
    Ok(BxValue::new_ptr(s_id))
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
    if val.is_null() {
        JsonValue::Null
    } else if val.is_bool() {
        JsonValue::Bool(val.as_bool())
    } else if val.is_number() {
        JsonValue::Number(serde_json::Number::from_f64(val.as_number()).unwrap())
    } else if let Some(id) = val.as_gc_id() {
        // This is a bit tricky as BxVM doesn't expose a way to check object type easily
        // We assume to_string() works for strings, and we might need to handle arrays/structs
        // For now, let's use a heuristic or just to_string for non-containers

        let s = vm.to_string(val);
        // If it looks like a container, we might have a problem here without better VM introspection
        // But BxVM trait has array_len/struct_len

        // Let's try to detect if it's a struct or array
        let struct_len = vm.struct_len(id);
        if struct_len > 0 || vm.struct_get_shape(id) != vm.get_root_shape() {
            let mut map = serde_json::Map::new();
            for key in vm.struct_key_array(id) {
                let item = vm.struct_get(id, &key);
                map.insert(key, bx_to_json(vm, item));
            }
            return JsonValue::Object(map);
        }

        let array_len = vm.array_len(id);
        if array_len > 0 {
            let mut vec = Vec::new();
            for i in 0..array_len {
                let item = vm.array_get(id, i);
                vec.push(bx_to_json(vm, item));
            }
            return JsonValue::Array(vec);
        }

        JsonValue::String(s)
    } else {
        JsonValue::Null
    }
}
