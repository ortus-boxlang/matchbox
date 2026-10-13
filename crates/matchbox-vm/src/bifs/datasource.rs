#[cfg(feature = "bif-datasource")]
use crate::types::{BxVM, BxValue};

#[cfg(feature = "bif-datasource")]
use crate::datasource::traits::{
    DatasourceConfig, QueryColumn, QueryColumnType, QueryParam, SqlValue,
};
#[cfg(feature = "bif-datasource")]
use crate::datasource::{BxQuery, bx_to_sql, registry, sql_to_bx};
#[cfg(all(
    feature = "bif-datasource",
    feature = "qoq",
    not(target_arch = "wasm32")
))]
use crate::qoq;
#[cfg(feature = "bif-datasource")]
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
#[cfg(feature = "bif-datasource")]
use std::rc::Rc;
#[cfg(feature = "bif-datasource")]
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
#[cfg(feature = "bif-datasource")]
use std::time::Instant;

#[cfg(feature = "bif-datasource")]
static QUERY_CACHE: OnceLock<
    Mutex<HashMap<String, (crate::datasource::traits::QueryResult, Instant)>>,
> = OnceLock::new();
#[cfg(feature = "bif-datasource")]
static TRANSACTION_CONTEXTS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

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
    use crate::datasource::drivers::sqlite::SqliteDriver;
    match driver_name.as_str() {
        "postgresql" | "postgres" => {
            let driver = PostgresDriver::new(&config)
                .map_err(|e| format!("Failed to create PostgreSQL datasource '{}': {}", name, e))?;
            registry::register(&name, Arc::new(driver));
            return Ok(BxValue::new_bool(true));
        }
        "sqlite" => {
            let driver = SqliteDriver::new(&config)
                .map_err(|e| format!("Failed to create SQLite datasource '{}': {}", name, e))?;
            registry::register(&name, Arc::new(driver));
            return Ok(BxValue::new_bool(true));
        }
        other => {
            return Err(format!(
                "Unknown datasource driver: '{}'. Supported: postgresql, sqlite",
                other
            ));
        }
    }
}

#[cfg(feature = "bif-datasource")]
pub fn datasource_unregister(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("datasourceUnregister() expects a datasource name".to_string());
    }
    registry::unregister(&vm.to_string(args[0]));
    Ok(BxValue::new_bool(true))
}

