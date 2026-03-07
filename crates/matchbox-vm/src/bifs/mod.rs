use std::collections::HashMap;
use crate::types::{BxValue, BxVM, BxNativeFunction};
use std::time::{SystemTime, UNIX_EPOCH};
use rand::RngExt;
use chrono::Local;

mod jni;

pub fn register_all() -> HashMap<String, BxNativeFunction> {
    let mut bifs = HashMap::new();

    // Math BIFs
    bifs.insert("round".to_string(), round as BxNativeFunction);
    bifs.insert("randrange".to_string(), rand_range as BxNativeFunction);

    // Array BIFs
    bifs.insert("len".to_string(), len as BxNativeFunction);
    bifs.insert("arrayappend".to_string(), array_append as BxNativeFunction);
    bifs.insert("arraynew".to_string(), array_new as BxNativeFunction);

    // Struct BIFs
    bifs.insert("structnew".to_string(), struct_new as BxNativeFunction);

    // Date/Time BIFs
    bifs.insert("now".to_string(), now as BxNativeFunction);
    bifs.insert("gettickcount".to_string(), get_tick_count as BxNativeFunction);
    bifs.insert("sleep".to_string(), sleep as BxNativeFunction);
    bifs.insert("yield".to_string(), bx_yield as BxNativeFunction);

    // Async BIFs
    bifs.insert("runasync".to_string(), run_async as BxNativeFunction);

    // Core BIFs
    bifs.insert("createobject".to_string(), create_object as BxNativeFunction);
    bifs.insert("ucase".to_string(), ucase as BxNativeFunction);
    bifs.insert("futureonerror".to_string(), future_on_error as BxNativeFunction);

    bifs
}

// --- Implementation ---

fn future_on_error(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 { return Err("onError() expects 2 arguments: (future, callback)".to_string()); }
    if let Some(id) = args[0].as_gc_id() {
        // We should ideally check if it's actually a future here, but requires VM access to heap type
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
        // Generic len for array/struct
        Ok(BxValue::new_number(vm.array_len(id) as f64))
    } else {
        Err("len() expects a string, array, or struct".to_string())
    }
}

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

fn struct_new(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

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
    vm.spawn_by_value(&args[0], Vec::new())
}

fn create_object(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 { return Err("createObject() expects at least 2 arguments: (type, class)".to_string()); }
    let obj_type = vm.to_string(args[0]).to_lowercase();
    let class_name = vm.to_string(args[1]);

    match obj_type.as_str() {
        "java" => {
            jni::create_java_object(vm, &class_name)
        }
        "rust" | "native" => {
            Err("Native objects not yet implemented for NaN-boxing".to_string())
        }
        _ => Err(format!("Unknown object type: {}", obj_type)),
    }
}
