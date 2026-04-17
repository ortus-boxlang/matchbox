use crate::types::{BxNativeFunction, BxVM, BxValue};
use chrono::Local;
use rand::RngExt;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[cfg(feature = "bif-jni")]
mod jni;
#[cfg(not(feature = "bif-jni"))]
mod jni {
    use crate::types::{BxVM, BxValue};
    pub fn create_java_object(
        _vm: &mut dyn BxVM,
        _class_name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        Err("Java interoperability is not enabled in this build.".to_string())
    }
}
mod cli;
mod crypto;
#[cfg(feature = "bif-datasource")]
mod datasource;
mod fs;
mod http;
mod json;
mod zip;

pub fn register_all() -> HashMap<String, BxNativeFunction> {
    let mut bifs = HashMap::new();

    // Math BIFs
    bifs.insert("round".to_string(), round as BxNativeFunction);
    bifs.insert("abs".to_string(), abs_bif as BxNativeFunction);
    bifs.insert("min".to_string(), min_bif as BxNativeFunction);
    bifs.insert("max".to_string(), max_bif as BxNativeFunction);
    bifs.insert("randrange".to_string(), rand_range as BxNativeFunction);

    // Array BIFs
    bifs.insert("arrayappend".to_string(), array_append as BxNativeFunction);
    bifs.insert("arraylen".to_string(), len as BxNativeFunction);
    bifs.insert("arraynew".to_string(), array_new as BxNativeFunction);
    bifs.insert("arraypop".to_string(), array_pop_bif as BxNativeFunction);
    bifs.insert(
        "arraydeleteat".to_string(),
        array_delete_at_bif as BxNativeFunction,
    );
    bifs.insert(
        "arrayinsertat".to_string(),
        array_insert_at_bif as BxNativeFunction,
    );
    bifs.insert(
        "arrayclear".to_string(),
        array_clear_bif as BxNativeFunction,
    );
    bifs.insert("arrayset".to_string(), array_set_bif as BxNativeFunction);
    bifs.insert("bytesnew".to_string(), bytes_new as BxNativeFunction);
    bifs.insert("byteslen".to_string(), bytes_len_bif as BxNativeFunction);
    bifs.insert("bytesget".to_string(), bytes_get_bif as BxNativeFunction);
    bifs.insert("bytesset".to_string(), bytes_set_bif as BxNativeFunction);
    bifs.insert("isbinary".to_string(), is_bytes_bif as BxNativeFunction);

    // Struct BIFs
    bifs.insert("structnew".to_string(), struct_new as BxNativeFunction);
    bifs.insert(
        "structinsert".to_string(),
        struct_set_bif as BxNativeFunction,
    );
    bifs.insert(
        "structupdate".to_string(),
        struct_set_bif as BxNativeFunction,
    );
    bifs.insert(
        "structdelete".to_string(),
        struct_delete_bif as BxNativeFunction,
    );
    bifs.insert(
        "structkeyexists".to_string(),
        struct_key_exists_bif as BxNativeFunction,
    );
    bifs.insert("structget".to_string(), struct_get_bif as BxNativeFunction);
    bifs.insert(
        "structkeyarray".to_string(),
        struct_key_array_bif as BxNativeFunction,
    );
    bifs.insert(
        "structclear".to_string(),
        struct_clear_bif as BxNativeFunction,
    );
    bifs.insert("structcount".to_string(), len as BxNativeFunction);

    // Core BIFs
    bifs.insert("len".to_string(), len as BxNativeFunction);
    bifs.insert(
        "writeoutput".to_string(),
        write_output_bif as BxNativeFunction,
    );
    bifs.insert(
        "createobject".to_string(),
        create_object as BxNativeFunction,
    );
    bifs.insert("isnull".to_string(), is_null_bif as BxNativeFunction);
    bifs.insert("ucase".to_string(), ucase as BxNativeFunction);
    bifs.insert("lcase".to_string(), lcase as BxNativeFunction);
    bifs.insert("trim".to_string(), trim_bif as BxNativeFunction);
    bifs.insert("listtoarray".to_string(), list_to_array as BxNativeFunction);
    bifs.insert("indexof".to_string(), index_of as BxNativeFunction);
    bifs.insert("chr".to_string(), chr_bif as BxNativeFunction);
    bifs.insert(
        "futureonerror".to_string(),
        future_on_error as BxNativeFunction,
    );

    // System BIFs
    bifs.insert("createuuid".to_string(), create_uuid as BxNativeFunction);
    bifs.insert("createguid".to_string(), create_guid as BxNativeFunction);
    bifs.insert(
        "getsystemsetting".to_string(),
        get_system_setting as BxNativeFunction,
    );

    // Date/Time BIFs
    bifs.insert("now".to_string(), now as BxNativeFunction);
    bifs.insert(
        "gettickcount".to_string(),
        get_tick_count as BxNativeFunction,
    );
    bifs.insert("sleep".to_string(), sleep as BxNativeFunction);
    bifs.insert("yield".to_string(), bx_yield as BxNativeFunction);

    // CLI BIFs
    #[cfg(feature = "bif-cli")]
    {
        bifs.insert("cliclear".to_string(), cli::cli_clear as BxNativeFunction);
        bifs.insert("cliexit".to_string(), cli::cli_exit as BxNativeFunction);
        bifs.insert("exit".to_string(), cli::cli_exit as BxNativeFunction);
        bifs.insert(
            "cligetargs".to_string(),
            cli::cli_get_args as BxNativeFunction,
        );
        bifs.insert("cliread".to_string(), cli::cli_read as BxNativeFunction);
        bifs.insert(
            "cliconfirm".to_string(),
            cli::cli_confirm as BxNativeFunction,
        );
        #[cfg(feature = "bif-cli")]
        bifs.insert("cliselect".to_string(), cli::cli_select as BxNativeFunction);
    }

    // Async BIFs
    bifs.insert("runasync".to_string(), run_async as BxNativeFunction);

    // IO BIFs
    #[cfg(feature = "bif-io")]
    {
        bifs.insert(
            "directoryexists".to_string(),
            fs::directory_exists as BxNativeFunction,
        );
        bifs.insert(
            "directorycreate".to_string(),
            fs::directory_create as BxNativeFunction,
        );
        bifs.insert(
            "directorydelete".to_string(),
            fs::directory_delete as BxNativeFunction,
        );
        bifs.insert(
            "directorylist".to_string(),
            fs::directory_list as BxNativeFunction,
        );
        bifs.insert(
            "fileexists".to_string(),
            fs::file_exists as BxNativeFunction,
        );
        bifs.insert(
            "filedelete".to_string(),
            fs::file_delete as BxNativeFunction,
        );
        bifs.insert("filemove".to_string(), fs::file_move as BxNativeFunction);
        bifs.insert("filecopy".to_string(), fs::file_copy as BxNativeFunction);
        bifs.insert("fileinfo".to_string(), fs::file_info as BxNativeFunction);
        bifs.insert(
            "filecreatesymlink".to_string(),
            fs::file_create_symlink as BxNativeFunction,
        );
        bifs.insert(
            "filesetexecutable".to_string(),
            fs::file_set_executable as BxNativeFunction,
        );
        bifs.insert("fileread".to_string(), fs::file_read as BxNativeFunction);
        bifs.insert("filewrite".to_string(), fs::file_write as BxNativeFunction);
        bifs.insert(
            "fileappend".to_string(),
            fs::file_append as BxNativeFunction,
        );
    }

    // HTTP BIFs
    #[cfg(feature = "bif-http")]
    bifs.insert("http".to_string(), http::http_bif as BxNativeFunction);

    // ZIP BIFs
    #[cfg(feature = "bif-zip")]
    bifs.insert("extract".to_string(), zip::zip_extract as BxNativeFunction);

    // JSON BIFs
    bifs.insert(
        "jsondeserialize".to_string(),
        json::json_deserialize as BxNativeFunction,
    );
    bifs.insert(
        "jsonserialize".to_string(),
        json::json_serialize as BxNativeFunction,
    );
    bifs.insert(
        "loadproperties".to_string(),
        json::load_properties as BxNativeFunction,
    );

    // Crypto BIFs
    #[cfg(feature = "bif-crypto")]
    bifs.insert("hash".to_string(), crypto::hash_bif as BxNativeFunction);

    // Datasource BIFs
    #[cfg(feature = "bif-datasource")]
    {
        bifs.insert(
            "datasourceregister".to_string(),
            datasource::datasource_register as BxNativeFunction,
        );
        bifs.insert(
            "queryexecute".to_string(),
            datasource::query_execute as BxNativeFunction,
        );
        bifs.insert(
            "querynew".to_string(),
            datasource::query_new as BxNativeFunction,
        );
        bifs.insert(
            "queryaddrow".to_string(),
            datasource::query_add_row as BxNativeFunction,
        );
        bifs.insert(
            "querycolumndata".to_string(),
            datasource::query_column_data as BxNativeFunction,
        );
        bifs.insert(
            "querycolumnlist".to_string(),
            datasource::query_column_list as BxNativeFunction,
        );
        bifs.insert(
            "transactionbegin".to_string(),
            datasource::transaction_begin as BxNativeFunction,
        );
        bifs.insert(
            "transactioncommit".to_string(),
            datasource::transaction_commit as BxNativeFunction,
        );
        bifs.insert(
            "transactionrollback".to_string(),
            datasource::transaction_rollback as BxNativeFunction,
        );
    }

    bifs
}