// ─── queryExecute ────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_execute(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err(
            "queryExecute() expects at least 1 argument: (sql [, params [, options]])".to_string(),
        );
    }
    let raw_sql = vm.to_string(args[0]);

    let (sql, params) = if args.len() > 1 && !args[1].is_null() {
        expand_query_parameters(vm, &raw_sql, args[1])?
    } else {
        (raw_sql, vec![])
    };
    let (
        datasource_name,
        return_type,
        db_type,
        column_key,
        result_name,
        max_rows,
        cache_enabled,
        cache_key,
        cache_timeout,
        cache_provider,
        cache_last_access_timeout,
        transformer,
    ) = if args.len() > 2 && !args[2].is_null() {
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
            let dbt = {
                let v = vm.struct_get(opts_id, "dbtype");
                if v.is_null() {
                    String::new()
                } else {
                    vm.to_string(v).to_lowercase()
                }
            };
            let ck = {
                let value = vm.struct_get(opts_id, "columnKey");
                if value.is_null() {
                    None
                } else {
                    Some(vm.to_string(value))
                }
            };
            let result = {
                let value = vm.struct_get(opts_id, "result");
                if value.is_null() {
                    None
                } else {
                    Some(vm.to_string(value))
                }
            };
            let max_rows = ["maxRows", "maxrows"]
                .iter()
                .map(|key| vm.struct_get(opts_id, key))
                .find(|value| value.is_number())
                .map(|value| value.as_number().max(0.0) as usize);
            let cache_enabled = vm.struct_get(opts_id, "cache").as_bool();
            let cache_key = {
                let value = vm.struct_get(opts_id, "cacheKey");
                if value.is_null() {
                    None
                } else {
                    Some(vm.to_string(value))
                }
            };
            let cache_timeout = {
                let value = vm.struct_get(opts_id, "cacheTimeout");
                if value.is_number() {
                    Some(value.as_number())
                } else {
                    None
                }
            };
            let cache_provider = {
                let value = vm.struct_get(opts_id, "cacheProvider");
                if value.is_null() {
                    None
                } else {
                    Some(vm.to_string(value))
                }
            };
            let cache_last_access_timeout = {
                let value = vm.struct_get(opts_id, "cacheLastAccessTimeout");
                if value.is_number() {
                    Some(value.as_number())
                } else {
                    None
                }
            };
            let transformer = {
                let value = vm.struct_get(opts_id, "transformer");
                if value.is_null() { None } else { Some(value) }
            };
            (
                ds,
                rt,
                dbt,
                ck,
                result,
                max_rows,
                cache_enabled,
                cache_key,
                cache_timeout,
                cache_provider,
                cache_last_access_timeout,
                transformer,
            )
        } else {
            (
                "default".to_string(),
                "query".to_string(),
                String::new(),
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
            )
        }
    } else {
        (
            "default".to_string(),
            "query".to_string(),
            String::new(),
            None,
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
    };

    if db_type == "query" {
        #[cfg(feature = "qoq")]
        {
            let qoq_params = if args.len() > 1 && !args[1].is_null() {
                parse_qoq_bind_params(vm, args[1])?
            } else {
                qoq::BindParams::default()
            };
            return query_execute_qoq(vm, &sql, &return_type, &qoq_params);
        }
        #[cfg(not(feature = "qoq"))]
        {
            return Err("QoQ support is not enabled in this build".to_string());
        }
    }

    let driver = registry::get(&datasource_name).ok_or_else(|| {
        if datasource_name.eq_ignore_ascii_case("default") {
            "No default datasource is registered. Use datasourceRegister() first.".to_string()
        } else {
            format!(
                "Datasource with name [{}] not found. Use datasourceRegister() first.",
                datasource_name
            )
        }
    })?;

    let configured_cache_key = cache_key.clone();
    let cache_key =
        cache_key.unwrap_or_else(|| format!("{}:{}:{:?}", datasource_name, sql, params));
    let cache_expired = cache_timeout.is_some_and(|timeout| timeout < 0.0);
    if cache_expired {
        if let Some(cache) = QUERY_CACHE.get() {
            cache
                .lock()
                .map_err(|_| "Query cache is poisoned".to_string())?
                .clear();
        }
    }
    let mut cached = false;
    let result = if cache_enabled && !cache_expired {
        let cache = QUERY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut cache = cache
            .lock()
            .map_err(|_| "Query cache is poisoned".to_string())?;
        let valid = cache.get(&cache_key).is_some_and(|(_, created)| {
            cache_timeout
                .is_none_or(|timeout| timeout <= 0.0 || created.elapsed().as_secs_f64() < timeout)
        });
        if valid {
            cached = true;
            cache
                .get(&cache_key)
                .map(|(query, _)| query.clone())
                .unwrap()
        } else {
            let result = driver.execute(&sql, &params)?;
            cache.insert(cache_key.clone(), (result.clone(), Instant::now()));
            result
        }
    } else {
        driver.execute(&sql, &params)?
    };
    let mut query = BxQuery::from_result(result);
    if query.columns.is_empty() {
        if let Some(cache) = QUERY_CACHE.get() {
            cache
                .lock()
                .map_err(|_| "Query cache is poisoned".to_string())?
                .clear();
        }
    }
    if let Some(max_rows) = max_rows {
        let row_count = query.record_count.min(max_rows);
        for column in &mut query.data {
            column.truncate(row_count);
        }
        query.record_count = row_count;
    }
    let metadata = if result_name.is_some() || transformer.is_some() {
        let metadata = query_result_metadata(
            vm,
            &sql,
            &params,
            &query,
            cached,
            cache_enabled,
            configured_cache_key.as_deref(),
            cache_timeout,
            cache_provider.as_deref(),
            cache_last_access_timeout,
        );
        if let Some(result_name) = result_name {
            vm.insert_global(result_name, BxValue::new_ptr(metadata));
        }
        Some(metadata)
    } else {
        None
    };

    if let Some(transformer) = transformer {
        if vm.is_string_value(transformer) {
            return Err(format!(
                "Query transformer '{}' not found",
                vm.to_string(transformer)
            ));
        }
        let query_id = vm.native_object_new(Rc::new(RefCell::new(query)));
        let metadata =
            metadata.ok_or_else(|| "Query transformer metadata was not created".to_string())?;
        let chunk = vm
            .current_chunk()
            .ok_or_else(|| "Query transformer requires an active execution chunk".to_string())?;
        let transformer_args = vec![BxValue::new_ptr(query_id), BxValue::new_ptr(metadata)];
        return match vm.call_function_by_value(&transformer, transformer_args.clone(), chunk) {
            Err(error) if error.contains("not a callable function") => vm.call_method_by_value(
                transformer,
                "transform",
                transformer_args,
                vm.current_chunk().ok_or_else(|| {
                    "Query transformer requires an active execution chunk".to_string()
                })?,
            ),
            result => result,
        };
    }

    match return_type.as_str() {
        "array" => query_result_to_array(vm, query),
        "struct" => query_result_to_struct(vm, query, column_key.as_deref()),
        "query" => {
            let id = vm.native_object_new(Rc::new(RefCell::new(query)));
            Ok(BxValue::new_ptr(id))
        }
        other => Err(format!("Unknown return type: {}", other)),
    }
}

