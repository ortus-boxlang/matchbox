use r2d2::Pool;
use r2d2_postgres::{postgres::NoTls, PostgresConnectionManager};
use postgres::types::ToSql;

use crate::datasource::traits::{
    DbDriver, DatasourceConfig, QueryColumn, QueryColumnType, QueryParam, QueryResult, SqlValue,
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
            conn_str.parse().map_err(|e| format!("Invalid connection string: {}", e))?,
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
        let mut conn = self.pool.get().map_err(|e| format!("Failed to get connection: {}", e))?;

        // Convert JDBC-style ? placeholders to PostgreSQL $1, $2, ...
        let converted_sql = convert_placeholders(sql);

        // Build boxed ToSql params
        let pg_params: Vec<Box<dyn ToSql + Sync>> = params
            .iter()
            .map(|p| sql_value_to_pg(&p.value))
            .collect();
        let pg_refs: Vec<&(dyn ToSql + Sync)> = pg_params.iter().map(|p| p.as_ref()).collect();

        let rows = conn
            .query(converted_sql.as_str(), pg_refs.as_slice())
            .map_err(|e| format!("Query failed: {}", e))?;

        if rows.is_empty() {
            // Could be a non-SELECT statement; return empty result
            return Ok(QueryResult { columns: vec![], rows: vec![] });
        }

        let columns: Vec<QueryColumn> = rows[0]
            .columns()
            .iter()
            .map(|col| QueryColumn {
                name: col.name().to_string(),
                col_type: pg_type_to_col_type(col.type_()),
            })
            .collect();

        let result_rows: Vec<Vec<SqlValue>> = rows
            .iter()
            .map(|row| {
                (0..columns.len())
                    .map(|i| extract_value(row, i))
                    .collect()
            })
            .collect();

        Ok(QueryResult { columns, rows: result_rows })
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

fn sql_value_to_pg(v: &SqlValue) -> Box<dyn ToSql + Sync> {
    match v {
        SqlValue::Null => Box::new(Option::<String>::None),
        SqlValue::Bool(b) => Box::new(*b),
        SqlValue::Int(i) => Box::new(*i),
        SqlValue::Float(f) => Box::new(*f),
        SqlValue::Text(s) => Box::new(s.clone()),
        SqlValue::Bytes(b) => Box::new(b.clone()),
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