// --- Implementation ---

fn future_on_error(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("onError() expects 2 arguments: (future, callback)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        vm.future_on_error(id, args[1]);
        Ok(args[0])
    } else {
        Err("First argument to onError must be a future".to_string())
    }
}

fn ucase(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("ucase() expects exactly 1 argument".to_string());
    }
    let s = vm.to_string(args[0]).to_uppercase();
    Ok(BxValue::new_ptr(vm.string_new(s)))
}

fn lcase(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("lcase() expects exactly 1 argument".to_string());
    }
    let s = vm.to_string(args[0]).to_lowercase();
    Ok(BxValue::new_ptr(vm.string_new(s)))
}

fn trim_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("trim() expects exactly 1 argument".to_string());
    }
    let s = vm.to_string(args[0]).trim().to_string();
    Ok(BxValue::new_ptr(vm.string_new(s)))
}

fn list_to_array(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("listToArray() expects at least 1 argument".to_string());
    }
    let s = vm.to_string(args[0]);
    let del = if args.len() > 1 {
        vm.to_string(args[1])
    } else {
        ",".to_string()
    };

    let array_id = vm.array_new();
    for part in s.split(&del) {
        let s_id = vm.string_new(part.to_string());
        vm.array_push(array_id, BxValue::new_ptr(s_id));
    }

    Ok(BxValue::new_ptr(array_id))
}

