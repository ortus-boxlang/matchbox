use postgres::types::{ToSql, Type};
use r2d2::Pool;
use r2d2_postgres::{PostgresConnectionManager, postgres::NoTls};

use crate::datasource::traits::{
    DatasourceConfig, DbDriver, QueryColumn, QueryColumnType, QueryParam, QueryResult, SqlValue,
};

pub struct PostgresDriver {
    pool: Pool<PostgresConnectionManager<NoTls>>,
}

impl PostgresDriver {
    pub fn new(config: &DatasourceConfig) -> Result<Self, String> {
        let conn_str = format!(
            "host={} port={} dbname={} user={} password={}",
            config.host, config.port, config.database, config.username, config.password
        );
        let manager = PostgresConnectionManager::new(
            conn_str
                .parse()
                .map_err(|e| format!("Invalid connection string: {}", e))?,
            NoTls,
        );
        let pool = Pool::builder()
            .max_size(config.max_connections)
            .build(manager)
            .map_err(|e| format!("Failed to create connection pool: {}", e))?;
        Ok(PostgresDriver { pool })
    }
}

impl DbDriver for PostgresDriver {
    fn name(&self) -> &str {
        "postgresql"
    }

    fn execute(&self, sql: &str, params: &[QueryParam]) -> Result<QueryResult, String> {
        let mut conn = self
            .pool
            .get()
            .map_err(|e| format!("Failed to get connection: {}", e))?;

        // Convert JDBC-style ? placeholders to PostgreSQL $1, $2, ...
        let converted_sql = convert_placeholders(sql);

        let statement = conn
            .prepare(converted_sql.as_str())
            .map_err(|e| format!("Query failed: {}", e))?;

        // Use PostgreSQL's inferred type for each placeholder. This matters for
        // reused named parameters appearing in columns with different types.
        let pg_params: Vec<Box<dyn ToSql + Sync>> = params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                let parameter_type = statement.params().get(index).unwrap_or(&Type::UNKNOWN);
                sql_value_to_pg(&param.value, parameter_type)
            })
            .collect();
        let pg_refs: Vec<&(dyn ToSql + Sync)> = pg_params.iter().map(|p| p.as_ref()).collect();

        if statement.columns().is_empty() {
            let affected = conn
                .execute(&statement, pg_refs.as_slice())
                .map_err(|e| format!("Query failed: {}", e))?;
            return Ok(QueryResult {
                columns: vec![],
                rows: vec![vec![]; affected as usize],
            });
        }

        let rows = conn
            .query(&statement, pg_refs.as_slice())
            .map_err(|e| format!("Query failed: {}", e))?;

        let columns: Vec<QueryColumn> = statement
            .columns()
            .iter()
            .map(|col| QueryColumn {
                name: col.name().to_string(),
                col_type: pg_type_to_col_type(col.type_()),
            })
            .collect();

        if rows.is_empty() {
            return Ok(QueryResult {
                columns,
                rows: vec![],
            });
        }

        let result_rows: Vec<Vec<SqlValue>> = rows
            .iter()
            .map(|row| (0..columns.len()).map(|i| extract_value(row, i)).collect())
            .collect();

        Ok(QueryResult {
            columns,
            rows: result_rows,
        })
    }
}

/// Replace JDBC-style `?` placeholders with PostgreSQL `$1`, `$2`, ...
fn convert_placeholders(sql: &str) -> String {
    let mut result = String::with_capacity(sql.len() + 8);
    let mut n = 1usize;
    let mut in_string = false;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_string => {
                in_string = true;
                result.push(c);
            }
            '\'' if in_string => {
                in_string = false;
                result.push(c);
            }
            '?' if !in_string => {
                result.push('$');
                result.push_str(&n.to_string());
                n += 1;
            }
            _ => result.push(c),
        }
    }
    result
}

fn sql_value_to_pg(v: &SqlValue, parameter_type: &Type) -> Box<dyn ToSql + Sync> {
    if matches!(v, SqlValue::Null) {
        return Box::new(Option::<String>::None);
    }
    match parameter_type {
        &Type::BOOL => Box::new(sql_value_as_bool(v)),
        &Type::INT2 => Box::new(sql_value_as_int(v) as i16),
        &Type::INT4 => Box::new(sql_value_as_int(v) as i32),
        &Type::INT8 => Box::new(sql_value_as_int(v)),
        &Type::FLOAT4 => Box::new(sql_value_as_float(v) as f32),
        &Type::FLOAT8 | &Type::NUMERIC => Box::new(sql_value_as_float(v)),
        &Type::VARCHAR | &Type::CHAR | &Type::BPCHAR | &Type::TEXT | &Type::NAME => {
            Box::new(sql_value_as_text(v))
        }
        &Type::BYTEA => match v {
            SqlValue::Bytes(bytes) => Box::new(bytes.clone()),
            _ => Box::new(sql_value_as_text(v)),
        },
        _ => match v {
            SqlValue::Null => Box::new(Option::<String>::None),
            SqlValue::Bool(value) => Box::new(*value),
            SqlValue::Int(value) => Box::new(*value as i32),
            SqlValue::Float(value) => Box::new(*value),
            SqlValue::Text(value) => Box::new(value.clone()),
            SqlValue::Bytes(value) => Box::new(value.clone()),
        },
    }
}

