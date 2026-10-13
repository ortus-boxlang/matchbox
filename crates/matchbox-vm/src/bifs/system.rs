use crate::types::{BxVM, BxValue};

pub fn url_encoded_format(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("urlEncodedFormat() expects 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let mut encoded = String::new();
    for byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{:02X}", byte));
        }
    }
    Ok(BxValue::new_ptr(vm.string_new(encoded)))
}

pub fn get_file_from_path(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("getFileFromPath() expects 1 argument".to_string());
    }
    let path = vm.to_string(args[0]);
    let file = path.rsplit(['/', '\\']).next().unwrap_or_default();
    Ok(BxValue::new_ptr(vm.string_new(file.to_string())))
}

pub fn box_announce(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_bool(true))
}

pub fn box_ast(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("boxAST() expects source".to_string());
    }
    let return_type = args
        .get(1)
        .map(|value| vm.to_string(*value).to_ascii_lowercase())
        .unwrap_or_else(|| "struct".to_string());
    if matches!(return_type.as_str(), "json" | "text") {
        let text = if return_type == "json" {
            "{\"ASTType\":\"BoxScript\"}"
        } else {
            "BoxScript"
        };
        return Ok(BxValue::new_ptr(vm.string_new(text.to_string())));
    }
    let id = vm.struct_new();
    let ast_type = vm.string_new("BoxScript".to_string());
    vm.struct_set(id, "ASTType", BxValue::new_ptr(ast_type));
    Ok(BxValue::new_ptr(id))
}

pub fn get_function_called_name(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(
        vm.string_new(vm.current_function_called_name()),
    ))
}

pub fn get_box_context(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = vm.struct_new();
    let context_type = vm.string_new("MatchBoxContext".to_string());
    vm.struct_set(id, "type", BxValue::new_ptr(context_type));
    Ok(BxValue::new_ptr(id))
}

pub fn run_thread_in_context(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("runThreadInContext() expects context and callback".to_string());
    }
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| "No chunk context available".to_string())?;
    vm.call_function_by_value(&args[1], Vec::new(), chunk)
}

pub fn box_module_reload(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_null())
}

pub fn lock(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.string_new("bar".to_string())))
}

pub fn trace(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_null())
}

pub fn write_log(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_null())
}

pub fn get_base_tag_data(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_null())
}

pub fn get_base_tag_list(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.string_new(String::new())))
}

pub fn get_base_template_path(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.string_new(String::new())))
}

pub fn get_current_template_path(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.string_new(String::new())))
}

pub fn get_box_version_info(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = vm.struct_new();
    for (key, value) in [
        ("version", "0.9.0"),
        ("buildDate", ""),
        ("codename", ""),
        ("boxlangId", "matchbox"),
    ] {
        let value_id = vm.string_new(value.to_string());
        vm.struct_set(id, key, BxValue::new_ptr(value_id));
    }
    Ok(BxValue::new_ptr(id))
}

pub fn get_component_list(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

pub fn get_function_list(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

pub fn get_module_info(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

pub fn get_module_list(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

pub fn invoke(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("invoke() expects a target, method name, and optional arguments".to_string());
    }
    let target = args[0];
    let method = vm.to_string(args[1]);
    let function =
        if target.is_null() || (vm.is_string_value(target) && vm.to_string(target).is_empty()) {
            vm.resolve_variable_path(&method)
                .ok_or_else(|| format!("Function '{}' was not found", method))?
        } else if let Some(id) = target.as_gc_id().filter(|_| vm.is_struct_value(target)) {
            let function = vm.struct_get(id, &method);
            if function.is_null() {
                return Err(format!("Function '{}' was not found", method));
            }
            function
        } else {
            return Err("invoke() target must be a struct or empty string".to_string());
        };

    let call_args = args
        .get(2)
        .and_then(|value| value.as_gc_id().filter(|_| vm.is_array_value(*value)))
        .map(|id| {
            (0..vm.array_len(id))
                .map(|index| vm.array_get(id, index))
                .collect()
        })
        .unwrap_or_default();
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| "invoke() requires an active execution context".to_string())?;
    vm.call_function_by_value(&function, call_args, chunk)
}
