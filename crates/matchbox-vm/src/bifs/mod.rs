use std::collections::HashMap;
use crate::types::{BxValue, BxVM, BxNativeFunction};
use std::time::{SystemTime, UNIX_EPOCH};
use rand::RngExt;
use chrono::Local;
use std::io::{self, Write};
use uuid::Uuid;

mod jni;

pub fn register_all() -> HashMap<String, BxNativeFunction> {
    let mut bifs = HashMap::new();

    // Math BIFs
    bifs.insert("round".to_string(), round as BxNativeFunction);
    bifs.insert("randrange".to_string(), rand_range as BxNativeFunction);

    // Array BIFs
    bifs.insert("arrayappend".to_string(), array_append as BxNativeFunction);
    bifs.insert("arraynew".to_string(), array_new as BxNativeFunction);
    bifs.insert("arraypop".to_string(), array_pop_bif as BxNativeFunction);
    bifs.insert("arraydeleteat".to_string(), array_delete_at_bif as BxNativeFunction);
    bifs.insert("arrayinsertat".to_string(), array_insert_at_bif as BxNativeFunction);
    bifs.insert("arrayclear".to_string(), array_clear_bif as BxNativeFunction);
    bifs.insert("arrayset".to_string(), array_set_bif as BxNativeFunction);

    // Struct BIFs
    bifs.insert("structnew".to_string(), struct_new as BxNativeFunction);
    bifs.insert("structinsert".to_string(), struct_set_bif as BxNativeFunction);
    bifs.insert("structupdate".to_string(), struct_set_bif as BxNativeFunction);
    bifs.insert("structdelete".to_string(), struct_delete_bif as BxNativeFunction);
    bifs.insert("structkeyexists".to_string(), struct_key_exists_bif as BxNativeFunction);
    bifs.insert("structget".to_string(), struct_get_bif as BxNativeFunction);
    bifs.insert("structkeyarray".to_string(), struct_key_array_bif as BxNativeFunction);
    bifs.insert("structclear".to_string(), struct_clear_bif as BxNativeFunction);
    bifs.insert("structcount".to_string(), len as BxNativeFunction);

    // Core BIFs
    bifs.insert("len".to_string(), len as BxNativeFunction);
    bifs.insert("createobject".to_string(), create_object as BxNativeFunction);
    bifs.insert("ucase".to_string(), ucase as BxNativeFunction);
    bifs.insert("lcase".to_string(), lcase as BxNativeFunction);
    bifs.insert("futureonerror".to_string(), future_on_error as BxNativeFunction);

    // System BIFs
    bifs.insert("createuuid".to_string(), create_uuid as BxNativeFunction);
    bifs.insert("createguid".to_string(), create_guid as BxNativeFunction);
    bifs.insert("getsystemsetting".to_string(), get_system_setting as BxNativeFunction);

    // Date/Time BIFs
    bifs.insert("now".to_string(), now as BxNativeFunction);
    bifs.insert("gettickcount".to_string(), get_tick_count as BxNativeFunction);
    bifs.insert("sleep".to_string(), sleep as BxNativeFunction);
    bifs.insert("yield".to_string(), bx_yield as BxNativeFunction);

    // CLI BIFs
    bifs.insert("cliclear".to_string(), cli_clear as BxNativeFunction);
    bifs.insert("cliexit".to_string(), cli_exit as BxNativeFunction);
    bifs.insert("cligetargs".to_string(), cli_get_args as BxNativeFunction);
    bifs.insert("cliread".to_string(), cli_read as BxNativeFunction);
    bifs.insert("cliconfirm".to_string(), cli_confirm as BxNativeFunction);

    // Async BIFs
    bifs.insert("runasync".to_string(), run_async as BxNativeFunction);

    bifs
}

// --- Implementation ---

