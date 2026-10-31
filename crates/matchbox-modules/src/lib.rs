//! Shared package discovery and compilation for CLI tools and native HTTP hosts.
pub mod discovery;
#[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
pub mod programs;
