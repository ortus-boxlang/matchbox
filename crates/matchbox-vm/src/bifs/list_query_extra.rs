use super::{join_list, parse_list_items};
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
use crate::datasource::BxQuery;
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
use crate::datasource::traits::QueryColumn;
use crate::types::{BxNativeFunction, BxVM, BxValue};
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
use std::cell::RefCell;
use std::collections::HashMap;
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
use std::rc::Rc;

pub fn register_list_query_extra_bifs(bifs: &mut HashMap<String, BxNativeFunction>) {
    bifs.insert("gettoken".to_string(), get_token as BxNativeFunction);
    bifs.insert("listavg".to_string(), list_avg as BxNativeFunction);
    bifs.insert(
        "listchangedelims".to_string(),
        list_change_delims as BxNativeFunction,
    );
    bifs.insert("listcompact".to_string(), list_compact as BxNativeFunction);
    bifs.insert(
        "listcontains".to_string(),
        list_contains as BxNativeFunction,
    );
    bifs.insert(
        "listcontainsnocase".to_string(),
        list_contains_nocase as BxNativeFunction,
    );
    bifs.insert("listeach".to_string(), list_each as BxNativeFunction);
    bifs.insert("listevery".to_string(), list_every as BxNativeFunction);
    bifs.insert("listfilter".to_string(), list_filter as BxNativeFunction);
    bifs.insert(
        "listgetendings".to_string(),
        list_get_endings as BxNativeFunction,
    );
    bifs.insert(
        "listindexexists".to_string(),
        list_index_exists as BxNativeFunction,
    );
    bifs.insert(
        "listinsertat".to_string(),
        list_insert_at as BxNativeFunction,
    );
    bifs.insert(
        "listitemtrim".to_string(),
        list_item_trim as BxNativeFunction,
    );
    bifs.insert("listmap".to_string(), list_map as BxNativeFunction);
    bifs.insert("listnone".to_string(), list_none as BxNativeFunction);
    bifs.insert("listprepend".to_string(), list_prepend as BxNativeFunction);
    bifs.insert("listqualify".to_string(), list_qualify as BxNativeFunction);
    bifs.insert("listreduce".to_string(), list_reduce as BxNativeFunction);
    bifs.insert(
        "listreduceright".to_string(),
        list_reduce_right as BxNativeFunction,
    );
    bifs.insert(
        "listremoveduplicates".to_string(),
        list_remove_duplicates as BxNativeFunction,
    );
    bifs.insert("listsetat".to_string(), list_set_at as BxNativeFunction);
    bifs.insert("listsome".to_string(), list_some as BxNativeFunction);
    bifs.insert(
        "listvaluecount".to_string(),
        list_value_count as BxNativeFunction,
    );
    bifs.insert(
        "listvaluecountnocase".to_string(),
        list_value_count_nocase as BxNativeFunction,
    );
    bifs.insert("stringeach".to_string(), string_each as BxNativeFunction);
    bifs.insert("stringevery".to_string(), string_every as BxNativeFunction);
    bifs.insert(
        "stringfilter".to_string(),
        string_filter as BxNativeFunction,
    );
    bifs.insert("stringmap".to_string(), string_map as BxNativeFunction);
    bifs.insert("stringnone".to_string(), string_none as BxNativeFunction);
    bifs.insert(
        "stringreduce".to_string(),
        string_reduce as BxNativeFunction,
    );
    bifs.insert(
        "stringreduceright".to_string(),
        string_reduce_right as BxNativeFunction,
    );
    bifs.insert("stringsome".to_string(), string_some as BxNativeFunction);
    bifs.insert("stringsort".to_string(), string_sort as BxNativeFunction);
    #[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
    {
        bifs.insert("queryeach".to_string(), query_each as BxNativeFunction);
        bifs.insert("queryevery".to_string(), query_every as BxNativeFunction);
        bifs.insert("queryfilter".to_string(), query_filter as BxNativeFunction);
        bifs.insert("querymap".to_string(), query_map as BxNativeFunction);
        bifs.insert("querynone".to_string(), query_none as BxNativeFunction);
        bifs.insert("queryreduce".to_string(), query_reduce as BxNativeFunction);
        bifs.insert(
            "queryregisterfunction".to_string(),
            query_register_function as BxNativeFunction,
        );
        bifs.insert("querysome".to_string(), query_some as BxNativeFunction);
        bifs.insert("querysort".to_string(), query_sort as BxNativeFunction);
    }
}

