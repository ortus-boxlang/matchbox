use crate::types::{BxVM, BxValue};

#[cfg(feature = "bif-datasource")]
use std::cell::RefCell;
#[cfg(feature = "bif-datasource")]
use std::rc::Rc;
#[cfg(feature = "bif-datasource")]
use std::sync::Arc;

#[cfg(feature = "bif-datasource")]
use crate::datasource::traits::{
    DatasourceConfig, QueryColumn, QueryColumnType, QueryParam, SqlValue,
};
#[cfg(feature = "bif-datasource")]
use crate::datasource::{bx_to_sql, registry, sql_to_bx, BxQuery};

// ─── datasourceRegister ──────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn datasource_register(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("datasourceRegister() expects 2 arguments: (name, configStruct)".to_string());
    }
    let name = vm.to_string(args[0]);
    let cfg_id = args[1]
        .as_gc_id()
        .ok_or_else(|| "datasourceRegister() second argument must be a struct".to_string())?;

    let config = DatasourceConfig {
        driver: vm.to_string(vm.struct_get(cfg_id, "driver")),
        host: vm.to_string(vm.struct_get(cfg_id, "host")),
        port: {
            let v = vm.struct_get(cfg_id, "port");
            if v.is_number() {
                v.as_number() as u16
            } else {
                5432
            }
        },
        database: vm.to_string(vm.struct_get(cfg_id, "database")),
        username: vm.to_string(vm.struct_get(cfg_id, "username")),
        password: vm.to_string(vm.struct_get(cfg_id, "password")),
        max_connections: {
            let v = vm.struct_get(cfg_id, "maxConnections");
            if v.is_number() {
                v.as_number() as u32
            } else {
                10
            }
        },
    };

    let driver_name = config.driver.to_lowercase();

    use crate::datasource::drivers::postgres::PostgresDriver;
    match driver_name.as_str() {
        "postgresql" | "postgres" => {
            let driver = PostgresDriver::new(&config)
                .map_err(|e| format!("Failed to create PostgreSQL datasource '{}': {}", name, e))?;
            registry::register(&name, Arc::new(driver));
            return Ok(BxValue::new_bool(true));
        }
        other => {
            return Err(format!(
                "Unknown datasource driver: '{}'. Supported: postgresql",
                other
            ));
        }
    }
}

// ─── queryExecute ────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_execute(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err(
            "queryExecute() expects at least 1 argument: (sql [, params [, options]])".to_string(),
        );
    }
    let sql = vm.to_string(args[0]);

    let params = if args.len() > 1 && !args[1].is_null() {
        parse_query_params(vm, args[1])?
    } else {
        vec![]
    };

    let (datasource_name, return_type) = if args.len() > 2 && !args[2].is_null() {
        if let Some(opts_id) = args[2].as_gc_id() {
            let ds = {
                let v = vm.struct_get(opts_id, "datasource");
                if v.is_null() {
                    "default".to_string()
                } else {
                    vm.to_string(v)
                }
            };
            let rt = {
                let v = vm.struct_get(opts_id, "returnType");
                if v.is_null() {
                    "query".to_string()
                } else {
                    vm.to_string(v).to_lowercase()
                }
            };
            (ds, rt)
        } else {
            ("default".to_string(), "query".to_string())
        }
    } else {
        ("default".to_string(), "query".to_string())
    };

    let driver = registry::get(&datasource_name).ok_or_else(|| {
        format!(
            "Datasource '{}' not registered. Use datasourceRegister() first.",
            datasource_name
        )
    })?;

    let result = driver.execute(&sql, &params)?;
    let query = BxQuery::from_result(result);

    match return_type.as_str() {
        "array" => query_result_to_array(vm, query),
        "struct" => query_result_to_struct(vm, query),
        _ => {
            let id = vm.native_object_new(Rc::new(RefCell::new(query)));
            Ok(BxValue::new_ptr(id))
        }
    }
}

