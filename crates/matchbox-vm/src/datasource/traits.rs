#[derive(Debug, Clone)]
pub enum QueryColumnType {
    Varchar,
    Integer,
    BigInt,
    Double,
    Decimal,
    Boolean,
    Date,
    Timestamp,
    Blob,
    Null,
    Other(String),
}

#[derive(Debug, Clone)]
pub enum SqlValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct QueryParam {
    pub value: SqlValue,
    /// Optional CF-style SQL type hint: "CF_SQL_VARCHAR", "CF_SQL_INTEGER", etc.
    pub sql_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct QueryColumn {
    pub name: String,
    pub col_type: QueryColumnType,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<QueryColumn>,
    /// Row-major: rows[row_idx][col_idx]
    pub rows: Vec<Vec<SqlValue>>,
}

#[derive(Debug, Clone)]
pub struct DatasourceConfig {
    pub driver: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub max_connections: u32,
}

/// The driver trait — implement per DB engine.
pub trait DbDriver: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, sql: &str, params: &[QueryParam]) -> Result<QueryResult, String>;
}