#[cfg(all(
    feature = "bif-datasource",
    feature = "qoq",
    not(target_arch = "wasm32")
))]
fn query_execute_qoq(
    vm: &mut dyn BxVM,
    sql: &str,
    return_type: &str,
    params: &qoq::BindParams,
) -> Result<BxValue, String> {
    let query = parse_cached_qoq(sql)?;
    let query =
        qoq::bind_params(&query, params).map_err(|e| qoq_error_to_string("QoQ bind error", e))?;
    let result = {
        let vm_ref: &dyn BxVM = &*vm;
        qoq::execute_with_source_resolver(&query, move |path| resolve_qoq_source(vm_ref, path))
            .map_err(|e| qoq_error_to_string("QoQ execution error", e))?
    };
    let query = BxQuery::from_result(result);

    match return_type {
        "array" => query_result_to_array(vm, query),
        "struct" => query_result_to_struct(vm, query, None),
        _ => {
            let id = vm.native_object_new(Rc::new(RefCell::new(query)));
            Ok(BxValue::new_ptr(id))
        }
    }
}

#[cfg(all(
    feature = "bif-datasource",
    feature = "qoq",
    not(target_arch = "wasm32")
))]
static QOQ_PARSE_CACHE: OnceLock<Mutex<HashMap<String, qoq::Query>>> = OnceLock::new();

#[cfg(all(
    feature = "bif-datasource",
    feature = "qoq",
    not(target_arch = "wasm32")
))]
const QOQ_PARSE_CACHE_LIMIT: usize = 128;

#[cfg(all(
    feature = "bif-datasource",
    feature = "qoq",
    not(target_arch = "wasm32")
))]
fn parse_cached_qoq(sql: &str) -> Result<qoq::Query, String> {
    let cache = QOQ_PARSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(query) = cache
        .lock()
        .map_err(|_| "QoQ parse cache is poisoned".to_string())?
        .get(sql)
        .cloned()
    {
        return Ok(query);
    }

    let query = qoq::parse(sql).map_err(|e| format!("QoQ parse error: {}", e))?;
    let mut cache = cache
        .lock()
        .map_err(|_| "QoQ parse cache is poisoned".to_string())?;
    if cache.len() >= QOQ_PARSE_CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(sql.to_string(), query.clone());
    Ok(query)
}

#[cfg(all(
    feature = "bif-datasource",
    feature = "qoq",
    not(target_arch = "wasm32")
))]
struct VmQuerySource<'a> {
    vm: &'a dyn BxVM,
    id: usize,
    columns: Vec<QueryColumn>,
    row_count: usize,
}

#[cfg(all(
    feature = "bif-datasource",
    feature = "qoq",
    not(target_arch = "wasm32")
))]
impl<'a> qoq::QuerySource for VmQuerySource<'a> {
    fn columns(&self) -> &[QueryColumn] {
        &self.columns
    }

    fn row_count(&self) -> usize {
        self.row_count
    }

    fn value(&self, row_idx: usize, col_idx: usize) -> SqlValue {
        self.vm
            .native_object_query_cell(self.id, row_idx, col_idx)
            .unwrap_or(SqlValue::Null)
    }
}

#[cfg(all(
    feature = "bif-datasource",
    feature = "qoq",
    not(target_arch = "wasm32")
))]
fn resolve_qoq_source<'a>(
    vm: &'a dyn BxVM,
    path: &[String],
) -> Result<Option<Box<dyn qoq::QuerySource + 'a>>, qoq::ExecutionError> {
    let key = path.join(".");
    let value = vm
        .resolve_query_source_path(path)
        .ok_or_else(|| qoq::ExecutionError {
            message: format!("unknown QoQ source '{}'", key),
            span: None,
        })?;
    let id = value.as_gc_id().ok_or_else(|| qoq::ExecutionError {
        message: format!("QoQ source '{}' is not a query object", key),
        span: None,
    })?;
    let columns = vm
        .native_object_query_columns(id)
        .ok_or_else(|| qoq::ExecutionError {
            message: format!("QoQ source '{}' is not a query object", key),
            span: None,
        })?;
    let row_count = vm
        .native_object_query_row_count(id)
        .ok_or_else(|| qoq::ExecutionError {
            message: format!("QoQ source '{}' is not a query object", key),
            span: None,
        })?;
    Ok(Some(Box::new(VmQuerySource {
        vm,
        id,
        columns,
        row_count,
    })))
}

#[cfg(all(
    feature = "bif-datasource",
    feature = "qoq",
    not(target_arch = "wasm32")
))]
fn qoq_error_to_string(prefix: &str, e: qoq::ExecutionError) -> String {
    if let Some(span) = e.span {
        format!(
            "{}: {} at line {}, col {}",
            prefix, e.message, span.line, span.col
        )
    } else {
        format!("{}: {}", prefix, e.message)
    }
}

