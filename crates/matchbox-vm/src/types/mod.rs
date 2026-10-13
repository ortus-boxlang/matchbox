pub mod box_string;
pub mod value;

#[cfg(test)]
mod macros_test;

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use self::box_string::BoxString;

#[derive(Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxValue(u64);

impl BxValue {
    // ------------------------------------------------------------------------
    // Constants & Masks (NaN-Boxing)
    // ------------------------------------------------------------------------
    pub const TAGGED_BASE: u64 = 0xFFF0000000000000;
    pub const TAG_SHIFT: u64 = 48;
    pub const PAYLOAD_MASK: u64 = 0x0000FFFFFFFFFFFF;

    pub const TAG_INT: u64 = 0x8;
    pub const TAG_BOOL: u64 = 0x9;
    pub const TAG_NULL: u64 = 0xA;
    pub const TAG_PTR: u64 = 0xB;

    #[inline(always)]
    fn tag(tag: u64, payload: u64) -> u64 {
        Self::TAGGED_BASE | (tag << Self::TAG_SHIFT) | payload
    }

    // ------------------------------------------------------------------------
    // Constructors
    // ------------------------------------------------------------------------
    #[inline]
    pub fn to_bits(self) -> u64 {
        self.0
    }

    #[inline]
    pub fn new_number(f: f64) -> Self {
        let bits = f.to_bits();
        if bits >= 0xFFF8000000000000 {
            Self(0x7FF8000000000000)
        } else {
            Self(bits)
        }
    }

    #[inline]
    pub fn new_int(i: i32) -> Self {
        Self(Self::tag(Self::TAG_INT, (i as u32) as u64))
    }

    #[inline]
    pub fn new_bool(b: bool) -> Self {
        Self(Self::tag(Self::TAG_BOOL, b as u64))
    }

    #[inline]
    pub fn new_null() -> Self {
        Self(Self::tag(Self::TAG_NULL, 0))
    }

    #[inline]
    pub fn new_ptr(id: usize) -> Self {
        debug_assert!(id as u64 <= Self::PAYLOAD_MASK);
        Self(Self::tag(Self::TAG_PTR, id as u64))
    }

    // ------------------------------------------------------------------------
    // Predicates
    // ------------------------------------------------------------------------
    #[inline]
    pub fn is_float(&self) -> bool {
        self.0 < 0xFFF8000000000000
    }
    #[inline]
    pub fn is_number(&self) -> bool {
        self.is_float() || self.is_int()
    }
    #[inline]
    pub fn is_int(&self) -> bool {
        (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_INT, 0)
    }
    #[inline]
    pub fn is_bool(&self) -> bool {
        (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_BOOL, 0)
    }
    #[inline]
    pub fn is_null(&self) -> bool {
        (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_NULL, 0)
    }
    #[inline]
    pub fn is_ptr(&self) -> bool {
        (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_PTR, 0)
    }

    // ------------------------------------------------------------------------
    // Extractors
    // ------------------------------------------------------------------------
    #[inline]
    pub fn as_number(&self) -> f64 {
        if self.is_float() {
            f64::from_bits(self.0)
        } else if self.is_int() {
            self.as_int() as f64
        } else {
            f64::NAN
        }
    }
    #[inline]
    pub fn as_int(&self) -> i32 {
        self.0 as i32
    }
    #[inline]
    pub fn as_bool(&self) -> bool {
        (self.0 & Self::PAYLOAD_MASK) != 0
    }
    #[inline]
    pub fn as_gc_id(&self) -> Option<usize> {
        if self.is_ptr() {
            Some((self.0 & Self::PAYLOAD_MASK) as usize)
        } else {
            None
        }
    }
}

// ------------------------------------------------------------------------
// Interfaces
// ------------------------------------------------------------------------

