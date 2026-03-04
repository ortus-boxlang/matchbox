use std::fmt;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BxValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
    Array(Rc<RefCell<Vec<BxValue>>>),
    Struct(Rc<RefCell<BxStruct>>),
    CompiledFunction(Rc<BxCompiledFunction>),
    #[serde(skip)]
    NativeFunction(BxNativeFunction),
    Class(Rc<RefCell<BxClass>>),
    Instance(Rc<RefCell<BxInstance>>),
    Future(Rc<RefCell<BxFuture>>),
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    JsValue(wasm_bindgen::JsValue),
    #[serde(skip)]
    NativeObject(Rc<RefCell<dyn BxNativeObject>>),
    #[cfg(feature = "jvm")]
    #[serde(skip)]
    JavaObject(jni::objects::GlobalRef),
}

pub trait BxVM {
    fn spawn(&mut self, func: Rc<BxCompiledFunction>, args: Vec<BxValue>) -> BxValue;
    fn yield_fiber(&mut self);
    fn sleep(&mut self, ms: u64);
    fn get_root_shape(&self) -> usize;
    fn get_shape_index(&self, shape_id: usize, field_name: &str) -> Option<usize>;
}

pub type BxNativeFunction = fn(&mut dyn BxVM, &[BxValue]) -> Result<BxValue, String>;

pub trait BxNativeObject: fmt::Debug {
    fn get_property(&self, name: &str) -> BxValue;
    fn set_property(&mut self, name: &str, value: BxValue);
    fn call_method(&mut self, vm: &mut dyn BxVM, name: &str, args: &[BxValue]) -> Result<BxValue, String>;
}

// Implement PartialEq for NativeObject manually since dyn trait can't derive it
impl PartialEq for dyn BxNativeObject {
    fn eq(&self, _other: &Self) -> bool {
        false // Identity-based equality is safer for native objects
    }
}

impl fmt::Display for BxValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BxValue::String(s) => write!(f, "{}", s),
            BxValue::Number(n) => write!(f, "{}", n),
            BxValue::Boolean(b) => write!(f, "{}", b),
            BxValue::Null => write!(f, "null"),
            BxValue::Array(arr) => {
                let items: Vec<String> = arr.borrow().iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", items.join(", "))
            }
            BxValue::Struct(s) => {
                write!(f, "<struct shape:{}>", s.borrow().shape_id)
            }
            BxValue::CompiledFunction(func) => write!(f, "<compiled function {}>", func.name),
            BxValue::NativeFunction(_) => write!(f, "<native function>"),
            BxValue::Class(class) => write!(f, "<class {}>", class.borrow().name),
            BxValue::Instance(inst) => write!(f, "<instance of {} shape:{}>", inst.borrow().class.borrow().name, inst.borrow().shape_id),
            BxValue::Future(_) => write!(f, "<future>"),
            #[cfg(target_arch = "wasm32")]
            BxValue::JsValue(js) => write!(f, "<js value {:?}>", js),
            BxValue::NativeObject(obj) => write!(f, "<native object {:?}>", obj.borrow()),
            #[cfg(feature = "jvm")]
            BxValue::JavaObject(_) => write!(f, "<java object>"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxCompiledFunction {
    pub name: String,
    pub arity: usize,
    pub chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxClass {
    pub name: String,
    pub constructor: Rc<RefCell<crate::vm::chunk::Chunk>>,
    pub methods: HashMap<String, Rc<BxCompiledFunction>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxStruct {
    pub shape_id: usize,
    pub properties: Vec<BxValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxInstance {
    pub class: Rc<RefCell<BxClass>>,
    pub shape_id: usize,
    pub properties: Vec<BxValue>,
    pub variables: Rc<RefCell<HashMap<String, BxValue>>>, 
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxFuture {
    pub value: BxValue,
    pub status: FutureStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FutureStatus {
    Pending,
    Completed,
    Failed(String),
}
