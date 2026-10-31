pub mod drivers;
pub mod registry;
pub mod traits;

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::types::{BxNativeObject, BxVM, BxValue};
use traits::{QueryColumn, QueryColumnType, SqlValue};

/// A BoxLang query result object, stored column-major: `data[col_idx][row_idx]`.
pub struct BxQuery {
    pub columns: Vec<QueryColumn>,
    /// Column-major storage: data[col_idx][row_idx]
    pub data: Vec<Vec<SqlValue>>,
    pub record_count: usize,
    pub current_row: usize,
}

impl BxQuery {
    pub fn new(columns: Vec<QueryColumn>) -> Self {
        let num_cols = columns.len();
        BxQuery {
            columns,
            data: vec![Vec::new(); num_cols],
            record_count: 0,
            current_row: 0,
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
        BxQuery {
            columns: result.columns,
            data,
            record_count,
            current_row: 0,
        }
    }

    fn col_index(&self, name: &str) -> Option<usize> {
        let lower = name.to_lowercase();
        self.columns
            .iter()
            .position(|c| c.name.to_lowercase() == lower)
    }
}

impl fmt::Debug for BxQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "<query recordCount={} columns={}>",
            self.record_count,
            self.columns.len()
        )
    }
}

impl BxNativeObject for BxQuery {
    fn get_property(&self, name: &str) -> BxValue {
        match name.to_lowercase().as_str() {
            "recordcount" => BxValue::new_number(self.record_count as f64),
            "columncount" => BxValue::new_number(self.columns.len() as f64),
            "currentrow" => BxValue::new_number(self.current_row as f64),
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
            "len" | "length" | "size" => Ok(BxValue::new_number(self.record_count as f64)),
            "toset" => {
                let set_id = vm.array_new();
                for row_idx in 0..self.record_count {
                    let row_id = vm.struct_new();
                    for (col_idx, column) in self.columns.iter().enumerate() {
                        let value = self
                            .data
                            .get(col_idx)
                            .and_then(|column_data| column_data.get(row_idx))
                            .map(|value| sql_to_bx(vm, value))
                            .unwrap_or_else(BxValue::new_null);
                        vm.struct_set(row_id, &column.name, value);
                    }
                    vm.array_push(set_id, BxValue::new_ptr(row_id));
                }
                Ok(BxValue::new_ptr(set_id))
            }
            "columnlist" | "getcolumnlist" => {
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
                    return Err(format!(
                        "Row {} out of range (1..{})",
                        row_num, self.record_count
                    ));
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
            "getrowasstruct" => {
                if args.is_empty() {
                    return Err(
                        "getRowAsStruct() requires a row number argument (0-based)".to_string()
                    );
                }
                let row_idx = args[0].as_number() as usize;
                if row_idx >= self.record_count {
                    return Err(format!(
                        "Row {} out of range (0..{})",
                        row_idx,
                        self.record_count.saturating_sub(1)
                    ));
                }
                let struct_id = vm.struct_new();
                for (col_idx, col) in self.columns.iter().enumerate() {
                    let value = self
                        .data
                        .get(col_idx)
                        .and_then(|column| column.get(row_idx))
                        .cloned()
                        .unwrap_or(SqlValue::Null);
                    let bx_value = sql_to_bx(vm, &value);
                    vm.struct_set(struct_id, &col.name, bx_value);
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
            "clear" => {
                for col_data in &mut self.data {
                    col_data.clear();
                }
                self.record_count = 0;
                Ok(BxValue::new_ptr(_id)) // return the query itself
            }
            "columnarray" | "getcolumnnames" => {
                let arr_id = vm.array_new();
                for col in &self.columns {
                    let s_id = vm.string_new(col.name.clone());
                    vm.array_push(arr_id, BxValue::new_ptr(s_id));
                }
                Ok(BxValue::new_ptr(arr_id))
            }
            "toarrayofstructs" => {
                let arr_id = vm.array_new();
                for row_idx in 0..self.record_count {
                    let struct_id = vm.struct_new();
                    for (col_idx, col) in self.columns.iter().enumerate() {
                        let value = self
                            .data
                            .get(col_idx)
                            .and_then(|column| column.get(row_idx))
                            .cloned()
                            .unwrap_or(SqlValue::Null);
                        let bx_value = sql_to_bx(vm, &value);
                        vm.struct_set(struct_id, &col.name, bx_value);
                    }
                    vm.array_push(arr_id, BxValue::new_ptr(struct_id));
                }
                Ok(BxValue::new_ptr(arr_id))
            }
            "getcolumnmeta" => {
                let meta_id = vm.struct_new();
                for col in &self.columns {
                    let info_id = vm.struct_new();
                    let type_name = vm.string_new(col_type_name(&col.col_type).to_string());
                    vm.struct_set(info_id, "type", BxValue::new_ptr(type_name));
                    vm.struct_set(info_id, "nullable", BxValue::new_bool(true));
                    vm.struct_set(info_id, "readOnly", BxValue::new_bool(false));
                    vm.struct_set(info_id, "decimals", BxValue::new_number(0.0));
                    vm.struct_set(info_id, "maxLength", BxValue::new_number(0.0));
                    vm.struct_set(meta_id, &col.name, BxValue::new_ptr(info_id));
                }
                Ok(BxValue::new_ptr(meta_id))
            }
            "columncount" => Ok(BxValue::new_number(self.columns.len() as f64)),
            "columnexists" => {
                if args.is_empty() {
                    return Err("columnExists() requires a column name argument".to_string());
                }
                let col_name = vm.to_string(args[0]);
                Ok(BxValue::new_bool(self.col_index(&col_name).is_some()))
            }
            "keyexists" => {
                if args.is_empty() {
                    return Err("keyExists() requires a column name argument".to_string());
                }
                let col_name = vm.to_string(args[0]);
                Ok(BxValue::new_bool(self.col_index(&col_name).is_some()))
            }
            "recordcount" => Ok(BxValue::new_number(self.record_count as f64)),
            "currentrow" => Ok(BxValue::new_number(self.current_row as f64)),
            "reverse" => {
                for col_data in &mut self.data {
                    col_data.reverse();
                }
                Ok(BxValue::new_ptr(_id))
            }
            "rowswap" => {
                if args.len() < 2 {
                    return Err("rowSwap() requires 2 arguments: (sourceRow, destRow)".to_string());
                }
                let src = args[0].as_number() as usize;
                let dst = args[1].as_number() as usize;
                if src == 0 || src > self.record_count || dst == 0 || dst > self.record_count {
                    return Err(format!(
                        "rowSwap() rows out of range (1..{})",
                        self.record_count
                    ));
                }
                let src_idx = src - 1;
                let dst_idx = dst - 1;
                for col_data in &mut self.data {
                    col_data.swap(src_idx, dst_idx);
                }
                Ok(BxValue::new_ptr(_id))
            }
            "slice" => {
                if args.is_empty() {
                    return Err(
                        "slice() requires at least 1 argument: (offset [, length])".to_string()
                    );
                }
                let offset = args[0].as_number() as isize;
                let mut start_idx = offset - 1;
                if start_idx < 0 {
                    start_idx += self.record_count as isize;
                }
                if start_idx < 0 || start_idx >= self.record_count as isize {
                    return Err(format!(
                        "slice() offset out of range (1..{})",
                        self.record_count
                    ));
                }
                let length = if args.len() > 1 {
                    args[1].as_number() as isize
                } else {
                    0
                };
                if length < 0 || (length > 0 && start_idx + length > self.record_count as isize) {
                    return Err("slice() length is outside the query row range".to_string());
                }
                let end_idx = if length > 0 {
                    start_idx + length
                } else {
                    self.record_count as isize
                };

                let mut new_query = BxQuery {
                    columns: self.columns.clone(),
                    data: Vec::with_capacity(self.columns.len()),
                    record_count: 0,
                    current_row: 0,
                };
                for col_data in &self.data {
                    let sliced: Vec<SqlValue> =
                        col_data[start_idx as usize..end_idx as usize].to_vec();
                    new_query.data.push(sliced);
                }
                new_query.record_count = (end_idx - start_idx) as usize;

                let id = vm.native_object_new(Rc::new(RefCell::new(new_query)));
                Ok(BxValue::new_ptr(id))
            }
            "deletecolumn" => {
                if args.is_empty() {
                    return Err("deleteColumn() requires a column name argument".to_string());
                }
                let col_name = vm.to_string(args[0]);
                let col_idx = self
                    .col_index(&col_name)
                    .ok_or_else(|| format!("Column '{}' not found", col_name))?;
                self.columns.remove(col_idx);
                self.data.remove(col_idx);
                Ok(BxValue::new_ptr(_id))
            }
            "deleterow" => {
                if args.is_empty() {
                    return Err("deleteRow() requires a row number argument (1-based)".to_string());
                }
                let row_num = args[0].as_number() as usize;
                if row_num == 0 || row_num > self.record_count {
                    return Err(format!(
                        "deleteRow() row {} out of range (1..{})",
                        row_num, self.record_count
                    ));
                }
                let row_idx = row_num - 1;
                for col_data in &mut self.data {
                    col_data.remove(row_idx);
                }
                self.record_count -= 1;
                Ok(BxValue::new_ptr(_id))
            }
            "addcolumn" => {
                if args.len() < 2 {
                    return Err("addColumn() requires at least 2 arguments: (columnName, datatype [, arrayData])".to_string());
                }
                let col_name = vm.to_string(args[0]);
                let col_type_str = vm.to_string(args[1]);
                let col_type = parse_col_type(&col_type_str);
                self.columns.push(QueryColumn {
                    name: col_name.clone(),
                    col_type,
                });
                let default_data: Vec<SqlValue> = if args.len() > 2 && !args[2].is_null() {
                    if let Some(arr_id) = args[2].as_gc_id() {
                        let len = vm.array_len(arr_id);
                        (0..len)
                            .map(|i| bx_to_sql(vm, vm.array_get(arr_id, i)))
                            .collect()
                    } else {
                        vec![SqlValue::Null; self.record_count]
                    }
                } else {
                    vec![SqlValue::Null; self.record_count]
                };
                if default_data.len() > self.record_count {
                    for column in &mut self.data {
                        column.resize(default_data.len(), SqlValue::Null);
                    }
                    self.record_count = default_data.len();
                }
                self.data.push(default_data);
                Ok(BxValue::new_ptr(_id))
            }
            "append" => {
                if args.is_empty() {
                    return Err("append() requires a query argument".to_string());
                }
                let source_id = args[0]
                    .as_gc_id()
                    .ok_or_else(|| "append() argument must be a query object".to_string())?;
                let source_cols = vm
                    .native_object_query_columns(source_id)
                    .ok_or_else(|| "append() argument is not a query object".to_string())?;
                let source_row_count = vm
                    .native_object_query_row_count(source_id)
                    .ok_or_else(|| "append() argument is not a query object".to_string())?;
                for row_idx in 0..source_row_count {
                    for (col_idx, col) in self.columns.iter().enumerate() {
                        if let Some(src_col_idx) = source_cols
                            .iter()
                            .position(|c| c.name.to_lowercase() == col.name.to_lowercase())
                        {
                            let val = vm
                                .native_object_query_cell(source_id, row_idx, src_col_idx)
                                .unwrap_or(SqlValue::Null);
                            self.data[col_idx].push(val);
                        } else {
                            self.data[col_idx].push(SqlValue::Null);
                        }
                    }
                    self.record_count += 1;
                }
                Ok(BxValue::new_ptr(_id))
            }
            "prepend" => {
                if args.is_empty() {
                    return Err("prepend() requires a query argument".to_string());
                }
                let source_id = args[0]
                    .as_gc_id()
                    .ok_or_else(|| "prepend() argument must be a query object".to_string())?;
                let source_cols = vm
                    .native_object_query_columns(source_id)
                    .ok_or_else(|| "prepend() argument is not a query object".to_string())?;
                let source_row_count = vm
                    .native_object_query_row_count(source_id)
                    .ok_or_else(|| "prepend() argument is not a query object".to_string())?;
                for row_idx in (0..source_row_count).rev() {
                    for (col_idx, col) in self.columns.iter().enumerate() {
                        if let Some(src_col_idx) = source_cols
                            .iter()
                            .position(|c| c.name.to_lowercase() == col.name.to_lowercase())
                        {
                            let val = vm
                                .native_object_query_cell(source_id, row_idx, src_col_idx)
                                .unwrap_or(SqlValue::Null);
                            self.data[col_idx].insert(0, val);
                        } else {
                            self.data[col_idx].insert(0, SqlValue::Null);
                        }
                    }
                    self.record_count += 1;
                }
                Ok(BxValue::new_ptr(_id))
            }
            "getcell" => {
                if args.len() < 1 {
                    return Err(
                        "getCell() requires at least 1 argument: (columnName [, rowNumber])"
                            .to_string(),
                    );
                }
                let col_name = vm.to_string(args[0]);
                let col_idx = self
                    .col_index(&col_name)
                    .ok_or_else(|| format!("Column '{}' not found", col_name))?;
                let row_num = if args.len() > 1 && !args[1].is_null() {
                    args[1].as_number() as usize
                } else {
                    self.current_row
                };
                if row_num == 0 || row_num > self.record_count {
                    return Err(format!(
                        "getCell() row {} out of range (1..{})",
                        row_num, self.record_count
                    ));
                }
                let val = self.data[col_idx][row_num - 1].clone();
                Ok(sql_to_bx(vm, &val))
            }
            "setcell" => {
                if args.len() < 2 {
                    return Err("setCell() requires at least 2 arguments: (columnName, value [, rowNumber])".to_string());
                }
                let col_name = vm.to_string(args[0]);
                let col_idx = self
                    .col_index(&col_name)
                    .ok_or_else(|| format!("Column '{}' not found", col_name))?;
                let val = args[1];
                let sql_val = bx_to_sql(vm, val);
                let row_num = if args.len() > 2 && !args[2].is_null() {
                    args[2].as_number() as usize
                } else {
                    self.current_row
                };
                if row_num == 0 || row_num > self.record_count {
                    return Err(format!(
                        "setCell() row {} out of range (1..{})",
                        row_num, self.record_count
                    ));
                }
                self.data[col_idx][row_num - 1] = sql_val;
                Ok(BxValue::new_ptr(_id))
            }
            "insertat" => {
                if args.len() < 2 {
                    return Err(
                        "insertAt() requires at least 2 arguments: (sourceQuery, position)"
                            .to_string(),
                    );
                }
                let position = if args[0].is_number() {
                    args[0].as_number() as usize
                } else {
                    args[1].as_number() as usize
                };
                if position == 0 || position > self.record_count + 1 {
                    return Err(format!(
                        "insertAt() position {} out of range (1..{})",
                        position,
                        self.record_count + 1
                    ));
                }
                if args[0].is_number() {
                    let row_id = args[1]
                        .as_gc_id()
                        .filter(|_| vm.is_struct_value(args[1]))
                        .ok_or_else(|| "insertAt() row must be a struct".to_string())?;
                    let insert_idx = position - 1;
                    for (col_idx, col) in self.columns.iter().enumerate() {
                        let value = bx_to_sql(vm, vm.struct_get(row_id, &col.name));
                        self.data[col_idx].insert(insert_idx, value);
                    }
                    self.record_count += 1;
                    return Ok(BxValue::new_ptr(_id));
                }
                let source_id = args[0].as_gc_id().ok_or_else(|| {
                    "insertAt() first argument must be a query object".to_string()
                })?;
                let source_cols = vm
                    .native_object_query_columns(source_id)
                    .ok_or_else(|| "insertAt() source is not a query object".to_string())?;
                let source_row_count = vm
                    .native_object_query_row_count(source_id)
                    .ok_or_else(|| "insertAt() source is not a query object".to_string())?;
                let insert_idx = position - 1;
                for row_idx in (0..source_row_count).rev() {
                    for (col_idx, col) in self.columns.iter().enumerate() {
                        if let Some(src_col_idx) = source_cols
                            .iter()
                            .position(|c| c.name.to_lowercase() == col.name.to_lowercase())
                        {
                            let val = vm
                                .native_object_query_cell(source_id, row_idx, src_col_idx)
                                .unwrap_or(SqlValue::Null);
                            self.data[col_idx].insert(insert_idx, val);
                        } else {
                            self.data[col_idx].insert(insert_idx, SqlValue::Null);
                        }
                    }
                    self.record_count += 1;
                }
                Ok(BxValue::new_ptr(_id))
            }
            "sortcolumn" => {
                if args.is_empty() {
                    return Err("sort() requires a column name".to_string());
                }
                let col_name = vm.to_string(args[0]);
                let col_idx = self
                    .col_index(&col_name)
                    .ok_or_else(|| format!("Column '{}' not found", col_name))?;
                let descending = args
                    .get(1)
                    .map(|value| vm.to_string(*value).eq_ignore_ascii_case("desc"))
                    .unwrap_or(false);
                let numeric = args
                    .get(2)
                    .map(|value| vm.to_string(*value).eq_ignore_ascii_case("numeric"))
                    .unwrap_or(false);
                let mut order: Vec<usize> = (0..self.record_count).collect();
                order.sort_by(|left, right| {
                    let lhs = self.data[col_idx].get(*left).unwrap_or(&SqlValue::Null);
                    let rhs = self.data[col_idx].get(*right).unwrap_or(&SqlValue::Null);
                    let ordering = if numeric {
                        sql_number(lhs)
                            .partial_cmp(&sql_number(rhs))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        sql_text(lhs).cmp(&sql_text(rhs))
                    };
                    if descending {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                });
                for column in &mut self.data {
                    let old = column.clone();
                    for (new_index, old_index) in order.iter().enumerate() {
                        column[new_index] = old[*old_index].clone();
                    }
                }
                Ok(BxValue::new_ptr(_id))
            }
            "setrow" => {
                if args.is_empty() {
                    return Err(
                        "setRow() requires at least 1 argument: (rowData [, rowNumber])"
                            .to_string(),
                    );
                }
                // Determine if first arg is rowNumber or rowData
                let (row_num, row_data) =
                    if args.len() >= 2 && args[0].is_number() && vm.is_struct_value(args[1]) {
                        (args[0].as_number() as usize, args[1])
                    } else if args.len() >= 2 && args[0].is_number() && !vm.is_struct_value(args[1])
                    {
                        // rowNumber, rowData (non-struct) — treat as append with specified position
                        (args[0].as_number() as usize, args[1])
                    } else if args.len() == 2 && vm.is_struct_value(args[0]) {
                        // rowData, rowNumber
                        (args[1].as_number() as usize, args[0])
                    } else if args.len() == 1 && vm.is_struct_value(args[0]) {
                        // Just rowData — append
                        (0, args[0])
                    } else if args.len() == 1 {
                        // Single scalar for single-column query
                        (0, args[0])
                    } else {
                        return Err("setRow() invalid arguments".to_string());
                    };

                if row_num == 0 {
                    // Append
                    if let Some(struct_id) =
                        row_data.as_gc_id().filter(|_| vm.is_struct_value(row_data))
                    {
                        for (col_idx, col) in self.columns.iter().enumerate() {
                            let bx_val = vm.struct_get(struct_id, &col.name);
                            self.data[col_idx].push(bx_to_sql(vm, bx_val));
                        }
                    } else if vm.is_array_value(row_data) {
                        let arr_id = row_data.as_gc_id().unwrap();
                        for col_idx in 0..self.columns.len() {
                            let bx_val = if col_idx < vm.array_len(arr_id) {
                                vm.array_get(arr_id, col_idx)
                            } else {
                                BxValue::new_null()
                            };
                            self.data[col_idx].push(bx_to_sql(vm, bx_val));
                        }
                    } else if self.columns.len() == 1 {
                        self.data[0].push(bx_to_sql(vm, row_data));
                    } else {
                        return Err("setRow() rowData must be a struct or array".to_string());
                    }
                    self.record_count += 1;
                } else if row_num > 0 && row_num <= self.record_count {
                    // Update existing row
                    let row_idx = row_num - 1;
                    if let Some(struct_id) =
                        row_data.as_gc_id().filter(|_| vm.is_struct_value(row_data))
                    {
                        for (col_idx, col) in self.columns.iter().enumerate() {
                            let bx_val = vm.struct_get(struct_id, &col.name);
                            self.data[col_idx][row_idx] = bx_to_sql(vm, bx_val);
                        }
                    } else {
                        return Err(
                            "setRow() rowData must be a struct for updating existing rows"
                                .to_string(),
                        );
                    }
                } else {
                    return Err(format!("setRow() row {} out of range", row_num));
                }
                Ok(BxValue::new_ptr(_id))
            }
            "getresult" => {
                let struct_id = vm.struct_new();
                vm.struct_set(
                    struct_id,
                    "recordCount",
                    BxValue::new_number(self.record_count as f64),
                );
                let col_list: String = self
                    .columns
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                let col_list_id = vm.string_new(col_list);
                vm.struct_set(struct_id, "columnList", BxValue::new_ptr(col_list_id));
                Ok(BxValue::new_ptr(struct_id))
            }
            _ => Err(format!("Unknown query method: {}", name)),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn query_result(&self) -> Option<traits::QueryResult> {
        let rows = (0..self.record_count)
            .map(|row_idx| {
                self.data
                    .iter()
                    .map(|col| col.get(row_idx).cloned().unwrap_or(SqlValue::Null))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        Some(traits::QueryResult {
            columns: self.columns.clone(),
            rows,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn query_columns(&self) -> Option<Vec<QueryColumn>> {
        Some(self.columns.clone())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn query_row_count(&self) -> Option<usize> {
        Some(self.record_count)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn query_cell(&self, row_idx: usize, col_idx: usize) -> Option<SqlValue> {
        if row_idx >= self.record_count || col_idx >= self.columns.len() {
            return None;
        }
        Some(
            self.data
                .get(col_idx)
                .and_then(|column| column.get(row_idx))
                .cloned()
                .unwrap_or(SqlValue::Null),
        )
    }
}

fn sql_number(value: &SqlValue) -> f64 {
    match value {
        SqlValue::Int(value) => *value as f64,
        SqlValue::Float(value) => *value,
        SqlValue::Bool(value) => *value as u8 as f64,
        SqlValue::Text(value) => value.parse::<f64>().unwrap_or(f64::NAN),
        SqlValue::Null | SqlValue::Bytes(_) => f64::NAN,
    }
}

fn sql_text(value: &SqlValue) -> String {
    match value {
        SqlValue::Null => String::new(),
        SqlValue::Bool(value) => value.to_string(),
        SqlValue::Int(value) => value.to_string(),
        SqlValue::Float(value) => value.to_string(),
        SqlValue::Text(value) => value.clone(),
        SqlValue::Bytes(value) => String::from_utf8_lossy(value).into_owned(),
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

pub fn parse_col_type(s: &str) -> QueryColumnType {
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
        "object" => QueryColumnType::Varchar, // default to varchar for "object" type
        other => QueryColumnType::Other(other.to_string()),
    }
}