fn gd(args: &[BxValue], idx: usize, vm: &dyn BxVM) -> String {
    if args.len() > idx && !args[idx].is_null() {
        vm.to_string(args[idx])
    } else {
        ",".to_string()
    }
}

pub fn get_token(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("getToken() expects at least 2 arguments".to_string());
    }
    let delim = if args.len() > 2 {
        vm.to_string(args[2])
    } else {
        ",".to_string()
    };
    let items = parse_list_items(&vm.to_string(args[0]), delim, false, false);
    let idx = args[1].as_number() as usize;
    if idx < 1 || idx > items.len() {
        return Ok(BxValue::new_ptr(vm.string_new(String::new())));
    }
    Ok(BxValue::new_ptr(vm.string_new(items[idx - 1].clone())))
}

pub fn list_avg(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listAvg() expects at least 1 argument".to_string());
    }
    let items = parse_list_items(&vm.to_string(args[0]), gd(args, 1, vm), false, false);
    if items.is_empty() {
        return Ok(BxValue::new_number(0.0));
    }
    let sum: f64 = items
        .iter()
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .sum();
    Ok(BxValue::new_number(sum / items.len() as f64))
}

pub fn list_change_delims(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listChangeDelims() expects at least 2 arguments".to_string());
    }
    let delimiter = gd(args, 2, vm);
    let items = parse_list_items(
        &vm.to_string(args[0]),
        delimiter.clone(),
        args.get(3).is_some_and(BxValue::as_bool),
        args.get(4).is_some_and(BxValue::as_bool),
    );
    let output_delimiter = vm.to_string(args[1]);
    let multi = args.get(4).is_some_and(BxValue::as_bool);
    Ok(BxValue::new_ptr(vm.string_new(join_list(
        &items,
        &output_delimiter,
        multi,
    ))))
}

pub fn list_compact(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listCompact() expects at least 1 argument".to_string());
    }
    let delimiter = gd(args, 1, vm);
    let multi = args.get(3).is_some_and(BxValue::as_bool);
    let items = parse_list_items(&vm.to_string(args[0]), delimiter.clone(), true, multi);
    let filtered: Vec<String> = if multi {
        items.into_iter().filter(|s| !s.trim().is_empty()).collect()
    } else {
        let first = items
            .iter()
            .position(|s| !s.trim().is_empty())
            .unwrap_or(items.len());
        let last = items
            .iter()
            .rposition(|s| !s.trim().is_empty())
            .map_or(first, |index| index + 1);
        items[first..last].to_vec()
    };
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&filtered, &delimiter, multi)),
    ))
}

pub fn list_contains(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listContains() expects 2 arguments".to_string());
    }
    let items = parse_list_items(
        &vm.to_string(args[0]),
        gd(args, 2, vm),
        args.get(3).is_some_and(BxValue::as_bool),
        args.get(4).is_some_and(BxValue::as_bool),
    );
    let value = vm.to_string(args[1]);
    Ok(BxValue::new_bool(
        items.iter().any(|item| item.contains(&value)),
    ))
}

pub fn list_contains_nocase(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listContainsNoCase() expects 2 arguments".to_string());
    }
    let items = parse_list_items(
        &vm.to_string(args[0]),
        gd(args, 2, vm),
        args.get(3).is_some_and(BxValue::as_bool),
        args.get(4).is_some_and(BxValue::as_bool),
    );
    let value = vm.to_string(args[1]).to_ascii_lowercase();
    Ok(BxValue::new_bool(
        items
            .iter()
            .any(|item| item.to_ascii_lowercase().contains(&value)),
    ))
}

