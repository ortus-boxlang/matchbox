//! Shared BoxLang frontend. Parsing and source inspection do not depend on the VM.
pub mod ast;
pub mod cst;
pub mod parser;
pub mod tokenizer;
pub mod inspect;
pub mod visit;