fn future_on_error(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 { return Err("onError() expects 2 arguments: (future, callback)".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        vm.future_on_error(id, args[1]);
        Ok(args[0])
    } else {
        Err("First argument to onError must be a future".to_string())
    }
}

fn ucase(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 { return Err("ucase() expects exactly 1 argument".to_string()); }
    let s = vm.to_string(args[0]).to_uppercase();
    Ok(BxValue::new_ptr(vm.string_new(s)))
}

fn lcase(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 { return Err("lcase() expects exactly 1 argument".to_string()); }
    let s = vm.to_string(args[0]).to_lowercase();
    Ok(BxValue::new_ptr(vm.string_new(s)))
}

fn round(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 { return Err("round() expects exactly 1 argument".to_string()); }
    if args[0].is_number() {
        Ok(BxValue::new_number(args[0].as_number().round()))
    } else {
        Err("round() expects a number".to_string())
    }
}

fn rand_range(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 { return Err("randRange() expects exactly 2 arguments".to_string()); }
    if args[0].is_number() && args[1].is_number() {
        let mut rng = rand::rng();
        let val = rng.random_range((args[0].as_number() as i64)..=(args[1].as_number() as i64));
        Ok(BxValue::new_number(val as f64))
    } else {
        Err("randRange() expects numbers".to_string())
    }
}

fn len(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 { return Err("len() expects exactly 1 argument".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        Ok(BxValue::new_number(vm.get_len(id) as f64))
    } else {
        Err("len() expects a string, array, or struct".to_string())
    }
}

// --- System BIFs ---

fn create_uuid(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = Uuid::new_v4().to_string().to_uppercase();
    Ok(BxValue::new_ptr(vm.string_new(id)))
}

fn create_guid(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = Uuid::new_v4().to_string().to_uppercase();
    Ok(BxValue::new_ptr(vm.string_new(id)))
}

fn get_system_setting(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() { return Err("getSystemSetting() expects at least 1 argument".to_string()); }
    let key = vm.to_string(args[0]);
    
    match std::env::var(&key) {
        Ok(val) => Ok(BxValue::new_ptr(vm.string_new(val))),
        Err(_) => {
            if args.len() > 1 {
                Ok(args[1])
            } else {
                Ok(BxValue::new_null())
            }
        }
    }
}

// --- Array BIFs ---

fn array_append(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 { return Err("arrayAppend() expects exactly 2 arguments".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        vm.array_push(id, args[1]);
        Ok(BxValue::new_bool(true))
    } else {
        Err("arrayAppend() expects an array as the first argument".to_string())
    }
}

fn array_new(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.array_new()))
}

fn array_pop_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() { return Err("arrayPop() expects 1 argument".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        vm.array_pop(id)
    } else {
        Err("arrayPop() expects an array".to_string())
    }
}

fn array_delete_at_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 { return Err("arrayDeleteAt() expects 2 arguments: (array, index)".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 { return Err("Array index must be 1-based".to_string()); }
        vm.array_delete_at(id, idx - 1)
    } else {
        Err("arrayDeleteAt() expects an array as the first argument".to_string())
    }
}

fn array_insert_at_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 { return Err("arrayInsertAt() expects 3 arguments: (array, index, value)".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 { return Err("Array index must be 1-based".to_string()); }
        vm.array_insert_at(id, idx - 1, args[2])?;
        Ok(args[0])
    } else {
        Err("arrayInsertAt() expects an array as the first argument".to_string())
    }
}

fn array_clear_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() { return Err("arrayClear() expects 1 argument".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        vm.array_clear(id)?;
        Ok(BxValue::new_bool(true))
    } else {
        Err("arrayClear() expects an array".to_string())
    }
}

fn array_set_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 { return Err("arraySet() expects 3 arguments: (array, index, value)".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 { return Err("Array index must be 1-based".to_string()); }
        vm.array_set(id, idx - 1, args[2])?;
        Ok(args[0])
    } else {
        Err("arraySet() expects an array as the first argument".to_string())
    }
}

// --- Struct BIFs ---

fn struct_new(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

fn struct_set_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 { return Err("structInsert() expects 3 arguments: (struct, key, value)".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        vm.struct_set(id, &key, args[2]);
        Ok(args[0])
    } else {
        Err("structInsert() expects a struct as the first argument".to_string())
    }
}

fn struct_get_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 { return Err("structGet() expects 2 arguments: (struct, key)".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        Ok(vm.struct_get(id, &key))
    } else {
        Err("structGet() expects a struct as the first argument".to_string())
    }
}

fn struct_delete_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 { return Err("structDelete() expects 2 arguments: (struct, key)".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        Ok(BxValue::new_bool(vm.struct_delete(id, &key)))
    } else {
        Err("structDelete() expects a struct as the first argument".to_string())
    }
}

fn struct_key_exists_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 { return Err("structKeyExists() expects 2 arguments: (struct, key)".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        Ok(BxValue::new_bool(vm.struct_key_exists(id, &key)))
    } else {
        Err("structKeyExists() expects a struct as the first argument".to_string())
    }
}

fn struct_key_array_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() { return Err("structKeyArray() expects 1 argument".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        let keys = vm.struct_key_array(id);
        let arr_id = vm.array_new();
        for key in keys {
            let s_id = vm.string_new(key);
            vm.array_push(arr_id, BxValue::new_ptr(s_id));
        }
        Ok(BxValue::new_ptr(arr_id))
    } else {
        Err("structKeyArray() expects a struct".to_string())
    }
}

fn struct_clear_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() { return Err("structClear() expects 1 argument".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        vm.struct_clear(id);
        Ok(BxValue::new_bool(true))
    } else {
        Err("structClear() expects a struct".to_string())
    }
}

// --- Date/Time BIFs ---

fn now(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let now = Local::now();
    let s = now.format("%Y-%m-%d %H:%M:%S").to_string();
    Ok(BxValue::new_ptr(vm.string_new(s)))
}

fn get_tick_count(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let start = SystemTime::now();
    let since_the_epoch = start.duration_since(UNIX_EPOCH).expect("Time went backwards");
    Ok(BxValue::new_number(since_the_epoch.as_millis() as f64))
}

fn sleep(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 { return Err("sleep() expects exactly 1 argument".to_string()); }
    if args[0].is_number() {
        vm.sleep(args[0].as_number() as u64);
        Ok(BxValue::new_null())
    } else {
        Err("sleep() expects a number (milliseconds)".to_string())
    }
}

fn bx_yield(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    vm.yield_fiber();
    Ok(BxValue::new_null())
}

fn run_async(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() { return Err("runAsync() expects at least 1 argument".to_string()); }
    let priority = if args.len() >= 2 && args[1].is_number() {
        args[1].as_number() as u8
    } else {
        0
    };
    vm.spawn_by_value(&args[0], Vec::new(), priority)
}

fn create_object(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 { return Err("createObject() expects at least 2 arguments: (type, class)".to_string()); }
    let obj_type = vm.to_string(args[0]).to_lowercase();
    let class_name = vm.to_string(args[1]);

    match obj_type.as_str() {
        "java" => {
            jni::create_java_object(vm, &class_name, &args[2..])
        }
        "rust" => {
            vm.construct_native_class(&class_name, &args[2..])
        }
        "native" => {
            Err("Use 'rust' type for native objects".to_string())
        }
        _ => Err(format!("Unknown object type: {}", obj_type)),
    }
}

// --- CLI BIFs ---

fn cli_clear(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        print!("\x1B[2J\x1B[1;1H");
        let _ = io::stdout().flush();
    }
    Ok(BxValue::new_null())
}

fn cli_exit(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let code = if args.len() >= 1 && args[0].is_number() {
        args[0].as_number() as i32
    } else {
        0
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::process::exit(code);
    
    #[cfg(target_arch = "wasm32")]
    return Err("cliExit not supported in WASM environment".to_string());
    
    #[allow(unreachable_code)]
    Ok(BxValue::new_null())
}

fn cli_get_args(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let all_args = vm.get_cli_args();
    let options_id = vm.struct_new();
    let positionals_id = vm.array_new();

    let mut user_args = Vec::new();
    let mut skip = true;
    for arg in all_args {
        if skip {
            if arg.ends_with("matchbox") || arg.ends_with("matchbox.exe") || arg.ends_with(".bxs") || arg.ends_with(".bxb") {
                continue;
            }
            skip = false;
        }
        user_args.push(arg);
    }

    for arg in user_args {
        if arg.starts_with("--") {
            let part = &arg[2..];
            if part.starts_with('!') {
                vm.struct_set(options_id, &part[1..], BxValue::new_bool(false));
            } else if part.starts_with("no-") {
                vm.struct_set(options_id, &part[3..], BxValue::new_bool(false));
            } else if let Some(idx) = part.find('=') {
                let key = &part[..idx];
                let val = &part[idx+1..];
                let val_id = vm.string_new(val.to_string());
                vm.struct_set(options_id, key, BxValue::new_ptr(val_id));
            } else {
                vm.struct_set(options_id, part, BxValue::new_bool(true));
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            let part = &arg[1..];
            if let Some(idx) = part.find('=') {
                let key = &part[..idx];
                let val = &part[idx+1..];
                let val_id = vm.string_new(val.to_string());
                vm.struct_set(options_id, key, BxValue::new_ptr(val_id));
            } else {
                vm.struct_set(options_id, part, BxValue::new_bool(true));
            }
        } else {
            let s_id = vm.string_new(arg);
            vm.array_push(positionals_id, BxValue::new_ptr(s_id));
        }
    }

    let result_id = vm.struct_new();
    vm.struct_set(result_id, "options", BxValue::new_ptr(options_id));
    vm.struct_set(result_id, "positionals", BxValue::new_ptr(positionals_id));
    
    Ok(BxValue::new_ptr(result_id))
}

fn cli_read(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() >= 1 {
        print!("{}", vm.to_string(args[0]));
        let _ = io::stdout().flush();
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                let trimmed = input.trim_end_matches(['\r', '\n']).to_string();
                Ok(BxValue::new_ptr(vm.string_new(trimmed)))
            }
            Err(e) => Err(format!("Failed to read from stdin: {}", e)),
        }
    }

    #[cfg(target_arch = "wasm32")]
    Err("cliRead not supported in WASM environment without JS interop".to_string())
}

fn cli_confirm(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let prompt = if args.len() >= 1 {
        vm.to_string(args[0])
    } else {
        "Confirm?".to_string()
    };
    
    print!("{} (Y/n): ", prompt);
    let _ = io::stdout().flush();

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                let trimmed = input.trim().to_lowercase();
                Ok(BxValue::new_bool(trimmed == "y" || trimmed == "yes" || trimmed.is_empty()))
            }
            Err(e) => Err(format!("Failed to read from stdin: {}", e)),
        }
    }

    #[cfg(target_arch = "wasm32")]
    Err("cliConfirm not supported in WASM environment".to_string())
}
