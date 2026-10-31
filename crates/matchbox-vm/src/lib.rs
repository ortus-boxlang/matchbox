extern crate self as matchbox_vm;

pub mod bifs;
#[cfg(all(feature = "qoq", not(target_arch = "wasm32")))]
pub mod qoq;
pub mod types;
pub mod vm;
#[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
pub mod plugins;
#[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
pub mod bundle;
#[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
mod module_services;

#[cfg(not(target_arch = "wasm32"))]
pub mod datasource;

pub use matchbox_macros::*;
pub use vm::chunk::Chunk;
