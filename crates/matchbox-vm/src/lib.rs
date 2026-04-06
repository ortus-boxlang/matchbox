extern crate self as matchbox_vm;

pub mod bifs;
pub mod types;
pub mod vm;

#[cfg(not(target_arch = "wasm32"))]
pub mod datasource;

pub use vm::chunk::Chunk;
pub use matchbox_macros::*;
