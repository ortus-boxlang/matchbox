pub mod traits;
pub mod registry;
pub mod drivers;

use std::fmt;

use crate::types::{BxNativeObject, BxVM, BxValue};
use traits::{QueryColumn, QueryColumnType, SqlValue};

/// A BoxLang query result object, stored column-major: `data[col_idx][row_idx]`.
pub struct BxQuery {
    pub columns: Vec<QueryColumn>,
    /// Column-major storage: data[col_idx][row_idx]
    pub data: Vec<Vec<SqlValue>>,
    pub record_count: usize,
}

impl BxQuery {
    pub fn new(columns: Vec<QueryColumn>) -> Self {
        let num_cols = columns.len();
        BxQuery {
            columns,
            data: vec![Vec::new(); num_cols],
            record_count: 0,
        }
    }

    pub fn from_result(result: traits::QueryResult) -> Self {
        let num_cols = result.columns.len();
        let mut data: Vec<Vec<SqlValue>> = vec![Vec::new(); num_cols];
        for row in &result.rows {
            for (col_idx, val) in row.iter().enumerate() {
                if col_idx < num_cols {
                    data[col_idx].push(val.clone());
                }
            }
        }
        let record_count = result.rows.len();
        BxQuery { columns: result.columns, data, record_count }
    }

    fn col_index(&self, name: &str) -> Option<usize> {
        let lower = name.to_lowercase();
        self.columns.iter().position(|c| c.name.to_lowercase() == lower)
    }
}

impl fmt::Debug for BxQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<query recordCount={} columns={}>", self.record_count, self.columns.len())
    }
}

impl BxNativeObject for BxQuery {
    fn get_property(&self, name: &str) -> BxValue {
        match name.to_lowercase().as_str() {
            "recordcount" => BxValue::new_number(self.record_count as f64),
            _ => BxValue::new_null(),
        }
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "columnlist" => {
                let list: String = self
                    .columns
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                Ok(BxValue::new_ptr(vm.string_new(list)))
            }
            "columndata" => {
                if args.is_empty() {
                    return Err("columnData() requires a column name argument".to_string());
                }
                let col_name = vm.to_string(args[0]);
                let col_idx = self
                    .col_index(&col_name)
                    .ok_or_else(|| format!("Column '{}' not found", col_name))?;
                let arr_id = vm.array_new();
                for val in &self.data[col_idx] {
                    let bx = sql_to_bx(vm, val);
                    vm.array_push(arr_id, bx);
                }
                Ok(BxValue::new_ptr(arr_id))
            }
            "getrow" => {
                if args.is_empty() {
                    return Err("getRow() requires a row number argument (1-based)".to_string());
                }
                let row_num = args[0].as_number() as usize;
                if row_num == 0 || row_num > self.record_count {
                    return Err(format!("Row {} out of range (1..{})", row_num, self.record_count));
                }
                let row_idx = row_num - 1;
                let struct_id = vm.struct_new();
                for (col_idx, col) in self.columns.iter().enumerate() {
                    let val = self
                        .data
                        .get(col_idx)
                        .and_then(|col_data| col_data.get(row_idx))
                        .cloned()
                        .unwrap_or(SqlValue::Null);
                    let bx = sql_to_bx(vm, &val);
                    vm.struct_set(struct_id, &col.name, bx);
                }
                Ok(BxValue::new_ptr(struct_id))
            }
            "addrow" => {
                // args[0]: struct of {colName: value, ...}
                if args.is_empty() {
                    return Err("addRow() requires a struct argument".to_string());
                }
                let struct_id = args[0]
                    .as_gc_id()
                    .ok_or_else(|| "addRow() argument must be a struct".to_string())?;
                for (col_idx, col) in self.columns.iter().enumerate() {
                    let bx_val = vm.struct_get(struct_id, &col.name);
                    let sql_val = bx_to_sql(vm, bx_val);
                    if col_idx < self.data.len() {
                        self.data[col_idx].push(sql_val);
                    }
                }
                self.record_count += 1;
                Ok(BxValue::new_bool(true))
            }
            _ => Err(format!("Unknown query method: {}", name)),
        }
    }
}

pub fn sql_to_bx(vm: &mut dyn BxVM, val: &SqlValue) -> BxValue {
    match val {
        SqlValue::Null => BxValue::new_null(),
        SqlValue::Bool(b) => BxValue::new_bool(*b),
        SqlValue::Int(i) => BxValue::new_number(*i as f64),
        SqlValue::Float(f) => BxValue::new_number(*f),
        SqlValue::Text(s) => BxValue::new_ptr(vm.string_new(s.clone())),
        SqlValue::Bytes(_) => BxValue::new_null(), // blobs not representable directly
    }
}

pub fn bx_to_sql(vm: &dyn BxVM, val: BxValue) -> SqlValue {
    if val.is_null() {
        SqlValue::Null
    } else if val.is_bool() {
        SqlValue::Bool(val.as_bool())
    } else if val.is_int() {
        SqlValue::Int(val.as_int() as i64)
    } else if val.is_number() {
        SqlValue::Float(val.as_number())
    } else {
        SqlValue::Text(vm.to_string(val))
    }
}

pub fn col_type_name(ct: &QueryColumnType) -> &'static str {
    match ct {
        QueryColumnType::Varchar => "varchar",
        QueryColumnType::Integer => "integer",
        QueryColumnType::BigInt => "bigint",
        QueryColumnType::Double => "double",
        QueryColumnType::Decimal => "decimal",
        QueryColumnType::Boolean => "boolean",
        QueryColumnType::Date => "date",
        QueryColumnType::Timestamp => "timestamp",
        QueryColumnType::Blob => "blob",
        QueryColumnType::Null => "null",
        QueryColumnType::Other(_) => "other",
    }
}
