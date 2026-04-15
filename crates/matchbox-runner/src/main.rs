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
    use std::cell::RefCell;
    use std::rc::Rc;

    #[wasm_bindgen]
    pub fn run_bytecode(bytecode: &[u8]) -> Result<String, JsValue> {
        console_error_panic_hook::set_once();
        let mut chunk: Chunk = postcard::from_bytes(bytecode)
            .map_err(|e| JsValue::from_str(&format!("Deserialize Error: {}", e)))?;
        chunk.reconstruct_functions();
        
        let mut vm = VM::new();
        let res = vm.interpret_sync(chunk)
            .map_err(|e| JsValue::from_str(&format!("Runtime Error: {}", e)))?;
        
        Ok(res.to_string())
    }

    #[wasm_bindgen]
    pub struct BoxLangVM {
        vm: VM,
        chunk: Option<Rc<RefCell<Chunk>>>,
    }

    #[wasm_bindgen]
    impl BoxLangVM {
        #[wasm_bindgen(constructor)]
        pub fn new() -> BoxLangVM {
            console_error_panic_hook::set_once();
            BoxLangVM {
                vm: VM::new(),
                chunk: None,
            }
        }

        pub fn load_bytecode(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
            let res = (|| -> Result<()> {
                let mut chunk: Chunk = postcard::from_bytes(bytes)?;
                chunk.reconstruct_functions();
                let chunk_rc = Rc::new(RefCell::new(chunk.clone()));
                self.chunk = Some(chunk_rc);
                self.vm.interpret_sync(chunk)?;
                Ok(())
            })();
            res.map_err(|e| js_sys::Error::new(&format!("Error: {}", e)).into())
        }

        pub fn vm_ptr(&self) -> usize {
            &self.vm as *const VM as usize
        }

        pub fn call(&mut self, name: &str, args: js_sys::Array) -> Result<JsValue, JsValue> {
            let mut bx_args = Vec::new();
            for i in 0..args.length() {
                bx_args.push(self.vm.js_to_bx(args.get(i)));
            }

            let func = self.vm.get_global(name)
                .ok_or_else(|| js_sys::Error::new(&format!("Function {} not found", name)))?;

            let future = self.vm.start_call_function_value(func, bx_args)
                .map_err(|e| -> JsValue { js_sys::Error::new(&format!("Error: {}", e)).into() })?;

            let val = self.vm.run_future_to_completion(future)
                .map_err(|e| -> JsValue { js_sys::Error::new(&format!("Error: {}", e)).into() })?;

            Ok(self.vm.bx_to_js(&val))
        }
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