// ─── queryNew ────────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let col_names: Vec<String> = if let Some(arr_id) = args.first().and_then(|v| v.as_gc_id()) {
        let len = vm.array_len(arr_id);
        (0..len)
            .map(|i| vm.to_string(vm.array_get(arr_id, i)))
            .collect()
    } else {
        return Err("queryNew() requires an array of column names".to_string());
    };

    let col_types: Vec<String> = if args.len() > 1 {
        if let Some(arr_id) = args[1].as_gc_id() {
            let len = vm.array_len(arr_id);
            (0..len)
                .map(|i| vm.to_string(vm.array_get(arr_id, i)))
                .collect()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    let columns: Vec<QueryColumn> = col_names
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            let col_type = col_types
                .get(i)
                .map(|t| parse_col_type(t))
                .unwrap_or(QueryColumnType::Varchar);
            QueryColumn { name, col_type }
        })
        .collect();

    let query = BxQuery::new(columns);
    let id = vm.native_object_new(Rc::new(RefCell::new(query)));
    Ok(BxValue::new_ptr(id))
}

// ─── queryAddRow ─────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_add_row(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryAddRow() expects 2 arguments: (query, dataStruct)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryAddRow() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "addrow", &args[1..])
}

// ─── queryColumnData ─────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_column_data(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryColumnData() expects 2 arguments: (query, columnName)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryColumnData() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "columndata", &args[1..])
}

// ─── queryColumnList ─────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_column_list(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("queryColumnList() expects 1 argument: (query)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryColumnList() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "columnlist", &[])
}