pub fn list_each(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listEach() expects 2 arguments".to_string());
    }
    let items = parse_list_items(
        &vm.to_string(args[0]),
        gd(args, 2, vm),
        args.get(3).is_some_and(BxValue::as_bool),
        args.get(4).is_some_and(BxValue::as_bool),
    );
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for (i, item) in items.iter().enumerate() {
        let s_id = vm.string_new(item.clone());
        vm.call_function_by_value(
            &args[1],
            vec![BxValue::new_ptr(s_id), BxValue::new_number((i + 1) as f64)],
            chunk.clone(),
        )?;
    }
    Ok(BxValue::new_null())
}

pub fn list_every(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listEvery() expects 2 arguments".to_string());
    }
    let items = parse_list_items(
        &vm.to_string(args[0]),
        gd(args, 2, vm),
        args.get(3).is_some_and(BxValue::as_bool),
        args.get(4).is_some_and(BxValue::as_bool),
    );
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for (i, item) in items.iter().enumerate() {
        let s_id = vm.string_new(item.clone());
        let r = vm.call_function_by_value(
            &args[1],
            vec![BxValue::new_ptr(s_id), BxValue::new_number((i + 1) as f64)],
            chunk.clone(),
        )?;
        if !r.as_bool() {
            return Ok(BxValue::new_bool(false));
        }
    }
    Ok(BxValue::new_bool(true))
}

pub fn list_filter(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listFilter() expects 2 arguments".to_string());
    }
    let delim = gd(args, 2, vm);
    let items = parse_list_items(
        &vm.to_string(args[0]),
        delim.clone(),
        args.get(3).is_some_and(BxValue::as_bool),
        args.get(4).is_some_and(BxValue::as_bool),
    );
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    let mut result = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let s_id = vm.string_new(item.clone());
        let keep = vm.call_function_by_value(
            &args[1],
            vec![BxValue::new_ptr(s_id), BxValue::new_number((i + 1) as f64)],
            chunk.clone(),
        )?;
        if keep.as_bool() {
            result.push(item.clone());
        }
    }
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&result, &delim, false)),
    ))
}

pub fn list_get_endings(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listGetEndings() expects at least 1 argument".to_string());
    }
    let items = parse_list_items(&vm.to_string(args[0]), gd(args, 1, vm), false, false);
    let arr = vm.array_new();
    let first_s = vm.string_new(items.first().cloned().unwrap_or_default());
    vm.array_push(arr, BxValue::new_ptr(first_s));
    let last_s = vm.string_new(items.last().cloned().unwrap_or_default());
    vm.array_push(arr, BxValue::new_ptr(last_s));
    Ok(BxValue::new_ptr(arr))
}

pub fn list_index_exists(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listIndexExists() expects 2 arguments".to_string());
    }
    let items = parse_list_items(
        &vm.to_string(args[0]),
        gd(args, 2, vm),
        args.get(3).is_some_and(BxValue::as_bool),
        args.get(4).is_some_and(BxValue::as_bool),
    );
    let idx = args[1].as_number() as usize;
    Ok(BxValue::new_bool(idx >= 1 && idx <= items.len()))
}

pub fn list_insert_at(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("listInsertAt() expects 3 arguments".to_string());
    }
    let delim = gd(args, 3, vm);
    let mut items = parse_list_items(&vm.to_string(args[0]), delim.clone(), false, false);
    let pos = args[1].as_number() as usize;
    if pos < 1 || pos > items.len() + 1 {
        return Err(format!("listInsertAt() position {} out of range", pos));
    }
    items.insert(pos - 1, vm.to_string(args[2]));
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&items, &delim, false)),
    ))
}

pub fn list_item_trim(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listItemTrim() expects at least 1 argument".to_string());
    }
    let delim = gd(args, 1, vm);
    let items = parse_list_items(
        &vm.to_string(args[0]),
        delim.clone(),
        args.get(3).is_some_and(BxValue::as_bool),
        args.get(4).is_some_and(BxValue::as_bool),
    );
    let trimmed: Vec<String> = items.iter().map(|s| s.trim().to_string()).collect();
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&trimmed, &delim, false)),
    ))
}