fn index_of(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("indexOf() expects 2 arguments: (string, substring)".to_string());
    }
    let s = vm.to_string(args[0]);
    let sub = vm.to_string(args[1]);

    match s.find(&sub) {
        Some(idx) => Ok(BxValue::new_number(idx as f64 + 1.0)), // 1-based index for BoxLang consistency
        None => Ok(BxValue::new_number(-1.0)),
    }
}

fn chr_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("chr() expects 1 argument".to_string());
    }
    let code = args[0].as_number() as u32;
    let c = std::char::from_u32(code).ok_or_else(|| format!("Invalid character code: {}", code))?;
    let s_id = vm.string_new(c.to_string());
    Ok(BxValue::new_ptr(s_id))
}

fn round(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("round() expects exactly 1 argument".to_string());
    }
    if args[0].is_number() {
        Ok(BxValue::new_number(args[0].as_number().round()))
    } else {
        Err("round() expects a number".to_string())
    }
}

fn abs_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("abs() expects exactly 1 argument".to_string());
    }
    if args[0].is_number() {
        Ok(BxValue::new_number(args[0].as_number().abs()))
    } else {
        Err("abs() expects a number".to_string())
    }
}

fn min_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("min() expects exactly 2 arguments".to_string());
    }
    if args[0].is_number() && args[1].is_number() {
        Ok(BxValue::new_number(
            args[0].as_number().min(args[1].as_number()),
        ))
    } else {
        Err("min() expects numbers".to_string())
    }
}

