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
    Struct(Rc<RefCell<HashMap<String, BxValue>>>),
    CompiledFunction(Rc<BxCompiledFunction>),
    #[serde(skip)]
    NativeFunction(BxNativeFunction),
    Class(Rc<RefCell<BxClass>>),
    Instance(Rc<RefCell<BxInstance>>),
}

pub type BxNativeFunction = fn(&[BxValue]) -> Result<BxValue, String>;

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
                let items: Vec<String> = s.borrow().iter().map(|(k, v)| {
                    if let BxValue::Struct(inner_s) = v {
                        if Rc::ptr_eq(s, inner_s) {
                            return format!("{}: <recursive struct>", k);
                        }
                    }
                    format!("{}: {}", k, v)
                }).collect();
                write!(f, "{{{}}}", items.join(", "))
            }
            BxValue::CompiledFunction(func) => write!(f, "<compiled function {}>", func.name),
            BxValue::NativeFunction(_) => write!(f, "<native function>"),
            BxValue::Class(class) => write!(f, "<class {}>", class.borrow().name),
            BxValue::Instance(inst) => write!(f, "<instance of {}>", inst.borrow().class.borrow().name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxCompiledFunction {
    pub name: String,
    pub arity: usize,
    pub chunk: crate::vm::chunk::Chunk,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxClass {
    pub name: String,
    pub constructor: crate::vm::chunk::Chunk,
    pub methods: HashMap<String, Rc<BxCompiledFunction>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BxInstance {
    pub class: Rc<RefCell<BxClass>>,
    pub this: Rc<RefCell<HashMap<String, BxValue>>>,
    pub variables: Rc<RefCell<HashMap<String, BxValue>>>,
}
