use std::fmt;

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum BxValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
    Array(Vec<BxValue>),
    Struct(HashMap<String, BxValue>),
    Function(BxFunction),
    Return(Box<BxValue>),
}

impl fmt::Display for BxValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BxValue::String(s) => write!(f, "{}", s),
            BxValue::Number(n) => write!(f, "{}", n),
            BxValue::Boolean(b) => write!(f, "{}", b),
            BxValue::Null => write!(f, "null"),
            BxValue::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", items.join(", "))
            }
            BxValue::Struct(s) => {
                let items: Vec<String> = s.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
                write!(f, "{{{}}}", items.join(", "))
            }
            BxValue::Function(func) => write!(f, "<function {}>", func.name),
            BxValue::Return(val) => write!(f, "{}", val),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BxFunction {
    pub name: String,
    pub params: Vec<String>,
    pub body: crate::ast::FunctionBody,
}