fn max_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("max() expects exactly 2 arguments".to_string());
    }
    if args[0].is_number() && args[1].is_number() {
        Ok(BxValue::new_number(
            args[0].as_number().max(args[1].as_number()),
        ))
    } else {
        Err("max() expects numbers".to_string())
    }
}

fn rand_range(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("randRange() expects exactly 2 arguments".to_string());
    }
    if args[0].is_number() && args[1].is_number() {
        let mut rng = rand::rng();
        let val = rng.random_range((args[0].as_number() as i64)..=(args[1].as_number() as i64));
        Ok(BxValue::new_number(val as f64))
    } else {
        Err("randRange() expects numbers".to_string())
    }
}

fn len(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("len() expects exactly 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        Ok(BxValue::new_number(vm.get_len(id) as f64))
    } else {
        Err("len() expects a string, array, or struct".to_string())
    }
}

fn write_output_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    for arg in args {
        let s = vm.to_string(*arg);
        vm.write_output(&s);
    }
    Ok(BxValue::new_bool(true))
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
    if args.is_empty() {
        return Err("getSystemSetting() expects at least 1 argument".to_string());
    }
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
    if args.len() != 2 {
        return Err("arrayAppend() expects exactly 2 arguments".to_string());
    }
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
    if args.is_empty() {
        return Err("arrayPop() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        vm.array_pop(id)
    } else {
        Err("arrayPop() expects an array".to_string())
    }
}

fn array_delete_at_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("arrayDeleteAt() expects 2 arguments: (array, index)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 {
            return Err("Array index must be 1-based".to_string());
        }
        vm.array_delete_at(id, idx - 1)
    } else {
        Err("arrayDeleteAt() expects an array as the first argument".to_string())
    }
}

fn array_insert_at_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("arrayInsertAt() expects 3 arguments: (array, index, value)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 {
            return Err("Array index must be 1-based".to_string());
        }
        vm.array_insert_at(id, idx - 1, args[2])?;
        Ok(args[0])
    } else {
        Err("arrayInsertAt() expects an array as the first argument".to_string())
    }
}

fn array_clear_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("arrayClear() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        vm.array_clear(id)?;
        Ok(BxValue::new_bool(true))
    } else {
        Err("arrayClear() expects an array".to_string())
    }
}

fn array_set_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("arraySet() expects 3 arguments: (array, index, value)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 {
            return Err("Array index must be 1-based".to_string());
        }
        vm.array_set(id, idx - 1, args[2])?;
        Ok(args[0])
    } else {
        Err("arraySet() expects an array as the first argument".to_string())
    }
}

fn bytes_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("bytesNew() expects 1 argument: (array)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let len = vm.array_len(id);
        let mut out = Vec::with_capacity(len);
        for idx in 0..len {
            let value = vm.array_get(id, idx);
            if !value.is_number() {
                return Err("bytesNew() expects an array of byte values".to_string());
            }
            let byte = value.as_number();
            if !(0.0..=255.0).contains(&byte) || byte.fract() != 0.0 {
                return Err("bytesNew() byte values must be integers in the range 0..255".to_string());
            }
            out.push(byte as u8);
        }
        Ok(BxValue::new_ptr(vm.bytes_new(out)))
    } else {
        Err("bytesNew() expects an array".to_string())
    }
}