#[cfg(all(
    feature = "bif-datasource",
    feature = "qoq",
    not(target_arch = "wasm32")
))]
fn parse_qoq_bind_params(vm: &mut dyn BxVM, val: BxValue) -> Result<qoq::BindParams, String> {
    if let Some(id) = val.as_gc_id() {
        if vm.struct_len(id) > 0 {
            let mut named = std::collections::HashMap::new();
            for key in vm.struct_key_array(id) {
                let item = vm.struct_get(id, &key);
                let sql_val = if let Some(item_id) = item.as_gc_id() {
                    if vm.struct_key_exists(item_id, "value") {
                        let v = vm.struct_get(item_id, "value");
                        let sql_type_str = {
                            let t = vm.struct_get(item_id, "cfsqltype");
                            if t.is_null() {
                                None
                            } else {
                                Some(vm.to_string(t))
                            }
                        };
                        coerce_cf_sql_type(vm, v, sql_type_str.as_deref())
                    } else {
                        bx_to_sql(vm, item)
                    }
                } else {
                    bx_to_sql(vm, item)
                };
                named.insert(key.to_lowercase(), sql_val);
            }
            Ok(qoq::BindParams {
                positional: vec![],
                named,
            })
        } else {
            let len = vm.array_len(id);
            let mut positional = Vec::with_capacity(len);
            for i in 0..len {
                let item = vm.array_get(id, i);
                let sql_val = if let Some(item_id) = item.as_gc_id() {
                    if vm.struct_key_exists(item_id, "value") {
                        let v = vm.struct_get(item_id, "value");
                        let sql_type_str = {
                            let t = vm.struct_get(item_id, "cfsqltype");
                            if t.is_null() {
                                None
                            } else {
                                Some(vm.to_string(t))
                            }
                        };
                        coerce_cf_sql_type(vm, v, sql_type_str.as_deref())
                    } else {
                        bx_to_sql(vm, item)
                    }
                } else {
                    bx_to_sql(vm, item)
                };
                positional.push(sql_val);
            }
            Ok(qoq::BindParams {
                positional,
                named: std::collections::HashMap::new(),
            })
        }
    } else {
        Ok(qoq::BindParams {
            positional: vec![bx_to_sql(vm, val)],
            named: std::collections::HashMap::new(),
        })
    }
}

#[cfg(all(
    feature = "bif-datasource",
    not(feature = "qoq"),
    not(target_arch = "wasm32")
))]
fn query_execute_qoq(
    _vm: &mut dyn BxVM,
    _sql: &str,
    _return_type: &str,
) -> Result<BxValue, String> {
    Err("QoQ support is not enabled in this build".to_string())
}

// ─── queryNew ────────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let column_arg = args
        .first()
        .ok_or_else(|| "queryNew() requires a column list".to_string())?;
    let col_names = query_list_arg(vm, *column_arg, "columnList")?;

    let col_types = if args.len() > 1 {
        query_list_arg(vm, args[1], "columnTypeList")?
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

    let mut query = BxQuery::new(columns);
    if args.len() > 2 && !args[2].is_null() {
        if vm.is_struct_value(args[2]) {
            add_query_struct_row(vm, &mut query, args[2])?;
        } else {
            populate_query_rows(vm, &mut query, args[2])?;
        }
    }

    let id = vm.native_object_new(Rc::new(RefCell::new(query)));
    Ok(BxValue::new_ptr(id))
}

#[cfg(feature = "bif-datasource")]
fn query_list_arg(vm: &dyn BxVM, value: BxValue, name: &str) -> Result<Vec<String>, String> {
    if value.is_null() {
        return Ok(vec![]);
    }

    if vm.is_array_value(value) {
        let arr_id = value
            .as_gc_id()
            .ok_or_else(|| format!("queryNew() {name} array is invalid"))?;
        return Ok((0..vm.array_len(arr_id))
            .map(|i| vm.to_string(vm.array_get(arr_id, i)).trim().to_string())
            .filter(|s| !s.is_empty())
            .collect());
    }

    if vm.is_string_value(value) {
        return Ok(vm
            .to_string(value)
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect());
    }

    Err(format!(
        "queryNew() {name} must be a comma-delimited string or array"
    ))
}

#[cfg(feature = "bif-datasource")]
fn populate_query_rows(
    vm: &mut dyn BxVM,
    query: &mut BxQuery,
    row_data: BxValue,
) -> Result<(), String> {
    let row_data_id = row_data
        .as_gc_id()
        .filter(|_| vm.is_array_value(row_data))
        .ok_or_else(|| "queryNew() rowData must be an array".to_string())?;

    for row_idx in 0..vm.array_len(row_data_id) {
        let row = vm.array_get(row_data_id, row_idx);
        if vm.is_struct_value(row) {
            add_query_struct_row(vm, query, row)?;
        } else if vm.is_array_value(row) {
            add_query_array_row(vm, query, row)?;
        } else if query.columns.len() == 1 {
            add_query_scalar_row(query, row, vm);
        } else {
            return Err("queryNew() rowData entries must be structs or arrays".to_string());
        }
    }
    Ok(())
}

#[cfg(feature = "bif-datasource")]
fn add_query_struct_row(
    vm: &mut dyn BxVM,
    query: &mut BxQuery,
    row: BxValue,
) -> Result<(), String> {
    let struct_id = row
        .as_gc_id()
        .ok_or_else(|| "queryNew() row struct is invalid".to_string())?;
    for (col_idx, col) in query.columns.iter().enumerate() {
        let bx_val = vm.struct_get(struct_id, &col.name);
        query.data[col_idx].push(bx_to_sql(vm, bx_val));
    }
    query.record_count += 1;
    Ok(())
}