pub fn list_map(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listMap() expects 2 arguments".to_string());
    }
    let delim = gd(args, 2, vm);
    let items = parse_list_items(
        &vm.to_string(args[0]),
        delim.clone(),
        args.get(3).is_some_and(BxValue::as_bool),
        args.get(4).is_some_and(BxValue::as_bool),
    );
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    let mut result = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let s_id = vm.string_new(item.clone());
        let val = vm.call_function_by_value(
            &args[1],
            vec![BxValue::new_ptr(s_id), BxValue::new_number((i + 1) as f64)],
            chunk.clone(),
        )?;
        result.push(vm.to_string(val));
    }
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&result, &delim, false)),
    ))
}

pub fn list_none(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let some = list_some(vm, args)?;
    Ok(BxValue::new_bool(!some.as_bool()))
}

pub fn list_prepend(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listPrepend() expects at least 2 arguments".to_string());
    }
    let delim = gd(args, 2, vm);
    let mut items = parse_list_items(&vm.to_string(args[0]), delim.clone(), false, false);
    items.insert(0, vm.to_string(args[1]));
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&items, &delim, false)),
    ))
}

pub fn list_qualify(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listQualify() expects 2 arguments".to_string());
    }
    let delim = gd(args, 2, vm);
    let items = parse_list_items(
        &vm.to_string(args[0]),
        delim.clone(),
        args.get(4).is_some_and(BxValue::as_bool),
        false,
    );
    let q = vm.to_string(args[1]);
    let element_type = args
        .get(3)
        .map(|value| vm.to_string(*value).to_ascii_lowercase());
    let qualified: Vec<String> = items
        .iter()
        .map(|item| {
            let should_qualify = match element_type.as_deref() {
                Some("char") => item.parse::<f64>().is_err(),
                _ => true,
            };
            if should_qualify {
                format!("{}{}{}", q, item, q)
            } else {
                item.clone()
            }
        })
        .collect();
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&qualified, &delim, false)),
    ))
}

pub fn list_reduce(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("listReduce() expects 3 arguments".to_string());
    }
    let items = parse_list_items(&vm.to_string(args[0]), gd(args, 3, vm), false, false);
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    let mut acc = args[2];
    for (i, item) in items.iter().enumerate() {
        let s_id = vm.string_new(item.clone());
        acc = vm.call_function_by_value(
            &args[1],
            vec![
                acc,
                BxValue::new_ptr(s_id),
                BxValue::new_number((i + 1) as f64),
            ],
            chunk.clone(),
        )?;
    }
    Ok(acc)
}

pub fn list_reduce_right(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("listReduceRight() expects 3 arguments".to_string());
    }
    let items = parse_list_items(&vm.to_string(args[0]), gd(args, 3, vm), false, false);
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    let mut acc = args[2];
    for (i, item) in items.iter().rev().enumerate() {
        let s_id = vm.string_new(item.clone());
        acc = vm.call_function_by_value(
            &args[1],
            vec![
                acc,
                BxValue::new_ptr(s_id),
                BxValue::new_number((i + 1) as f64),
            ],
            chunk.clone(),
        )?;
    }
    Ok(acc)
}

pub fn list_remove_duplicates(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listRemoveDuplicates() expects at least 1 argument".to_string());
    }
    let delim = gd(args, 1, vm);
    let items = parse_list_items(&vm.to_string(args[0]), delim.clone(), true, false);
    let mut seen = Vec::new();
    let mut result = Vec::new();
    for item in items {
        let compare = if args.get(2).is_some_and(BxValue::as_bool) {
            item.to_ascii_lowercase()
        } else {
            item.clone()
        };
        if !seen.contains(&compare) {
            seen.push(compare);
            result.push(item);
        }
    }
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&result, &delim, false)),
    ))
}

