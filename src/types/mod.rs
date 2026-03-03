use std::fmt;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum BxValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
    Function(BxFunction),
    // Array(Vec<BxValue>),
    // Struct(HashMap<String, BxValue>),
}

impl fmt::Display for BxValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BxValue::String(s) => write!(f, "{}", s),
            BxValue::Number(n) => write!(f, "{}", n),
            BxValue::Boolean(b) => write!(f, "{}", b),
            BxValue::Null => write!(f, "null"),
            BxValue::Function(func) => write!(f, "<function {}>", func.name),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BxFunction {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<crate::ast::Statement>,
}