#[cfg(feature = "bif-datasource")]
fn add_query_array_row(vm: &mut dyn BxVM, query: &mut BxQuery, row: BxValue) -> Result<(), String> {
    let row_id = row
        .as_gc_id()
        .ok_or_else(|| "queryNew() row array is invalid".to_string())?;
    for col_idx in 0..query.columns.len() {
        let bx_val = if col_idx < vm.array_len(row_id) {
            vm.array_get(row_id, col_idx)
        } else {
            BxValue::new_null()
        };
        query.data[col_idx].push(bx_to_sql(vm, bx_val));
    }
    query.record_count += 1;
    Ok(())
}

#[cfg(feature = "bif-datasource")]
fn add_query_scalar_row(query: &mut BxQuery, row: BxValue, vm: &dyn BxVM) {
    query.data[0].push(bx_to_sql(vm, row));
    query.record_count += 1;
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

// ─── queryClear ──────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_clear(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("queryClear() expects 1 argument: (query)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryClear() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "clear", &[])
}

// ─── queryColumnArray ────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_column_array(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("queryColumnArray() expects 1 argument: (query)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryColumnArray() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "columnarray", &[])
}

// ─── queryColumnCount ────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_column_count(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("queryColumnCount() expects 1 argument: (query)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryColumnCount() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "columncount", &[])
}

// ─── queryColumnExists ───────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_column_exists(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryColumnExists() expects 2 arguments: (query, columnName)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryColumnExists() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "columnexists", &args[1..])
}

// ─── queryKeyExists ──────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_key_exists(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryKeyExists() expects 2 arguments: (query, key)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryKeyExists() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "keyexists", &args[1..])
}

// ─── queryRecordCount ────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_record_count(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("queryRecordCount() expects 1 argument: (query)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryRecordCount() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "recordcount", &[])
}

// ─── queryRowData ────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_row_data(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryRowData() expects 2 arguments: (query, rowNumber)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryRowData() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "getrow", &args[1..])
}

// ─── queryReverse ────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_reverse(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("queryReverse() expects 1 argument: (query)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryReverse() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "reverse", &[])
}

// ─── queryRowSwap ────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_row_swap(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("queryRowSwap() expects 3 arguments: (query, source, destination)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryRowSwap() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "rowswap", &args[1..])
}

// ─── querySlice ──────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_slice(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err(
            "querySlice() expects at least 2 arguments: (query, offset [, length])".to_string(),
        );
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "querySlice() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "slice", &args[1..])
}

// ─── queryDeleteColumn ───────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_delete_column(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryDeleteColumn() expects 2 arguments: (query, columnName)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryDeleteColumn() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "deletecolumn", &args[1..])
}

// ─── queryDeleteRow ──────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_delete_row(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryDeleteRow() expects 2 arguments: (query, row)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryDeleteRow() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "deleterow", &args[1..])
}

// ─── queryAddColumn ──────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_add_column(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryAddColumn() expects at least 2 arguments: (query, columnName [, datatype, arrayData])".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryAddColumn() first argument must be a query object".to_string())?;
    // Build args for native method: columnName, datatype, arrayData
    let col_name = args.get(1).copied().unwrap_or(BxValue::new_null());
    let (datatype, array_data) = if args.get(2).is_some_and(|value| vm.is_array_value(*value)) {
        (
            BxValue::new_ptr(vm.string_new("object".to_string())),
            args[2],
        )
    } else {
        (
            args.get(2)
                .copied()
                .unwrap_or_else(|| BxValue::new_ptr(vm.string_new("object".to_string()))),
            args.get(3).copied().unwrap_or(BxValue::new_null()),
        )
    };
    vm.native_object_call_method(query_id, "addcolumn", &[col_name, datatype, array_data])
}

// ─── queryAppend ─────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_append(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryAppend() expects 2 arguments: (query1, query2)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryAppend() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "append", &args[1..])
}

// ─── queryPrepend ────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_prepend(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("queryPrepend() expects 2 arguments: (query1, query2)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryPrepend() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "prepend", &args[1..])
}

// ─── queryGetCell ────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_get_cell(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err(
            "queryGetCell() expects at least 2 arguments: (query, columnName [, rowNumber])"
                .to_string(),
        );
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryGetCell() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "getcell", &args[1..])
}

// ─── querySetCell ────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_set_cell(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err(
            "querySetCell() expects at least 3 arguments: (query, columnName, value [, rowNumber])"
                .to_string(),
        );
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "querySetCell() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "setcell", &args[1..])
}

// ─── queryInsertAt ───────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_insert_at(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("queryInsertAt() expects 3 arguments: (query, value, position)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryInsertAt() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "insertat", &args[1..])
}

// ─── querySetRow ─────────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_set_row(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err(
            "querySetRow() expects at least 2 arguments: (query, rowData [, rowNumber])"
                .to_string(),
        );
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "querySetRow() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "setrow", &args[1..])
}

// ─── queryCurrentRow ─────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_current_row(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("queryCurrentRow() expects 1 argument: (query)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryCurrentRow() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "currentrow", &[])
}