pub fn list_set_at(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("listSetAt() expects 3 arguments".to_string());
    }
    let delim = gd(args, 3, vm);
    let mut items = parse_list_items(&vm.to_string(args[0]), delim.clone(), false, false);
    let pos = args[1].as_number() as usize;
    if pos < 1 || pos > items.len() {
        return Err(format!("listSetAt() position {} out of range", pos));
    }
    items[pos - 1] = vm.to_string(args[2]);
    Ok(BxValue::new_ptr(
        vm.string_new(join_list(&items, &delim, false)),
    ))
}

pub fn list_some(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listSome() expects 2 arguments".to_string());
    }
    let items = parse_list_items(
        &vm.to_string(args[0]),
        gd(args, 2, vm),
        args.get(3).is_some_and(BxValue::as_bool),
        args.get(4).is_some_and(BxValue::as_bool),
    );
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for (i, item) in items.iter().enumerate() {
        let s_id = vm.string_new(item.clone());
        let r = vm.call_function_by_value(
            &args[1],
            vec![BxValue::new_ptr(s_id), BxValue::new_number((i + 1) as f64)],
            chunk.clone(),
        )?;
        if r.as_bool() {
            return Ok(BxValue::new_bool(true));
        }
    }
    Ok(BxValue::new_bool(false))
}

pub fn list_value_count(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listValueCount() expects 2 arguments".to_string());
    }
    let items = parse_list_items(&vm.to_string(args[0]), gd(args, 2, vm), false, false);
    let value = vm.to_string(args[1]);
    Ok(BxValue::new_number(
        items.iter().filter(|s| *s == &value).count() as f64,
    ))
}

pub fn list_value_count_nocase(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("listValueCountNoCase() expects 2 arguments".to_string());
    }
    let items = parse_list_items(&vm.to_string(args[0]), gd(args, 2, vm), false, false);
    let value = vm.to_string(args[1]).to_ascii_lowercase();
    Ok(BxValue::new_number(
        items
            .iter()
            .filter(|item| item.to_ascii_lowercase() == value)
            .count() as f64,
    ))
}

fn string_callback_args(
    vm: &mut dyn BxVM,
    chunk: &std::rc::Rc<std::cell::RefCell<crate::vm::chunk::Chunk>>,
    callback: &BxValue,
    ch: char,
    index: usize,
) -> Result<BxValue, String> {
    let ch_id = vm.string_new(ch.to_string());
    vm.call_function_by_value(
        callback,
        vec![
            BxValue::new_ptr(ch_id),
            BxValue::new_number((index + 1) as f64),
        ],
        chunk.clone(),
    )
}

pub fn string_each(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("stringEach() expects 2 arguments".to_string());
    }
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for (index, ch) in vm.to_string(args[0]).chars().enumerate() {
        string_callback_args(vm, &chunk, &args[1], ch, index)?;
    }
    Ok(BxValue::new_null())
}

pub fn string_every(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("stringEvery() expects 2 arguments".to_string());
    }
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for (index, ch) in vm.to_string(args[0]).chars().enumerate() {
        if !string_callback_args(vm, &chunk, &args[1], ch, index)?.as_bool() {
            return Ok(BxValue::new_bool(false));
        }
    }
    Ok(BxValue::new_bool(true))
}

pub fn string_filter(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("stringFilter() expects 2 arguments".to_string());
    }
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    let mut result = String::new();
    for (index, ch) in vm.to_string(args[0]).chars().enumerate() {
        if string_callback_args(vm, &chunk, &args[1], ch, index)?.as_bool() {
            result.push(ch);
        }
    }
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

pub fn string_map(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("stringMap() expects 2 arguments".to_string());
    }
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    let mut result = String::new();
    for (index, ch) in vm.to_string(args[0]).chars().enumerate() {
        let value = string_callback_args(vm, &chunk, &args[1], ch, index)?;
        result.push_str(&vm.to_string(value));
    }
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

pub fn string_none(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_bool(!string_some(vm, args)?.as_bool()))
}

pub fn string_reduce(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("stringReduce() expects 3 arguments".to_string());
    }
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    let mut result = args[2];
    for (index, ch) in vm.to_string(args[0]).chars().enumerate() {
        let ch_id = vm.string_new(ch.to_string());
        result = vm.call_function_by_value(
            &args[1],
            vec![
                result,
                BxValue::new_ptr(ch_id),
                BxValue::new_number((index + 1) as f64),
            ],
            chunk.clone(),
        )?;
    }
    Ok(result)
}

