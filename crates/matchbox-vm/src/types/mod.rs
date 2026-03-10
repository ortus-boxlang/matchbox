pub mod box_string;
pub mod value;

use std::fmt;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use serde::{Serialize, Deserialize};

use self::box_string::BoxString;

#[derive(Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxValue(u64);

impl BxValue {
    // ------------------------------------------------------------------------
    // Constants & Masks (NaN-Boxing)
    // ------------------------------------------------------------------------
    const TAGGED_BASE: u64 = 0xFFF0000000000000;
    const TAG_SHIFT: u64 = 48;
    const PAYLOAD_MASK: u64 = 0x0000FFFFFFFFFFFF;
    
    const TAG_INT: u64  = 0x8;
    const TAG_BOOL: u64 = 0x9;
    const TAG_NULL: u64 = 0xA;
    const TAG_PTR: u64  = 0xB;

    #[inline(always)]
    fn tag(tag: u64, payload: u64) -> u64 {
        Self::TAGGED_BASE | (tag << Self::TAG_SHIFT) | payload
    }

    // ------------------------------------------------------------------------
    // Constructors
    // ------------------------------------------------------------------------
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
    #[inline] pub fn is_float(&self) -> bool { self.0 < 0xFFF8000000000000 }
    #[inline] pub fn is_number(&self) -> bool { self.is_float() || self.is_int() }
    #[inline] pub fn is_int(&self) -> bool { (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_INT, 0) }
    #[inline] pub fn is_bool(&self) -> bool { (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_BOOL, 0) }
    #[inline] pub fn is_null(&self) -> bool { (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_NULL, 0) }
    #[inline] pub fn is_ptr(&self) -> bool { (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_PTR, 0) }

    // ------------------------------------------------------------------------
    // Extractors
    // ------------------------------------------------------------------------
    #[inline] pub fn as_number(&self) -> f64 { 
        if self.is_float() {
            f64::from_bits(self.0)
        } else if self.is_int() {
            self.as_int() as f64
        } else {
            f64::NAN
        }
    }
    #[inline] pub fn as_int(&self) -> i32 { self.0 as i32 }
    #[inline] pub fn as_bool(&self) -> bool { (self.0 & Self::PAYLOAD_MASK) != 0 }
    #[inline] pub fn as_gc_id(&self) -> Option<usize> {
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
    fn spawn(&mut self, func: Rc<BxCompiledFunction>, args: Vec<BxValue>, priority: u8) -> BxValue;
    fn spawn_by_value(&mut self, func: &BxValue, args: Vec<BxValue>, priority: u8) -> Result<BxValue, String>;
    fn call_function_by_value(&mut self, func: &BxValue, args: Vec<BxValue>) -> Result<BxValue, String>;
    fn yield_fiber(&mut self);
    fn sleep(&mut self, ms: u64);
    fn get_root_shape(&self) -> u32;
    fn get_shape_index(&self, shape_id: u32, field_name: &str) -> Option<u32>;
    fn get_len(&self, id: usize) -> usize;
    fn array_len(&self, id: usize) -> usize;
    fn array_push(&mut self, id: usize, val: BxValue);
    fn array_pop(&mut self, id: usize) -> Result<BxValue, String>;
    fn array_get(&self, id: usize, idx: usize) -> BxValue;
    fn array_set(&mut self, id: usize, idx: usize, val: BxValue) -> Result<(), String>;
    fn array_delete_at(&mut self, id: usize, idx: usize) -> Result<BxValue, String>;
    fn array_insert_at(&mut self, id: usize, idx: usize, val: BxValue) -> Result<(), String>;
    fn array_clear(&mut self, id: usize) -> Result<(), String>;
    fn array_new(&mut self) -> usize;
    fn struct_len(&self, id: usize) -> usize;
    fn struct_new(&mut self) -> usize;
    fn struct_set(&mut self, id: usize, key: &str, val: BxValue);
    fn struct_get(&self, id: usize, key: &str) -> BxValue;
    fn struct_delete(&mut self, id: usize, key: &str) -> bool;
    fn struct_key_exists(&self, id: usize, key: &str) -> bool;
    fn struct_key_array(&self, id: usize) -> Vec<String>;
    fn struct_clear(&mut self, id: usize);
    fn struct_get_shape(&self, id: usize) -> u32;
    fn future_on_error(&mut self, id: usize, handler: BxValue);
    fn native_object_new(&mut self, obj: Rc<RefCell<dyn BxNativeObject>>) -> usize;
    fn construct_native_class(&mut self, class_name: &str, args: &[BxValue]) -> Result<BxValue, String>;
    fn string_new(&mut self, s: String) -> usize;
    fn to_string(&self, val: BxValue) -> String;
    fn to_box_string(&self, val: BxValue) -> BoxString;
    fn get_cli_args(&self) -> Vec<String>;
}

pub type BxNativeFunction = fn(&mut dyn BxVM, &[BxValue]) -> Result<BxValue, String>;

pub trait BxNativeObject: fmt::Debug {
    fn get_property(&self, name: &str) -> BxValue;
    fn set_property(&mut self, name: &str, value: BxValue);
    fn call_method(&mut self, vm: &mut dyn BxVM, name: &str, args: &[BxValue]) -> Result<BxValue, String>;
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxCompiledFunction {
    pub name: String,
    pub arity: u32,     // Total parameters
    pub min_arity: u32, // Required parameters
    pub params: Vec<String>, // Parameter names
    pub chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    #[serde(skip)]
    pub promoted_constants: RefCell<Vec<Option<BxValue>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxClass {
    pub name: String,
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub constructor: Rc<BxCompiledFunction>,
    pub methods: HashMap<String, Rc<BxCompiledFunction>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxInterface {
    pub name: String,
    pub methods: HashMap<String, Option<Rc<BxCompiledFunction>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxStruct {
    pub shape_id: u32,
    pub properties: Vec<BxValue>,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FutureStatus {
    Pending,
    Completed,
    Failed(String),
}
