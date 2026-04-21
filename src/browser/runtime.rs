pub fn render_fusion_web_host_source(registration_calls: &str, bytecode: &[u8]) -> String {
    format!(
        r#"
use matchbox_vm::{{vm::{{HostFutureState, VM}}, types::{{BxNativeFunction, BxValue}}, Chunk}};
use std::collections::HashMap;
use console_error_panic_hook;
use js_sys::{{Array, Function, Promise, Error}};
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::window;

fn as_js_error(message: impl Into<String>) -> JsValue {{
    Error::new(&message.into()).into()
}}

fn log_error(message: impl Into<String>) {{
    if let Some(win) = window() {{
        let console = win.console();
        console.error_1(&JsValue::from_str(&message.into()));
    }}
}}

async fn yield_to_host() -> Result<(), JsValue> {{
    let promise = Promise::new(&mut |resolve: Function, reject: Function| {{
        let win: web_sys::Window = match window() {{
            Some(win) => win,
            None => {{
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("window is unavailable"));
                return;
            }}
        }};

        if let Err(err) = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0) {{
            let _ = reject.call1(&JsValue::NULL, &err);
        }}
    }});

    let _ = JsFuture::from(promise).await?;
    Ok(())
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
}}

#[wasm_bindgen]
impl BoxLangVM {{
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<BoxLangVM, JsValue> {{
        console_error_panic_hook::set_once();
        let vm = new_vm();
        Ok(BoxLangVM {{ vm }})
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

    pub fn pump(&mut self) -> Result<(), JsValue> {{
        self.vm
            .pump_until_blocked()
            .map_err(|e| as_js_error(format!("VM Runtime Error: {{}}", e)))
    }}

    pub async fn call(&mut self, name: &str, args: Array) -> Result<JsValue, JsValue> {{
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

        let future = match self.vm.start_call_function_value(func, bx_args) {{
            Ok(f) => f,
            Err(e) => {{
                let msg = format!("VM Runtime Error: {{}}", e);
                log_error(&msg);
                return Err(as_js_error(msg));
            }}
        }};

        loop {{
            if let Err(e) = self.vm.pump_until_blocked() {{
                let msg = format!("VM Runtime Error: {{}}", e);
                log_error(&msg);
                return Err(as_js_error(msg));
            }}

            match self.vm.future_state(future) {{
                Ok(HostFutureState::Pending) => yield_to_host().await?,
                Ok(HostFutureState::Completed(value)) => return Ok(self.vm.bx_to_js(&value)),
                Ok(HostFutureState::Failed(error)) => {{
                    let js_err = self.vm.bx_to_js(&error);
                    let msg = js_err.as_string().unwrap_or_else(|| "Unknown VM error".to_string());
                    log_error(&msg);
                    return Err(as_js_error(msg));
                }}
                Err(e) => {{
                    let msg = format!("VM Runtime Error: {{}}", e);
                    log_error(&msg);
                    return Err(as_js_error(msg));
                }}
            }}
        }}
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
        assert!(source.contains("start_call_function_value"));
        assert!(source.contains("pump_until_blocked"));
        assert!(source.contains("future_state"));
        assert!(source.contains("HostFutureState::Pending"));
    }
}
