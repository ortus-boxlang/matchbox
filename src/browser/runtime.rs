pub fn render_fusion_web_host_source(registration_calls: &str, bytecode: &[u8]) -> String {
    format!(
        r#"
use matchbox_vm::{{vm::{{HostFutureState, VM}}, types::{{BxNativeFunction, BxValue}}, Chunk}};
use std::collections::HashMap;
use console_error_panic_hook;
use js_sys::{{Array, Error}};
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsValue;
use web_sys::window;
use matchbox_vm::types::BxVM;
use matchbox_vm::vm::gc::RootedValue;
use std::rc::Rc;

fn as_js_error(message: impl Into<String>) -> JsValue {{
    Error::new(&message.into()).into()
}}

fn log_error(message: impl Into<String>) {{
    if let Some(win) = window() {{
        let console = win.console();
        console.error_1(&JsValue::from_str(&message.into()));
    }}
}}

fn new_vm() -> VM {{
    let mut bifs = HashMap::new();
    let mut classes = HashMap::new();
{registration_calls}    VM::new_with_bifs(bifs, classes)
}}

fn embedded_chunk() -> Result<Chunk, String> {{
    let bytecode: Vec<u8> = vec!{bytecode:?};
    let mut chunk: Chunk = postcard::from_bytes(&bytecode).map_err(|e| e.to_string())?;
    chunk.reconstruct_functions();
    Ok(chunk)
}}

#[wasm_bindgen]
pub struct BoxLangVM {{
    vm: VM,
    /// Calls started with `start` and not yet finished. Retained so the GC keeps
    /// their futures alive while the host waits.
    calls: HashMap<u32, Rc<RootedValue>>,
    next_call: u32,
}}

#[wasm_bindgen]
impl BoxLangVM {{
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<BoxLangVM, JsValue> {{
        console_error_panic_hook::set_once();
        let vm = new_vm();
        Ok(BoxLangVM {{ vm, calls: HashMap::new(), next_call: 1 }})
    }}

    /// Run embedded bytecode. Must be called after the callback bridge is
    /// registered so that any callbacks created during top-level execution
    /// use the VM's final heap address.
    pub fn init(&mut self) -> Result<(), JsValue> {{
        let chunk = embedded_chunk().map_err(as_js_error)?;
        if let Err(e) = self.vm.interpret_sync(chunk) {{
            let msg = format!("VM Runtime Error: {{}}", e);
            log_error(&msg);
            return Err(as_js_error(msg));
        }}
        Ok(())
    }}

    pub fn vm_ptr(&self) -> usize {{
        &self.vm as *const VM as usize
    }}

    /// Identifies this VM to the JS host. Call before `init`.
    pub fn set_host_id(&mut self, host_id: usize) {{
        self.vm.host_id = host_id;
    }}

    pub fn pump(&mut self) -> Result<(), JsValue> {{
        self.vm
            .pump_until_blocked()
            .map_err(|e| as_js_error(format!("VM Runtime Error: {{}}", e)))
    }}

    /// Starts `name(...args)` as its own fiber and returns a handle for `poll`.
    /// Synchronous, so any number of calls can be in flight on one VM.
    pub fn start(&mut self, name: &str, args: Array) -> Result<u32, JsValue> {{
        let func = match self.vm.get_global(name) {{
            Some(f) => f,
            None => {{
                let msg = format!("Function {{}} not found", name);
                log_error(&msg);
                return Err(as_js_error(msg));
            }}
        }};

        let mut bx_args = Vec::new();
        for idx in 0..args.length() {{
            bx_args.push(self.vm.js_to_bx(args.get(idx)));
        }}

        let future = self.vm.start_call_function_value(func, bx_args).map_err(|e| {{
            let msg = format!("VM Runtime Error: {{}}", e);
            log_error(&msg);
            as_js_error(msg)
        }})?;
        let root = self.vm.retain_value(future).map_err(as_js_error)?;
        let handle = self.next_call;
        self.next_call += 1;
        self.calls.insert(handle, root);
        Ok(handle)
    }}

    /// Returns `{{ done: false }}` while the call is running and `{{ done: true, value }}` when it
    /// has finished. A failed call throws. A finished handle is released.
    pub fn poll(&mut self, handle: u32) -> Result<JsValue, JsValue> {{
        let root = self.calls.get(&handle)
            .ok_or_else(|| as_js_error(format!("Unknown call handle {{}}", handle)))?;
        let future = self.vm.read_retained_value(root).map_err(as_js_error)?;
        let state = self.vm
            .future_state(future)
            .map_err(|e| as_js_error(format!("VM Runtime Error: {{}}", e)))?;
        let result = js_sys::Object::new();
        match state {{
            HostFutureState::Pending => {{
                js_sys::Reflect::set(&result, &"done".into(), &JsValue::FALSE)?;
            }}
            HostFutureState::Completed(value) => {{
                js_sys::Reflect::set(&result, &"done".into(), &JsValue::TRUE)?;
                js_sys::Reflect::set(&result, &"value".into(), &self.vm.bx_to_js(&value))?;
                self.calls.remove(&handle);
            }}
            HostFutureState::Failed(error) => {{
                let msg = self.vm.format_error_value(error);
                self.calls.remove(&handle);
                return Err(as_js_error(msg));
            }}
        }}
        Ok(result.into())
    }}
}}
"#,
        registration_calls = registration_calls,
        bytecode = bytecode
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fusion_web_host_source_registers_modules_and_uses_scheduler_api() {
        let source = render_fusion_web_host_source(
            "    for (name, val) in demo::register_bifs() { bifs.insert(name, val); }\n",
            &[1, 2, 3],
        );

        assert!(source.contains("demo::register_bifs()"));
        assert!(source.contains("VM::new_with_bifs(bifs, classes)"));
        assert!(source.contains("fn init(&mut self)"));
        assert!(source.contains(".interpret_sync(chunk)"));
        assert!(source.contains("vm_ptr"));
        assert!(source.contains("fn pump(&mut self)"));
        assert!(source.contains("fn start(&mut self"));
        assert!(source.contains("fn poll(&mut self"));
        assert!(source.contains("start_call_function_value"));
        assert!(source.contains("pump_until_blocked"));
        assert!(source.contains("future_state"));
        assert!(source.contains("HostFutureState::Pending"));
        assert!(!source.contains("pub async fn"));
    }
}