fn sql_value_as_text(value: &SqlValue) -> String {
    match value {
        SqlValue::Null => String::new(),
        SqlValue::Bool(value) => value.to_string(),
        SqlValue::Int(value) => value.to_string(),
        SqlValue::Float(value) => value.to_string(),
        SqlValue::Text(value) => value.clone(),
        SqlValue::Bytes(value) => String::from_utf8_lossy(value).into_owned(),
    }
}

fn sql_value_as_int(value: &SqlValue) -> i64 {
    match value {
        SqlValue::Int(value) => *value,
        SqlValue::Float(value) => *value as i64,
        SqlValue::Text(value) => value.parse::<i64>().unwrap_or_default(),
        SqlValue::Bool(value) => *value as i64,
        SqlValue::Null | SqlValue::Bytes(_) => 0,
    }
}

fn sql_value_as_float(value: &SqlValue) -> f64 {
    match value {
        SqlValue::Float(value) => *value,
        SqlValue::Int(value) => *value as f64,
        SqlValue::Text(value) => value.parse::<f64>().unwrap_or_default(),
        SqlValue::Bool(value) => *value as u8 as f64,
        SqlValue::Null | SqlValue::Bytes(_) => 0.0,
    }
}

fn sql_value_as_bool(value: &SqlValue) -> bool {
    match value {
        SqlValue::Bool(value) => *value,
        SqlValue::Int(value) => *value != 0,
        SqlValue::Float(value) => *value != 0.0,
        SqlValue::Text(value) => {
            matches!(value.to_ascii_lowercase().as_str(), "true" | "1" | "yes")
        }
        SqlValue::Null | SqlValue::Bytes(_) => false,
    }
}

fn pg_type_to_col_type(t: &postgres::types::Type) -> QueryColumnType {
    use postgres::types::Type;
    match t {
        &Type::BOOL => QueryColumnType::Boolean,
        &Type::INT2 | &Type::INT4 => QueryColumnType::Integer,
        &Type::INT8 => QueryColumnType::BigInt,
        &Type::FLOAT4 | &Type::FLOAT8 => QueryColumnType::Double,
        &Type::NUMERIC => QueryColumnType::Decimal,
        &Type::DATE => QueryColumnType::Date,
        &Type::TIMESTAMP | &Type::TIMESTAMPTZ => QueryColumnType::Timestamp,
        &Type::BYTEA => QueryColumnType::Blob,
        &Type::VARCHAR | &Type::TEXT | &Type::BPCHAR => QueryColumnType::Varchar,
        other => QueryColumnType::Other(other.name().to_string()),
    }
}

fn extract_value(row: &postgres::Row, i: usize) -> SqlValue {
    use postgres::types::Type;
    // Use try_get throughout so unsupported type deserialization (e.g. NUMERIC with
    // no rust_decimal dep) returns Null rather than panicking.
    let t = row.columns()[i].type_().clone();
    match t {
        Type::BOOL => row
            .try_get::<_, Option<bool>>(i)
            .unwrap_or(None)
            .map(SqlValue::Bool)
            .unwrap_or(SqlValue::Null),
        Type::INT2 => row
            .try_get::<_, Option<i16>>(i)
            .unwrap_or(None)
            .map(|v| SqlValue::Int(v as i64))
            .unwrap_or(SqlValue::Null),
        Type::INT4 => row
            .try_get::<_, Option<i32>>(i)
            .unwrap_or(None)
            .map(|v| SqlValue::Int(v as i64))
            .unwrap_or(SqlValue::Null),
        Type::INT8 => row
            .try_get::<_, Option<i64>>(i)
            .unwrap_or(None)
            .map(SqlValue::Int)
            .unwrap_or(SqlValue::Null),
        Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(i)
            .unwrap_or(None)
            .map(|v| SqlValue::Float(v as f64))
            .unwrap_or(SqlValue::Null),
        Type::FLOAT8 => row
            .try_get::<_, Option<f64>>(i)
            .unwrap_or(None)
            .map(SqlValue::Float)
            .unwrap_or(SqlValue::Null),
        Type::NUMERIC => {
            // NUMERIC requires the rust_decimal feature; without it we fall back
            // to a text cast sent by simple_query or return Null for binary-protocol
            // parameterized results.
            row.try_get::<_, Option<f64>>(i)
                .unwrap_or(None)
                .map(SqlValue::Float)
                .unwrap_or_else(|| {
                    row.try_get::<_, Option<String>>(i)
                        .unwrap_or(None)
                        .and_then(|s| s.parse::<f64>().ok().map(SqlValue::Float))
                        .unwrap_or(SqlValue::Null)
                })
        }
        Type::BYTEA => row
            .try_get::<_, Option<Vec<u8>>>(i)
            .unwrap_or(None)
            .map(SqlValue::Bytes)
            .unwrap_or(SqlValue::Null),
        _ => row
            .try_get::<_, Option<String>>(i)
            .unwrap_or(None)
            .map(SqlValue::Text)
            .unwrap_or(SqlValue::Null),
    }
}
