//This is the driver for the implementation of SQLite.

use crate::datasource::traits::{
    DatasourceConfig, DbDriver, QueryColumn, QueryColumnType, QueryParam, QueryResult, SqlValue,
};
use rusqlite::{
    Connection,
    types::{Value, ValueRef},
};
use std::sync::Mutex; //Keeps code consistant, acts like a lock. //Takes matchbox's types to be converted to SQL

pub struct SqliteDriver {
    conn: Mutex<Connection>, //Lock keeps two queries from collision.
}

//Build the driver: opens the db named in the config and creates a new db in RAM.
impl SqliteDriver {
    pub fn new(config: &DatasourceConfig) -> Result<Self, String> {
        let conn = Connection::open(&config.database).map_err(|e| {
            format!(
                "Failed to open SQLite database '{}': {}",
                config.database, e
            )
        })?; //Fail: return specific error to user, instead of exiting suddenly.
        Ok(SqliteDriver {
            conn: Mutex::new(conn),
        }) //Pass: return OK.
    }
}

fn sqlite_value_to_value(value: SqlValue) -> Value {
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Bool(value) => Value::Integer(if value { 1 } else { 0 }),
        SqlValue::Int(value) => Value::Integer(value),
        SqlValue::Float(value) => Value::Real(value),
        SqlValue::Text(value) => Value::Text(value),
        SqlValue::Bytes(value) => Value::Blob(value),
    }
}

fn sqlite_value_from_ref(value: ValueRef<'_>) -> Result<SqlValue, String> {
    match value {
        ValueRef::Null => Ok(SqlValue::Null),
        ValueRef::Integer(value) => Ok(SqlValue::Int(value)),
        ValueRef::Real(value) => Ok(SqlValue::Float(value)),
        ValueRef::Text(value) => Ok(SqlValue::Text(String::from_utf8_lossy(value).into_owned())),
        ValueRef::Blob(_) => {
            Err("SQLite query returned a BLOB/blob value, which is not supported".to_string())
        }
    }
}

fn sqlite_col_type(_decl_type: &str) -> QueryColumnType {
    QueryColumnType::Other("sqlite".to_string())
}

//We have to make the SQLite driver fit into the rulebook of the DbDriver already set.

impl DbDriver for SqliteDriver {
    fn name(&self) -> &str {
        "sqlite"
    }

    fn execute(&self, sql: &str, params: &[QueryParam]) -> Result<QueryResult, String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "SQLite connection lock failed".to_string())?;
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| format!("SQLite prepare failed: {}", e))?;

        let sqlite_params: Vec<Value> = params
            .iter()
            .map(|param| sqlite_value_to_value(param.value.clone()))
            .collect();

        let column_count = stmt.column_count();
        let columns: Vec<QueryColumn> = stmt
            .column_names()
            .into_iter()
            .map(|name| QueryColumn {
                name: name.to_string(),
                col_type: sqlite_col_type(""),
            })
            .collect();

        if column_count == 0 {
            let affected = if sqlite_params.is_empty() {
                stmt.execute([])
            } else {
                stmt.execute(rusqlite::params_from_iter(sqlite_params.iter()))
            }
            .map_err(|e| format!("SQLite execution failed: {}", e))?;
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: vec![vec![]; affected as usize],
            });
        }

        let mut rows = if sqlite_params.is_empty() {
            stmt.query([])
        } else {
            stmt.query(rusqlite::params_from_iter(sqlite_params.iter()))
        }
        .map_err(|e| format!("SQLite query failed: {}", e))?;

        let mut result_rows = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|e| format!("SQLite row fetch failed: {}", e))?
        {
            let mut values = Vec::with_capacity(column_count);
            for index in 0..column_count {
                let raw_value = row
                    .get_ref(index)
                    .map_err(|e| format!("SQLite column {} conversion failed: {}", index, e))?;
                values.push(sqlite_value_from_ref(raw_value)?);
            }
            result_rows.push(values);
        }

        Ok(QueryResult {
            columns,
            rows: result_rows,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datasource::traits::SqlValue;

    #[test]
    fn sqlite_select_returns_query_result() {
        let driver = SqliteDriver::new(&DatasourceConfig {
            driver: "sqlite".to_string(),
            host: String::new(),
            port: 0,
            database: ":memory:".to_string(),
            username: String::new(),
            password: String::new(),
            max_connections: 1,
        })
        .unwrap();

        let result = driver.execute("SELECT 1 AS val", &[]).unwrap();
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.columns[0].name, "val");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0], vec![SqlValue::Int(1)]);
    }

    #[test]
    fn sqlite_blob_result_is_an_error() {
        let driver = SqliteDriver::new(&DatasourceConfig {
            driver: "sqlite".to_string(),
            host: String::new(),
            port: 0,
            database: ":memory:".to_string(),
            username: String::new(),
            password: String::new(),
            max_connections: 1,
        })
        .unwrap();

        let err = driver
            .execute("SELECT CAST(x'0102' AS BLOB) AS val", &[])
            .unwrap_err();
        assert!(err.contains("BLOB") || err.contains("blob"));
    }
}