fn bytes_len_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("bytesLen() expects 1 argument".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        Ok(BxValue::new_number(vm.bytes_len(id) as f64))
    } else {
        Err("bytesLen() expects bytes".to_string())
    }
}

fn bytes_get_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("bytesGet() expects 2 arguments: (bytes, index)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 {
            return Err("Byte index must be 1-based".to_string());
        }
        Ok(BxValue::new_number(vm.bytes_get(id, idx - 1)? as f64))
    } else {
        Err("bytesGet() expects bytes as the first argument".to_string())
    }
}

fn bytes_set_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 3 {
        return Err("bytesSet() expects 3 arguments: (bytes, index, value)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let idx = args[1].as_number() as usize;
        if idx == 0 {
            return Err("Byte index must be 1-based".to_string());
        }
        if !args[2].is_number() {
            return Err("bytesSet() expects a numeric byte value".to_string());
        }
        let value = args[2].as_number();
        if !(0.0..=255.0).contains(&value) || value.fract() != 0.0 {
            return Err("bytesSet() byte values must be integers in the range 0..255".to_string());
        }
        vm.bytes_set(id, idx - 1, value as u8)?;
        Ok(args[0])
    } else {
        Err("bytesSet() expects bytes as the first argument".to_string())
    }
}

fn is_bytes_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    Ok(BxValue::new_bool(vm.is_bytes(args[0])))
}

// --- Struct BIFs ---

fn struct_new(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

fn struct_set_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("structInsert() expects 3 arguments: (struct, key, value)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        vm.struct_set(id, &key, args[2]);
        Ok(args[0])
    } else {
        Err("structInsert() expects a struct as the first argument".to_string())
    }
}

fn struct_get_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("structGet() expects 2 arguments: (struct, key)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        Ok(vm.struct_get(id, &key))
    } else {
        Err("structGet() expects a struct as the first argument".to_string())
    }
}

fn struct_delete_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("structDelete() expects 2 arguments: (struct, key)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        Ok(BxValue::new_bool(vm.struct_delete(id, &key)))
    } else {
        Err("structDelete() expects a struct as the first argument".to_string())
    }
}

fn struct_key_exists_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("structKeyExists() expects 2 arguments: (struct, key)".to_string());
    }
    if let Some(id) = args[0].as_gc_id() {
        let key = vm.to_string(args[1]);
        Ok(BxValue::new_bool(vm.struct_key_exists(id, &key)))
    } else {
        Err("structKeyExists() expects a struct as the first argument".to_string())
    }
}

fn struct_key_array_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("structKeyArray() expects 1 argument".to_string());
    }
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
    if args.is_empty() {
        return Err("structClear() expects 1 argument".to_string());
    }
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
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    Ok(BxValue::new_number(since_the_epoch.as_millis() as f64))
}

fn sleep(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("sleep() expects exactly 1 argument".to_string());
    }
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
    if args.is_empty() {
        return Err("runAsync() expects at least 1 argument".to_string());
    }
    let priority = if args.len() >= 2 && args[1].is_number() {
        args[1].as_number() as u8
    } else {
        0
    };
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| "No chunk context available".to_string())?;
    vm.spawn_by_value(&args[0], Vec::new(), priority, chunk)
}

fn create_object(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("createObject() expects at least 2 arguments: (type, class)".to_string());
    }
    let obj_type = vm.to_string(args[0]).to_lowercase();
    let class_name = vm.to_string(args[1]);

    match obj_type.as_str() {
        "java" => jni::create_java_object(vm, &class_name, &args[2..]),
        "rust" => vm.construct_native_class(&class_name, &args[2..]),
        "native" => Err("Use 'rust' type for native objects".to_string()),
        _ => Err(format!("Unknown object type: {}", obj_type)),
    }
}

fn is_null_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(true));
    }
    Ok(BxValue::new_bool(args[0].is_null()))
}