pub fn string_reduce_right(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("stringReduceRight() expects 3 arguments".to_string());
    }
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    let mut result = args[2];
    for (index, ch) in vm.to_string(args[0]).chars().rev().enumerate() {
        let ch_id = vm.string_new(ch.to_string());
        result = vm.call_function_by_value(
            &args[1],
            vec![
                result,
                BxValue::new_ptr(ch_id),
                BxValue::new_number((index + 1) as f64),
            ],
            chunk.clone(),
        )?;
    }
    Ok(result)
}

pub fn string_some(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("stringSome() expects 2 arguments".to_string());
    }
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for (index, ch) in vm.to_string(args[0]).chars().enumerate() {
        if string_callback_args(vm, &chunk, &args[1], ch, index)?.as_bool() {
            return Ok(BxValue::new_bool(true));
        }
    }
    Ok(BxValue::new_bool(false))
}

pub fn string_sort(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("stringSort() expects at least 1 argument".to_string());
    }
    let mut chars: Vec<char> = vm.to_string(args[0]).chars().collect();
    chars.sort_unstable();
    Ok(BxValue::new_ptr(vm.string_new(chars.into_iter().collect())))
}

#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
pub fn query_each(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryEach() expects 2 arguments".to_string());
    }
    let query_id = query_id(args)?;
    let row_count = query_row_count(vm, query_id)?;
    let query = BxValue::new_ptr(query_id);
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for row_index in 0..row_count {
        let row = query_row(vm, query_id, row_index)?;
        query_callback(
            vm,
            &args[1],
            vec![row, BxValue::new_number((row_index + 1) as f64), query],
            vec![row],
            chunk.clone(),
        )?;
    }
    Ok(BxValue::new_null())
}
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
pub fn query_every(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryEvery() expects 2 arguments".to_string());
    }
    let query_id = query_id(args)?;
    let row_count = query_row_count(vm, query_id)?;
    let query = BxValue::new_ptr(query_id);
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for row_index in 0..row_count {
        let row = query_row(vm, query_id, row_index)?;
        let result = query_callback(
            vm,
            &args[1],
            vec![row, BxValue::new_number((row_index + 1) as f64), query],
            vec![row],
            chunk.clone(),
        )?;
        if !result.as_bool() {
            return Ok(BxValue::new_bool(false));
        }
    }
    Ok(BxValue::new_bool(true))
}
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
pub fn query_filter(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryFilter() expects 2 arguments".to_string());
    }
    let query_id = query_id(args)?;
    let columns = query_columns(vm, query_id)?;
    let result_id = new_query(vm, &columns);
    let row_count = query_row_count(vm, query_id)?;
    let query = BxValue::new_ptr(query_id);
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for row_index in 0..row_count {
        let row = query_row(vm, query_id, row_index)?;
        let keep = query_callback(
            vm,
            &args[1],
            vec![row, BxValue::new_number((row_index + 1) as f64), query],
            vec![row],
            chunk.clone(),
        )?;
        if keep.as_bool() {
            vm.native_object_call_method(result_id, "setrow", &[row])?;
        }
    }
    Ok(BxValue::new_ptr(result_id))
}
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
pub fn query_map(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryMap() expects 2 arguments".to_string());
    }
    let query_id = query_id(args)?;
    let columns = query_columns(vm, query_id)?;
    let result_id = new_query(vm, &columns);
    let row_count = query_row_count(vm, query_id)?;
    let query = BxValue::new_ptr(query_id);
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for row_index in 0..row_count {
        let row = query_row(vm, query_id, row_index)?;
        let mapped = query_callback(
            vm,
            &args[1],
            vec![row, BxValue::new_number((row_index + 1) as f64), query],
            vec![row],
            chunk.clone(),
        )?;
        let mapped_row = vm.struct_new();
        if let Some(mapped_id) = mapped.as_gc_id().filter(|_| vm.is_struct_value(mapped)) {
            for column in &columns {
                vm.struct_set(
                    mapped_row,
                    &column.name,
                    vm.struct_get(mapped_id, &column.name),
                );
            }
        } else if let Some(first) = columns.first() {
            vm.struct_set(mapped_row, &first.name, mapped);
        }
        vm.native_object_call_method(result_id, "setrow", &[BxValue::new_ptr(mapped_row)])?;
    }
    Ok(BxValue::new_ptr(result_id))
}
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
pub fn query_none(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_bool(!query_some(vm, args)?.as_bool()))
}
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
pub fn query_reduce(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("queryReduce() expects 3 arguments".to_string());
    }
    let query_id = query_id(args)?;
    let row_count = query_row_count(vm, query_id)?;
    let query = BxValue::new_ptr(query_id);
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    let mut accumulator = args[2];
    for row_index in 0..row_count {
        let row = query_row(vm, query_id, row_index)?;
        accumulator = query_callback(
            vm,
            &args[1],
            vec![
                accumulator,
                row,
                BxValue::new_number((row_index + 1) as f64),
                query,
            ],
            vec![accumulator, row],
            chunk.clone(),
        )?;
    }
    Ok(accumulator)
}
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
pub fn query_register_function(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    Err("queryRegisterFunction() not supported".to_string())
}
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
pub fn query_some(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("querySome() expects 2 arguments".to_string());
    }
    let query_id = query_id(args)?;
    let row_count = query_row_count(vm, query_id)?;
    let query = BxValue::new_ptr(query_id);
    let chunk = vm.current_chunk().ok_or("No current chunk")?;
    for row_index in 0..row_count {
        let row = query_row(vm, query_id, row_index)?;
        let result = query_callback(
            vm,
            &args[1],
            vec![row, BxValue::new_number((row_index + 1) as f64), query],
            vec![row],
            chunk.clone(),
        )?;
        if result.as_bool() {
            return Ok(BxValue::new_bool(true));
        }
    }
    Ok(BxValue::new_bool(false))
}
#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
pub fn query_sort(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("querySort() expects at least 2 arguments".to_string());
    }
    let query_id = query_id(args)?;
    vm.native_object_call_method(query_id, "sortcolumn", &args[1..])
}

