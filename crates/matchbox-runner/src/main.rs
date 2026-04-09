use matchbox_vm::{Chunk, vm::VM};
use anyhow::Result;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::env as std_env;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::fs;
use postcard;

const MAGIC_FOOTER: &[u8; 8] = b"BOXLANG\x01";

// ---------------------------------------------------------------------------
// WASM Entry Points (Web/Node)
// ---------------------------------------------------------------------------

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod wasm {
    use super::*;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub fn run_bytecode(bytecode: &[u8]) -> Result<String, JsValue> {
        let mut chunk: Chunk = postcard::from_bytes(bytecode)
            .map_err(|e| JsValue::from_str(&format!("Deserialize Error: {}", e)))?;
        chunk.reconstruct_functions();
        
        let mut vm = VM::new();
        let res = vm.interpret(chunk)
            .map_err(|e| JsValue::from_str(&format!("Runtime Error: {}", e)))?;
        
        Ok(res.to_string())
    }
}

// ---------------------------------------------------------------------------
// Native Entry Point (CLI)
// ---------------------------------------------------------------------------

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn load_embedded_bytecode() -> Result<Chunk> {
    let exe_path = std_env::current_exe()?;
    let bytes = fs::read(exe_path)?;
    
    if bytes.len() < 16 {
        return Err(anyhow::anyhow!("Binary too small to contain bytecode"));
    }

    let footer_start = bytes.len() - 8;
    let footer = &bytes[footer_start..];
    if footer != MAGIC_FOOTER {
        return Err(anyhow::anyhow!("No embedded bytecode found"));
    }

    let len_start = footer_start - 8;
    let len_bytes = &bytes[len_start..footer_start];
    let len = u64::from_le_bytes(len_bytes.try_into()?) as usize;

    let chunk_start = len_start - len;
    let chunk_bytes = &bytes[chunk_start..len_start];
    
    let mut chunk: Chunk = postcard::from_bytes(chunk_bytes)?;
    chunk.reconstruct_functions();
    Ok(chunk)
}

// wasm32-unknown-unknown: entry points are the #[wasm_bindgen] exported fns above
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn main() {}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn main() -> Result<()> {
    #[cfg(not(target_family = "wasm"))]
    {
        ctrlc::set_handler(move || {
            matchbox_vm::vm::INTERRUPT_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
        }).expect("Error setting Ctrl-C handler");
    }

    // 1. Try to load embedded bytecode from the executable itself
    let chunk = match load_embedded_bytecode() {
        Ok(c) => c,
        Err(_) => {
            // 2. Fallback: Check for an external 'app.bxb' in the same directory
            let bytes = fs::read("app.bxb")
                .map_err(|_| anyhow::anyhow!("No embedded bytecode or app.bxb found"))?;
            let mut chunk: Chunk = match postcard::from_bytes(&bytes) {
                Ok(c) => c,
                Err(e) => return Err(anyhow::anyhow!("Failed to deserialize app.bxb: {}", e)),
            };
            chunk.reconstruct_functions();
            chunk
        }
    };

    let mut vm = VM::new();
    match vm.interpret(chunk) {
        Ok(val) => {
            // println!("Result: {}", val);
            Ok(())
        }
        Err(e) => {
            eprintln!("Runtime Error: {}", e);
            std::process::exit(1);
        }
    }
}
