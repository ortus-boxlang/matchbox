use std::rc::Rc;
use std::cell::RefCell;
use crate::env::Environment;
use crate::types::{BxValue, BxFunction};
use crate::ast::Statement;

pub fn register_bifs(env: &mut Environment) {
    // We can simulate BIFs as special AST nodes, or we can handle them in the evaluator.
    // For this POC, let's treat BIFs as native Rust functions.
    // Since we don't have a NativeFunction variant in BxValue yet, let's just 
    // hook `println` and `echo` directly in the evaluator for now, or add a NativeFunction variant.
}
