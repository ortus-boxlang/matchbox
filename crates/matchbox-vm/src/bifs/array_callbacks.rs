//! Array higher-order BIFs. BoxLang passes `(value, index, array)` to every
//! callback and `(accumulator, value, index, array)` to `arrayReduce`.

use crate::types::{NativeBif, BxVM, BxValue};
use std::collections::HashMap;

pub fn register_array_callback_bifs(bifs: &mut HashMap<String, NativeBif>) {
    bifs.insert("arrayeach".to_string(), NativeBif { func: array_each, params: params!["array", "callback", "parallel" = Bool(false), "maxthreads", "ordered" = Bool(false), "virtual" = Bool(false)] });
    bifs.insert("arrayfilter".to_string(), NativeBif { func: array_filter, params: params!["array", "callback", "parallel" = Bool(false), "maxthreads", "virtual" = Bool(false)] });
    bifs.insert("arrayreduce".to_string(), NativeBif { func: array_reduce, params: params!["array", "callback", "initialvalue"] });
    bifs.insert("arraysome".to_string(), NativeBif { func: array_some, params: params!["array", "callback", "parallel" = Bool(false), "maxthreads", "virtual" = Bool(false)] });
    bifs.insert("arrayevery".to_string(), NativeBif { func: array_every, params: params!["array", "callback", "parallel" = Bool(false), "maxthreads", "virtual" = Bool(false)] });
    bifs.insert("arraynone".to_string(), NativeBif { func: array_none, params: params!["array", "callback", "parallel" = Bool(false), "maxthreads", "virtual" = Bool(false)] });
}

fn array_arg(vm: &dyn BxVM, args: &[BxValue], name: &str) -> Result<usize, String> {
    if args.len() < 2 {
        return Err(format!("{}() expects an array and callback", name));
    }
    if !vm.is_array_value(args[0]) {
        return Err(format!("{}() expects an array as the first argument", name));
    }
    args[0]
        .as_gc_id()
        .ok_or_else(|| format!("{}() expects an array as the first argument", name))
}

/// Calls `callback( value, index, array )` for each element until `stop` returns true.
fn each_element(
    vm: &mut dyn BxVM,
    args: &[BxValue],
    name: &str,
    mut visit: impl FnMut(&mut dyn BxVM, BxValue, BxValue) -> bool,
) -> Result<(), String> {
    let array_id = array_arg(vm, args, name)?;
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| format!("{}() requires an active execution chunk", name))?;
    for index in 0..vm.array_len(array_id) {
        let value = vm.array_get(array_id, index);
        let result = vm.call_function_by_value(
            &args[1],
            vec![value, BxValue::new_number((index + 1) as f64), args[0]],
            chunk.clone(),
        )?;
        if visit(vm, value, result) {
            break;
        }
    }
    Ok(())
}

fn array_each(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    each_element(vm, args, "arrayEach", |_, _, _| false)?;
    Ok(BxValue::new_null())
}

fn array_filter(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let result_id = vm.array_new();
    each_element(vm, args, "arrayFilter", |vm, value, keep| {
        if vm.is_truthy(keep) {
            vm.array_push(result_id, value);
        }
        false
    })?;
    Ok(BxValue::new_ptr(result_id))
}

fn array_some(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let mut found = false;
    each_element(vm, args, "arraySome", |vm, _, hit| {
        found = vm.is_truthy(hit);
        found
    })?;
    Ok(BxValue::new_bool(found))
}

fn array_every(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let mut all = true;
    each_element(vm, args, "arrayEvery", |vm, _, hit| {
        all = vm.is_truthy(hit);
        !all
    })?;
    Ok(BxValue::new_bool(all))
}

fn array_none(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let mut any = false;
    each_element(vm, args, "arrayNone", |vm, _, hit| {
        any = vm.is_truthy(hit);
        any
    })?;
    Ok(BxValue::new_bool(!any))
}

fn array_reduce(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let array_id = array_arg(vm, args, "arrayReduce")?;
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| "arrayReduce() requires an active execution chunk".to_string())?;
    let mut accumulator = args.get(2).copied().unwrap_or_else(BxValue::new_null);
    for index in 0..vm.array_len(array_id) {
        let value = vm.array_get(array_id, index);
        accumulator = vm.call_function_by_value(
            &args[1],
            vec![accumulator, value, BxValue::new_number((index + 1) as f64), args[0]],
            chunk.clone(),
        )?;
    }
    Ok(accumulator)
}