pub trait BxVM {
    fn current_chunk(&self) -> Option<Rc<RefCell<crate::vm::chunk::Chunk>>>;
    fn current_receiver(&self) -> Option<BxValue>;
    fn current_function_called_name(&self) -> String {
        String::new()
    }
    fn interpret_chunk(&mut self, chunk: crate::vm::chunk::Chunk) -> Result<BxValue, String>;
    fn spawn(
        &mut self,
        func: Rc<BxCompiledFunction>,
        args: Vec<BxValue>,
        priority: u8,
        chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    ) -> BxValue;
    fn spawn_by_value(
        &mut self,
        func: &BxValue,
        args: Vec<BxValue>,
        priority: u8,
        chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    ) -> Result<BxValue, String>;
    fn call_function_by_value(
        &mut self,
        func: &BxValue,
        args: Vec<BxValue>,
        chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    ) -> Result<BxValue, String>;
    fn call_method_by_value(
        &mut self,
        receiver: BxValue,
        name: &str,
        args: Vec<BxValue>,
        _chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    ) -> Result<BxValue, String> {
        let _ = (receiver, name, args);
        Err("Value does not support method invocation".to_string())
    }
    fn yield_fiber(&mut self);
    fn sleep(&mut self, ms: u64);
    fn get_root_shape(&self) -> u32;
    fn get_shape_index(&self, shape_id: u32, field_name: &str) -> Option<u32>;
    fn get_len(&self, id: usize) -> usize;
    fn is_array_value(&self, val: BxValue) -> bool;
    fn is_struct_value(&self, val: BxValue) -> bool;
    fn is_string_value(&self, val: BxValue) -> bool;
    fn is_bytes(&self, val: BxValue) -> bool;
    fn type_name_from_value(&self, val: BxValue) -> Option<String>;
    fn find_global_class_by_name(&self, type_name: &str) -> Option<Rc<RefCell<BxClass>>>;
    fn find_global_interface_by_name(&self, type_name: &str) -> Option<Rc<RefCell<BxInterface>>>;
    fn class_matches_type_name(&self, class: &Rc<RefCell<BxClass>>, type_name: &str) -> bool;
    fn value_matches_type_name(&self, val: BxValue, type_name: &str) -> bool;
    fn cast_value_to_type(&mut self, val: BxValue, type_name: &str) -> Result<BxValue, String>;
    fn bytes_new(&mut self, data: Vec<u8>) -> usize;
    fn bytes_len(&self, id: usize) -> usize;
    fn bytes_get(&self, id: usize, idx: usize) -> Result<u8, String>;
    fn bytes_set(&mut self, id: usize, idx: usize, value: u8) -> Result<(), String>;
    fn to_bytes(&self, val: BxValue) -> Result<Vec<u8>, String>;
    fn array_len(&self, id: usize) -> usize;
    fn array_push(&mut self, id: usize, val: BxValue);
    fn array_pop(&mut self, id: usize) -> Result<BxValue, String>;
    fn array_get(&self, id: usize, idx: usize) -> BxValue;
    fn array_set(&mut self, id: usize, idx: usize, val: BxValue) -> Result<(), String>;
    fn array_delete_at(&mut self, id: usize, idx: usize) -> Result<BxValue, String>;
    fn array_insert_at(&mut self, id: usize, idx: usize, val: BxValue) -> Result<(), String>;
    fn array_clear(&mut self, id: usize) -> Result<(), String>;
    fn array_new(&mut self) -> usize;
    fn range_values(&self, _id: usize) -> Option<Vec<BxValue>> {
        None
    }
    fn struct_len(&self, id: usize) -> usize;
    fn struct_new(&mut self) -> usize;
    fn struct_new_with_options(&mut self, _options: &[String]) -> usize {
        self.struct_new()
    }
    fn struct_set(&mut self, id: usize, key: &str, val: BxValue);
    fn struct_get(&self, id: usize, key: &str) -> BxValue;
    fn struct_delete(&mut self, id: usize, key: &str) -> bool;
    fn struct_key_exists(&self, id: usize, key: &str) -> bool;
    fn struct_key_array(&self, id: usize) -> Vec<String>;
    fn struct_clear(&mut self, id: usize);
    fn struct_get_shape(&self, id: usize) -> u32;
    fn struct_is_case_sensitive(&self, _id: usize) -> bool {
        false
    }
    fn struct_is_ordered(&self, _id: usize) -> bool {
        false
    }
    fn future_new(&mut self) -> BxValue;
    fn future_resolve(&mut self, future: BxValue, value: BxValue) -> Result<(), String>;
    fn future_reject(&mut self, future: BxValue, error: BxValue) -> Result<(), String>;
    fn future_schedule_resolve(&mut self, future: BxValue, value: BxValue) -> Result<(), String>;
    fn future_schedule_reject(&mut self, future: BxValue, error: BxValue) -> Result<(), String>;
    fn native_future_new(&mut self) -> NativeFutureHandle;
    fn future_on_error(&mut self, id: usize, handler: BxValue);
    fn future_on_success(&mut self, _id: usize, _handler: BxValue) {}
    fn future_status(&self, _future: BxValue) -> Option<FutureStatus> {
        None
    }
    fn future_value(&self, _future: BxValue) -> Option<BxValue> {
        None
    }
    fn future_wait(&mut self, _future: BxValue) -> Result<BxValue, String> {
        Err("Future waiting is not supported".to_string())
    }
    fn mark_future_as_thread(&mut self, _future: BxValue) {}
    fn is_in_thread(&self) -> bool {
        false
    }
    fn native_object_new(&mut self, obj: Rc<RefCell<dyn BxNativeObject>>) -> usize;
    fn native_object_call_method(
        &mut self,
        id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String>;
    fn construct_native_class(
        &mut self,
        class_name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String>;
    fn instance_class_name(&self, receiver: BxValue) -> Result<String, String>;
    fn instance_variables_json(&self, receiver: BxValue) -> Result<serde_json::Value, String>;
    fn datetime_new(&mut self, dt: chrono::DateTime<chrono::Utc>) -> usize;
    fn datetime_new_with_timezone(
        &mut self,
        dt: chrono::DateTime<chrono::Utc>,
        _timezone: &str,
    ) -> usize {
        self.datetime_new(dt)
    }
    fn datetime_timezone(&self, _value: BxValue) -> Option<String> {
        None
    }
    fn string_new(&mut self, s: String) -> usize;
    fn to_string(&self, val: BxValue) -> String;
    fn to_box_string(&self, val: BxValue) -> BoxString;
    fn insert_global(&mut self, name: String, val: BxValue);
    fn get_global_value(&self, _name: &str) -> Option<BxValue> {
        None
    }
    fn soap_client_get(&self, _url: &str) -> Option<BxValue> {
        None
    }
    fn soap_client_set(&mut self, _url: String, _client: BxValue) {}
    #[cfg(not(target_arch = "wasm32"))]
    fn resolve_query_source_path(&self, path: &[String]) -> Option<BxValue>;
    #[cfg(not(target_arch = "wasm32"))]
    fn native_object_query_result(
        &self,
        id: usize,
    ) -> Option<crate::datasource::traits::QueryResult>;
    #[cfg(not(target_arch = "wasm32"))]
    fn native_object_query_columns(
        &self,
        id: usize,
    ) -> Option<Vec<crate::datasource::traits::QueryColumn>>;
    #[cfg(not(target_arch = "wasm32"))]
    fn native_object_query_row_count(&self, id: usize) -> Option<usize>;
    #[cfg(not(target_arch = "wasm32"))]
    fn native_object_query_cell(
        &self,
        id: usize,
        row_idx: usize,
        col_idx: usize,
    ) -> Option<crate::datasource::traits::SqlValue>;
    fn resolve_variable_path(&self, _path: &str) -> Option<BxValue> {
        None
    }
    fn get_cli_args(&self) -> Vec<String>;
    fn write_output(&mut self, s: &str);
    fn begin_output_capture(&mut self);
    fn end_output_capture(&mut self) -> Option<String>;
    fn suspend_gc(&mut self);
    fn resume_gc(&mut self);
    fn push_root(&mut self, val: BxValue);
    fn pop_root(&mut self);

    fn get_interner(&mut self) -> &mut crate::vm::intern::StringInterner;
    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    fn js_to_bx_wasm(&mut self, val: wasm_bindgen::JsValue) -> BxValue;
}

pub type BxNativeFunction = fn(&mut dyn BxVM, &[BxValue]) -> Result<BxValue, String>;

#[derive(Debug, Clone)]
pub enum NativeFutureValue {
    Null,
    Bool(bool),
    Int(i32),
    Number(f64),
    String(String),
    Bytes(Vec<u8>),
    Error { message: String },
}

pub enum NativeFutureMessage {
    Resolve {
        future: BxValue,
        value: NativeFutureValue,
    },
    Reject {
        future: BxValue,
        error: NativeFutureValue,
    },
    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    ResolveWasmThunk {
        future: BxValue,
        thunk_id: u64,
    },
    Abandon {
        future: BxValue,
    },
}

pub struct NativeFutureHandle {
    future: BxValue,
    sender: Sender<NativeFutureMessage>,
    completed: bool,
}

impl NativeFutureHandle {
    pub fn new(future: BxValue, sender: Sender<NativeFutureMessage>) -> Self {
        Self {
            future,
            sender,
            completed: false,
        }
    }

    pub fn future(&self) -> BxValue {
        self.future
    }

    pub fn resolve(mut self, value: NativeFutureValue) -> Result<(), String> {
        self.completed = true;
        self.sender
            .send(NativeFutureMessage::Resolve {
                future: self.future,
                value,
            })
            .map_err(|_| "VM completion channel is closed".to_string())
    }

    pub fn reject(mut self, error: NativeFutureValue) -> Result<(), String> {
        self.completed = true;
        self.sender
            .send(NativeFutureMessage::Reject {
                future: self.future,
                error,
            })
            .map_err(|_| "VM completion channel is closed".to_string())
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    pub fn resolve_wasm_thunk(mut self, thunk_id: u64) -> Result<(), String> {
        self.completed = true;
        self.sender
            .send(NativeFutureMessage::ResolveWasmThunk {
                future: self.future,
                thunk_id,
            })
            .map_err(|_| "VM completion channel is closed".to_string())
    }
}

impl Drop for NativeFutureHandle {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.sender.send(NativeFutureMessage::Abandon {
                future: self.future,
            });
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
pub type WasmFutureThunk = Box<dyn FnOnce(&mut dyn BxVM) -> Result<BxValue, String>>;

#[cfg(all(target_arch = "wasm32", feature = "js"))]
thread_local! {
    static WASM_FUTURE_THUNKS: RefCell<HashMap<u64, WasmFutureThunk>> = RefCell::new(HashMap::new());
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
static NEXT_WASM_FUTURE_THUNK_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(all(target_arch = "wasm32", feature = "js"))]
pub fn register_wasm_future_thunk(thunk: WasmFutureThunk) -> u64 {
    let id = NEXT_WASM_FUTURE_THUNK_ID.fetch_add(1, Ordering::Relaxed);
    WASM_FUTURE_THUNKS.with(|registry| {
        registry.borrow_mut().insert(id, thunk);
    });
    id
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
pub(crate) fn take_wasm_future_thunk(id: u64) -> Option<WasmFutureThunk> {
    WASM_FUTURE_THUNKS.with(|registry| registry.borrow_mut().remove(&id))
}

pub trait BxNativeObject: fmt::Debug {
    fn get_property(&self, name: &str) -> BxValue;
    fn set_property(&mut self, name: &str, value: BxValue);
    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String>;
    #[cfg(not(target_arch = "wasm32"))]
    fn query_result(&self) -> Option<crate::datasource::traits::QueryResult> {
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn query_columns(&self) -> Option<Vec<crate::datasource::traits::QueryColumn>> {
        self.query_result().map(|result| result.columns)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn query_row_count(&self) -> Option<usize> {
        self.query_result().map(|result| result.rows.len())
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn query_cell(
        &self,
        row_idx: usize,
        col_idx: usize,
    ) -> Option<crate::datasource::traits::SqlValue> {
        self.query_result().and_then(|result| {
            result
                .rows
                .get(row_idx)
                .and_then(|row| row.get(col_idx))
                .cloned()
        })
    }
    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

pub trait Tracer {
    fn mark(&mut self, val: &BxValue);
}

impl PartialEq for dyn BxNativeObject {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

// Display will need the Heap context to display pointers
impl fmt::Display for BxValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_number() {
            write!(f, "{}", self.as_number())
        } else if self.is_int() {
            write!(f, "{}", self.as_int())
        } else if self.is_bool() {
            write!(f, "{}", self.as_bool())
        } else if self.is_null() {
            write!(f, "null")
        } else if self.is_ptr() {
            write!(f, "<ptr {}>", self.as_gc_id().unwrap())
        } else {
            write!(f, "<invalid value 0x{:X}>", self.0)
        }
    }
}

impl fmt::Debug for BxValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    Number(f64),
    String(BoxString),
    Boolean(bool),
    Null,
    CompiledFunction(BxCompiledFunction),
    Class(BxClass),
    Interface(BxInterface),
    StringArray(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FunctionModifiers {
    pub access: Option<String>,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FunctionKind {
    #[default]
    Udf,
    Closure,
    Lambda,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ClassModifiers {
    pub is_abstract: bool,
    pub is_final: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxCompiledFunction {
    pub name: String,
    pub kind: FunctionKind,
    pub arity: u32,          // Total parameters
    pub min_arity: u32,      // Required parameters
    pub params: Vec<String>, // Parameter names
    pub modifiers: FunctionModifiers,
    /// Captured `this` for closures created inside class contexts.
    #[serde(skip)]
    pub captured_receiver: Option<BxValue>,
    /// The actual bytecode for this function.
    pub chunk: crate::vm::chunk::Chunk,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxClass {
    pub name: String,
    pub modifiers: ClassModifiers,
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub constructor: BxCompiledFunction,
    pub methods: Vec<(String, BxCompiledFunction)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxInterface {
    pub name: String,
    pub methods: Vec<(String, Option<BxCompiledFunction>)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxRange {
    pub start: i64,
    pub end: i64,
    pub from_exclusive: bool,
    pub to_exclusive: bool,
}

impl BxRange {
    pub fn from_bounds(start: i64, end: i64, from_exclusive: bool, to_exclusive: bool) -> Self {
        Self {
            start,
            end,
            from_exclusive,
            to_exclusive,
        }
    }

    pub fn iter_bounds(&self) -> (i64, i64) {
        if self.start <= self.end {
            let start = self.start + i64::from(self.from_exclusive);
            let end = self.end - i64::from(self.to_exclusive);
            (start, end)
        } else {
            let start = self.start - i64::from(self.from_exclusive);
            let end = self.end + i64::from(self.to_exclusive);
            (start, end)
        }
    }

    pub fn len(&self) -> usize {
        let (start, end) = self.iter_bounds();
        if self.start <= self.end {
            if start > end {
                0
            } else {
                (end - start + 1) as usize
            }
        } else if start < end {
            0
        } else {
            (start - end + 1) as usize
        }
    }

    pub fn contains_number(&self, value: f64) -> bool {
        if value.fract() != 0.0 {
            return false;
        }
        let value = value as i64;
        let (start, end) = self.iter_bounds();
        if self.start <= self.end {
            value >= start && value <= end
        } else {
            value <= start && value >= end
        }
    }
}

impl std::fmt::Display for BxRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.from_exclusive && self.to_exclusive {
            write!(f, "{}.<.{}", self.start, self.end)
        } else if self.from_exclusive {
            write!(f, "{}.<{}", self.start, self.end)
        } else if self.to_exclusive {
            write!(f, "{}..<{}", self.start, self.end)
        } else {
            write!(f, "{}..{}", self.start, self.end)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxStruct {
    pub shape_id: u32,
    pub properties: Vec<BxValue>,
    pub case_sensitive: bool,
    pub ordered: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxInstance {
    pub class: Rc<RefCell<BxClass>>,
    pub shape_id: u32,
    pub properties: Vec<BxValue>,
    pub variables: Rc<RefCell<HashMap<String, BxValue>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxFuture {
    pub value: BxValue,
    pub status: FutureStatus,
    #[serde(skip)]
    pub error_handler: Option<BxValue>,
    #[serde(skip)]
    pub success_handler: Option<BxValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FutureStatus {
    Pending,
    Completed,
    Failed(BxValue),
}
