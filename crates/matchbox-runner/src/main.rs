use matchbox_vm::{Chunk, vm::VM};
use anyhow::Result;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::env as std_env;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::fs;
use postcard;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
const MAGIC_FOOTER: &[u8; 8] = b"BOXLANG\x01";

// ---------------------------------------------------------------------------
// WASM Entry Points (Web/Node)
// ---------------------------------------------------------------------------

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod wasm {
    use super::*;
    use js_sys::{Array, Error, Function, Promise};
    use wasm_bindgen::JsValue;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::window;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn as_js_error(message: impl Into<String>) -> JsValue {
        Error::new(&message.into()).into()
    }

    async fn yield_to_host() -> Result<(), JsValue> {
        let promise = Promise::new(&mut |resolve: Function, reject: Function| {
            let win = match window() {
                Some(win) => win,
                None => {
                    let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("window is unavailable"));
                    return;
                }
            };

            if let Err(err) = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0) {
                let _ = reject.call1(&JsValue::NULL, &err);
            }
        });

        let _ = JsFuture::from(promise).await?;
        Ok(())
    }

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

        pub fn pump(&mut self) -> Result<(), JsValue> {
            self.vm
                .pump_until_blocked()
                .map_err(|e| as_js_error(format!("Error: {}", e)))
        }

        pub async fn call(&mut self, name: &str, args: Array) -> Result<JsValue, JsValue> {
            let mut bx_args = Vec::new();
            for i in 0..args.length() {
                bx_args.push(self.vm.js_to_bx(args.get(i)));
            }

            let func = self.vm.get_global(name)
                .ok_or_else(|| as_js_error(format!("Function {} not found", name)))?;

            let future = self.vm.start_call_function_value(func, bx_args)
                .map_err(|e| as_js_error(format!("Error: {}", e)))?;

            loop {
                self.vm
                    .pump_until_blocked()
                    .map_err(|e| as_js_error(format!("Error: {}", e)))?;

                match self
                    .vm
                    .future_state(future)
                    .map_err(|e| as_js_error(format!("Error: {}", e)))? {
                    matchbox_vm::vm::HostFutureState::Pending => yield_to_host().await?,
                    matchbox_vm::vm::HostFutureState::Completed(value) => return Ok(self.vm.bx_to_js(&value)),
                    matchbox_vm::vm::HostFutureState::Failed(error) => {
                        let message = self.vm.format_error_value(error);
                        return Err(as_js_error(message));
                    }
                }
            }
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
        Ok(_val) => {
            // println!("Result: {}", val);
            Ok(())
        }
        Err(e) => {
            eprintln!("Runtime Error: {}", e);
            std::process::exit(1);
        }
    }
}