#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
fn query_id(args: &[BxValue]) -> Result<usize, String> {
    args[0]
        .as_gc_id()
        .ok_or_else(|| "first argument must be a query object".to_string())
}

#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
fn query_row_count(vm: &mut dyn BxVM, query_id: usize) -> Result<usize, String> {
    Ok(vm
        .native_object_call_method(query_id, "recordcount", &[])?
        .as_number() as usize)
}

#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
fn query_row(vm: &mut dyn BxVM, query_id: usize, row_index: usize) -> Result<BxValue, String> {
    vm.native_object_call_method(
        query_id,
        "getrow",
        &[BxValue::new_number((row_index + 1) as f64)],
    )
}

#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
fn query_columns(vm: &mut dyn BxVM, query_id: usize) -> Result<Vec<QueryColumn>, String> {
    vm.native_object_query_columns(query_id)
        .ok_or_else(|| "first argument must be a query object".to_string())
}

#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
fn new_query(vm: &mut dyn BxVM, columns: &[QueryColumn]) -> usize {
    vm.native_object_new(Rc::new(RefCell::new(BxQuery::new(columns.to_vec()))))
}

#[cfg(all(feature = "bif-datasource", not(target_arch = "wasm32")))]
fn query_callback(
    vm: &mut dyn BxVM,
    callback: &BxValue,
    full_args: Vec<BxValue>,
    minimal_args: Vec<BxValue>,
    chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
) -> Result<BxValue, String> {
    match vm.call_function_by_value(callback, full_args, chunk.clone()) {
        Ok(value) => Ok(value),
        Err(error) if error.contains("Expected") && error.contains("arguments") => {
            vm.call_function_by_value(callback, minimal_args, chunk)
        }
        Err(error) => Err(error),
    }
}