// ─── Transaction stubs ───────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn transaction_begin(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Err("transactionBegin() is not yet implemented. Transaction support is planned for a future release.".to_string())
}
#[cfg(feature = "bif-datasource")]
pub fn transaction_commit(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Err("transactionCommit() is not yet implemented. Transaction support is planned for a future release.".to_string())
}
#[cfg(feature = "bif-datasource")]
pub fn transaction_rollback(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Err("transactionRollback() is not yet implemented. Transaction support is planned for a future release.".to_string())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Coerce a BxValue to the appropriate SqlValue based on a CF SQL type hint.
#[cfg(feature = "bif-datasource")]
fn coerce_cf_sql_type(vm: &mut dyn BxVM, val: BxValue, cf_type: Option<&str>) -> SqlValue {
    match cf_type.map(|s| s.to_uppercase()).as_deref() {
        Some("CF_SQL_BIT") => {
            let s = vm.to_string(val).to_lowercase();
            SqlValue::Bool(s == "true" || s == "1" || s == "yes")
        }
        Some("CF_SQL_INTEGER")
        | Some("CF_SQL_INT")
        | Some("CF_SQL_SMALLINT")
        | Some("CF_SQL_TINYINT")
        | Some("CF_SQL_BIGINT") => {
            let s = vm.to_string(val);
            SqlValue::Int(s.parse::<i64>().unwrap_or(0))
        }
        Some("CF_SQL_FLOAT")
        | Some("CF_SQL_DOUBLE")
        | Some("CF_SQL_DECIMAL")
        | Some("CF_SQL_NUMERIC")
        | Some("CF_SQL_REAL")
        | Some("CF_SQL_MONEY")
        | Some("CF_SQL_SMALLMONEY") => {
            let s = vm.to_string(val);
            SqlValue::Float(s.parse::<f64>().unwrap_or(0.0))
        }
        _ => bx_to_sql(vm, val),
    }
}

#[cfg(feature = "bif-datasource")]
fn parse_query_params(vm: &mut dyn BxVM, val: BxValue) -> Result<Vec<QueryParam>, String> {
    if let Some(arr_id) = val.as_gc_id() {
        let len = vm.array_len(arr_id);
        let mut params = Vec::with_capacity(len);
        for i in 0..len {
            let item = vm.array_get(arr_id, i);
            if let Some(item_id) = item.as_gc_id() {
                if vm.struct_key_exists(item_id, "value") {
                    // CF-style: {value: ..., cfsqltype: "CF_SQL_VARCHAR"}
                    let v = vm.struct_get(item_id, "value");
                    let sql_type_str = {
                        let t = vm.struct_get(item_id, "cfsqltype");
                        if t.is_null() {
                            None
                        } else {
                            Some(vm.to_string(t))
                        }
                    };
                    let sql_val = coerce_cf_sql_type(vm, v, sql_type_str.as_deref());
                    params.push(QueryParam {
                        value: sql_val,
                        sql_type: sql_type_str,
                    });
                } else {
                    // Plain GC value (string, array, etc.)
                    params.push(QueryParam {
                        value: bx_to_sql(vm, item),
                        sql_type: None,
                    });
                }
            } else {
                params.push(QueryParam {
                    value: bx_to_sql(vm, item),
                    sql_type: None,
                });
            }
        }
        Ok(params)
    } else {
        Ok(vec![QueryParam {
            value: bx_to_sql(vm, val),
            sql_type: None,
        }])
    }
}

#[cfg(feature = "bif-datasource")]
fn query_result_to_array(vm: &mut dyn BxVM, query: BxQuery) -> Result<BxValue, String> {
    let arr_id = vm.array_new();
    for row_idx in 0..query.record_count {
        let struct_id = vm.struct_new();
        for (col_idx, col) in query.columns.iter().enumerate() {
            let val = query
                .data
                .get(col_idx)
                .and_then(|col_data| col_data.get(row_idx))
                .cloned()
                .unwrap_or(SqlValue::Null);
            let bx = sql_to_bx(vm, &val);
            vm.struct_set(struct_id, &col.name, bx);
        }
        vm.array_push(arr_id, BxValue::new_ptr(struct_id));
    }
    Ok(BxValue::new_ptr(arr_id))
}

#[cfg(feature = "bif-datasource")]
fn query_result_to_struct(vm: &mut dyn BxVM, query: BxQuery) -> Result<BxValue, String> {
    let outer_id = vm.struct_new();
    for (col_idx, col) in query.columns.iter().enumerate() {
        let arr_id = vm.array_new();
        let col_data = query.data.get(col_idx);
        for row_idx in 0..query.record_count {
            let val = col_data
                .and_then(|d| d.get(row_idx))
                .cloned()
                .unwrap_or(SqlValue::Null);
            let bx = sql_to_bx(vm, &val);
            vm.array_push(arr_id, bx);
        }
        vm.struct_set(outer_id, &col.name, BxValue::new_ptr(arr_id));
    }
    Ok(BxValue::new_ptr(outer_id))
}

#[cfg(feature = "bif-datasource")]
fn parse_col_type(s: &str) -> QueryColumnType {
    match s.to_lowercase().as_str() {
        "varchar" | "string" | "cf_sql_varchar" | "cf_sql_char" | "cf_sql_longvarchar" => {
            QueryColumnType::Varchar
        }
        "integer" | "int" | "cf_sql_integer" | "cf_sql_smallint" | "cf_sql_tinyint" => {
            QueryColumnType::Integer
        }
        "bigint" | "cf_sql_bigint" => QueryColumnType::BigInt,
        "double" | "float" | "cf_sql_double" | "cf_sql_float" | "cf_sql_real" => {
            QueryColumnType::Double
        }
        "decimal" | "numeric" | "cf_sql_decimal" | "cf_sql_numeric" | "cf_sql_money" => {
            QueryColumnType::Decimal
        }
        "boolean" | "bit" | "cf_sql_bit" => QueryColumnType::Boolean,
        "date" | "cf_sql_date" => QueryColumnType::Date,
        "timestamp" | "datetime" | "cf_sql_timestamp" => QueryColumnType::Timestamp,
        "blob" | "binary" | "cf_sql_blob" | "cf_sql_binary" | "cf_sql_varbinary" => {
            QueryColumnType::Blob
        }
        other => QueryColumnType::Other(other.to_string()),
    }
}