// ─── queryGetResult ──────────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn query_get_result(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("queryGetResult() expects 1 argument: (query)".to_string());
    }
    let query_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "queryGetResult() first argument must be a query object".to_string())?;
    vm.native_object_call_method(query_id, "getresult", &[])
}

// ─── Transaction context ─────────────────────────────────────────────────────
#[cfg(feature = "bif-datasource")]
pub fn transaction_begin(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let contexts = TRANSACTION_CONTEXTS.get_or_init(|| Mutex::new(HashSet::new()));
    contexts
        .lock()
        .map_err(|_| "Transaction context lock is poisoned".to_string())?
        .insert(vm as *mut dyn BxVM as *mut () as usize);
    Ok(BxValue::new_bool(true))
}
#[cfg(feature = "bif-datasource")]
pub fn transaction_commit(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let contexts = TRANSACTION_CONTEXTS.get_or_init(|| Mutex::new(HashSet::new()));
    let removed = contexts
        .lock()
        .map_err(|_| "Transaction context lock is poisoned".to_string())?
        .remove(&(vm as *mut dyn BxVM as *mut () as usize));
    if removed {
        Ok(BxValue::new_bool(true))
    } else {
        Err(
            "Transaction not started; Please place this method call inside a transaction{} block."
                .to_string(),
        )
    }
}
#[cfg(feature = "bif-datasource")]
pub fn transaction_rollback(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let contexts = TRANSACTION_CONTEXTS.get_or_init(|| Mutex::new(HashSet::new()));
    let removed = contexts
        .lock()
        .map_err(|_| "Transaction context lock is poisoned".to_string())?
        .remove(&(vm as *mut dyn BxVM as *mut () as usize));
    if removed {
        Ok(BxValue::new_bool(true))
    } else {
        Err(
            "Transaction not started; Please place this method call inside a transaction{} block."
                .to_string(),
        )
    }
}
#[cfg(feature = "bif-datasource")]
pub fn is_within_transaction(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let contexts = TRANSACTION_CONTEXTS.get_or_init(|| Mutex::new(HashSet::new()));
    let active = contexts
        .lock()
        .map_err(|_| "Transaction context lock is poisoned".to_string())?
        .contains(&(vm as *mut dyn BxVM as *mut () as usize));
    Ok(BxValue::new_bool(active))
}
#[cfg(feature = "bif-datasource")]
pub fn is_in_transaction(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    is_within_transaction(vm, args)
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
fn expand_query_parameters(
    vm: &mut dyn BxVM,
    sql: &str,
    value: BxValue,
) -> Result<(String, Vec<QueryParam>), String> {
    let params_id = value
        .as_gc_id()
        .ok_or_else(|| "queryExecute() parameters must be an array or struct".to_string())?;
    if vm.is_struct_value(value) {
        return expand_named_parameters(vm, sql, params_id);
    }
    if !vm.is_array_value(value) {
        return Ok((
            sql.to_string(),
            vec![QueryParam {
                value: bx_to_sql(vm, value),
                sql_type: None,
            }],
        ));
    }

    let named_params = vm.struct_new();
    let mut has_named_params = false;
    for index in 0..vm.array_len(params_id) {
        let item = vm.array_get(params_id, index);
        if let Some(item_id) = item.as_gc_id().filter(|_| vm.is_struct_value(item)) {
            let name = vm.struct_get(item_id, "name");
            if !name.is_null() {
                vm.struct_set(named_params, &vm.to_string(name), item);
                has_named_params = true;
            }
        }
    }
    if has_named_params {
        return expand_named_parameters(vm, sql, named_params);
    }

    let mut output = String::with_capacity(sql.len());
    let mut params = Vec::new();
    let mut param_index = 0;
    let mut in_string = false;
    for character in sql.chars() {
        if character == '\'' {
            in_string = !in_string;
            output.push(character);
            continue;
        }
        if character != '?' || in_string || param_index >= vm.array_len(params_id) {
            output.push(character);
            continue;
        }
        let item = vm.array_get(params_id, param_index);
        param_index += 1;
        let values = parameter_values(vm, item)?;
        output.push_str(&placeholder_list(values.len()));
        params.extend(values);
    }
    Ok((output, params))
}

#[cfg(feature = "bif-datasource")]
fn expand_named_parameters(
    vm: &mut dyn BxVM,
    sql: &str,
    params_id: usize,
) -> Result<(String, Vec<QueryParam>), String> {
    let mut output = String::with_capacity(sql.len());
    let mut params = Vec::new();
    let characters: Vec<char> = sql.chars().collect();
    let mut index = 0;
    let mut in_string = false;
    while index < characters.len() {
        let character = characters[index];
        if character == '\'' {
            in_string = !in_string;
            output.push(character);
            index += 1;
            continue;
        }
        if !in_string
            && character == ':'
            && characters.get(index + 1).is_some_and(|next| *next != ':')
        {
            let start = index + 1;
            let mut end = start;
            while characters
                .get(end)
                .is_some_and(|next| next.is_ascii_alphanumeric() || *next == '_')
            {
                end += 1;
            }
            if end > start {
                let name: String = characters[start..end].iter().collect();
                if let Some(key) = vm
                    .struct_key_array(params_id)
                    .into_iter()
                    .find(|key| key.eq_ignore_ascii_case(&name))
                {
                    let parameter = vm.struct_get(params_id, &key);
                    let values = parameter_values(vm, parameter)?;
                    output.push_str(&placeholder_list(values.len()));
                    params.extend(values);
                    index = end;
                    continue;
                }
                return Err(format!(
                    "Named parameter [:{}] not provided to query.",
                    name
                ));
            }
        }
        output.push(character);
        index += 1;
    }
    Ok((output, params))
}

#[cfg(feature = "bif-datasource")]
fn parameter_values(vm: &mut dyn BxVM, value: BxValue) -> Result<Vec<QueryParam>, String> {
    if let Some(item_id) = value.as_gc_id().filter(|_| vm.is_struct_value(value)) {
        if vm.struct_key_exists(item_id, "value") {
            let raw_value = vm.struct_get(item_id, "value");
            let sql_type = ["cfsqltype", "sqltype"]
                .iter()
                .map(|key| vm.struct_get(item_id, key))
                .find(|value| !value.is_null())
                .map(|value| vm.to_string(value));
            if let Some(sql_type) = sql_type.as_deref() {
                if !is_known_sql_type(sql_type) {
                    return Err(format!("Unknown QueryColumnType: {}", sql_type));
                }
            }
            let is_list = vm.struct_get(item_id, "list").as_bool();
            if is_list {
                return list_parameter_values(vm, raw_value, sql_type.as_deref());
            }
            return Ok(vec![QueryParam {
                value: coerce_query_value(vm, raw_value, sql_type.as_deref()),
                sql_type,
            }]);
        }
    }
    Ok(vec![QueryParam {
        value: bx_to_sql(vm, value),
        sql_type: None,
    }])
}

#[cfg(feature = "bif-datasource")]
fn list_parameter_values(
    vm: &mut dyn BxVM,
    value: BxValue,
    sql_type: Option<&str>,
) -> Result<Vec<QueryParam>, String> {
    if let Some(array_id) = value.as_gc_id().filter(|_| vm.is_array_value(value)) {
        return Ok((0..vm.array_len(array_id))
            .map(|index| {
                let item = vm.array_get(array_id, index);
                QueryParam {
                    value: coerce_query_value(vm, item, sql_type),
                    sql_type: sql_type.map(str::to_string),
                }
            })
            .collect());
    }
    Ok(vm
        .to_string(value)
        .split(',')
        .map(|item| {
            let item_value = BxValue::new_ptr(vm.string_new(item.trim().to_string()));
            QueryParam {
                value: coerce_query_value(vm, item_value, sql_type),
                sql_type: sql_type.map(str::to_string),
            }
        })
        .collect())
}

#[cfg(feature = "bif-datasource")]
fn placeholder_list(count: usize) -> String {
    std::iter::repeat_n("?", count.max(1))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(feature = "bif-datasource")]
fn coerce_query_value(vm: &mut dyn BxVM, value: BxValue, sql_type: Option<&str>) -> SqlValue {
    if sql_type.is_none() && vm.is_string_value(value) {
        let text = vm.to_string(value);
        if let Ok(integer) = text.parse::<i64>() {
            return SqlValue::Int(integer);
        }
        if let Ok(number) = text.parse::<f64>() {
            return SqlValue::Float(number);
        }
    }
    coerce_cf_sql_type(vm, value, sql_type)
}

#[cfg(feature = "bif-datasource")]
fn is_known_sql_type(value: &str) -> bool {
    matches!(
        value.to_ascii_uppercase().as_str(),
        "CF_SQL_BIT"
            | "CF_SQL_INTEGER"
            | "CF_SQL_INT"
            | "CF_SQL_SMALLINT"
            | "CF_SQL_TINYINT"
            | "CF_SQL_BIGINT"
            | "CF_SQL_FLOAT"
            | "CF_SQL_DOUBLE"
            | "CF_SQL_DECIMAL"
            | "CF_SQL_NUMERIC"
            | "CF_SQL_REAL"
            | "CF_SQL_MONEY"
            | "CF_SQL_SMALLMONEY"
            | "BIT"
            | "INTEGER"
            | "INT"
            | "SMALLINT"
            | "TINYINT"
            | "BIGINT"
            | "FLOAT"
            | "DOUBLE"
            | "DECIMAL"
            | "NUMERIC"
            | "REAL"
            | "MONEY"
            | "SMALLMONEY"
            | "VARCHAR"
            | "CHAR"
            | "LONGVARCHAR"
            | "NVARCHAR"
            | "NCHAR"
            | "DATE"
            | "TIME"
            | "TIMESTAMP"
            | "BINARY"
            | "VARBINARY"
            | "LONGVARBINARY"
            | "BLOB"
            | "CLOB"
            | "SQLXML"
    )
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
fn query_result_metadata(
    vm: &mut dyn BxVM,
    sql: &str,
    params: &[QueryParam],
    query: &BxQuery,
    cached: bool,
    cache_enabled: bool,
    cache_key: Option<&str>,
    cache_timeout: Option<f64>,
    cache_provider: Option<&str>,
    cache_last_access_timeout: Option<f64>,
) -> usize {
    let metadata_id = vm.struct_new();
    let rendered_sql = BxValue::new_ptr(vm.string_new(render_sql(sql, params)));
    vm.struct_set(metadata_id, "sql", rendered_sql);
    let parameter_array = vm.array_new();
    for parameter in params {
        let value = sql_to_bx(vm, &parameter.value);
        vm.array_push(parameter_array, value);
    }
    vm.struct_set(
        metadata_id,
        "sqlParameters",
        BxValue::new_ptr(parameter_array),
    );
    vm.struct_set(
        metadata_id,
        "recordCount",
        BxValue::new_number(query.record_count as f64),
    );
    let column_list = query
        .columns
        .iter()
        .map(|column| column.name.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(",");
    let column_list = BxValue::new_ptr(vm.string_new(column_list));
    vm.struct_set(metadata_id, "columnList", column_list);
    vm.struct_set(metadata_id, "executionTime", BxValue::new_number(0.0));
    vm.struct_set(metadata_id, "cached", BxValue::new_bool(cached));
    if cache_enabled {
        let provider = cache_provider.unwrap_or("default");
        let provider = BxValue::new_ptr(vm.string_new(provider.to_string()));
        vm.struct_set(metadata_id, "cacheProvider", provider);
        if let Some(cache_key) = cache_key {
            let cache_key = BxValue::new_ptr(vm.string_new(cache_key.to_string()));
            vm.struct_set(metadata_id, "cacheKey", cache_key);
        }
        if let Some(cache_timeout) = cache_timeout {
            vm.struct_set(
                metadata_id,
                "cacheTimeout",
                BxValue::new_number(cache_timeout),
            );
        }
        if let Some(timeout) = cache_last_access_timeout {
            vm.struct_set(
                metadata_id,
                "cacheLastAccessTimeout",
                BxValue::new_number(timeout),
            );
        }
    }
    metadata_id
}

#[cfg(feature = "bif-datasource")]
fn render_sql(sql: &str, params: &[QueryParam]) -> String {
    let mut rendered = String::with_capacity(sql.len());
    let mut parameter_index = 0;
    let mut in_string = false;
    for character in sql.chars() {
        if character == '\'' {
            in_string = !in_string;
        }
        if character == '?' && !in_string {
            if let Some(parameter) = params.get(parameter_index) {
                rendered.push_str(&sql_literal(&parameter.value));
                parameter_index += 1;
                continue;
            }
        }
        rendered.push(character);
    }
    rendered
}

#[cfg(feature = "bif-datasource")]
fn sql_literal(value: &SqlValue) -> String {
    match value {
        SqlValue::Null => "NULL".to_string(),
        SqlValue::Bool(value) => value.to_string().to_ascii_uppercase(),
        SqlValue::Int(value) => value.to_string(),
        SqlValue::Float(value) => value.to_string(),
        SqlValue::Text(value) => format!("'{}'", value.replace('\'', "''")),
        SqlValue::Bytes(value) => format!("'{}'", String::from_utf8_lossy(value)),
    }
}

#[cfg(feature = "bif-datasource")]
fn query_result_to_struct(
    vm: &mut dyn BxVM,
    query: BxQuery,
    column_key: Option<&str>,
) -> Result<BxValue, String> {
    let column_key = column_key.ok_or_else(|| {
        "You must define a `columnKey` option when using `returnType: struct`.".to_string()
    })?;
    let key_index = query
        .columns
        .iter()
        .position(|column| column.name.eq_ignore_ascii_case(column_key))
        .ok_or_else(|| format!("Column '{}' not found", column_key))?;
    let outer_id = vm.struct_new();
    for row_idx in 0..query.record_count {
        let key_value = query
            .data
            .get(key_index)
            .and_then(|data| data.get(row_idx))
            .cloned()
            .unwrap_or(SqlValue::Null);
        let key_value = sql_to_bx(vm, &key_value);
        let key = vm.to_string(key_value);
        let array_id = match vm.struct_get(outer_id, &key).as_gc_id() {
            Some(array_id) if vm.is_array_value(BxValue::new_ptr(array_id)) => array_id,
            _ => {
                let array_id = vm.array_new();
                vm.struct_set(outer_id, &key, BxValue::new_ptr(array_id));
                array_id
            }
        };
        let row_id = vm.struct_new();
        for (col_idx, col) in query.columns.iter().enumerate() {
            let val = query
                .data
                .get(col_idx)
                .and_then(|data| data.get(row_idx))
                .cloned()
                .unwrap_or(SqlValue::Null);
            let bx = sql_to_bx(vm, &val);
            vm.struct_set(row_id, &col.name, bx);
        }
        vm.array_push(array_id, BxValue::new_ptr(row_id));
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
