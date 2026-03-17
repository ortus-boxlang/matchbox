#[cfg(feature = "bif-http")]
use crate::types::{BxValue, BxVM};

#[cfg(all(feature = "bif-http", not(target_arch = "wasm32")))]
use std::fs::File;
#[cfg(all(feature = "bif-http", not(target_arch = "wasm32")))]
use std::io::copy;

#[cfg(all(feature = "bif-http", target_arch = "wasm32"))]
use wasm_bindgen_futures::JsFuture;
#[cfg(all(feature = "bif-http", target_arch = "wasm32"))]
use web_sys::{Request, RequestInit, RequestMode};

#[cfg(feature = "bif-http")]
pub fn http_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let mut url = String::new();
    let mut method = "GET".to_string();
    let mut path = None;

    if args.len() == 1 && args[0].as_gc_id().is_some() {
        let id = args[0].as_gc_id().unwrap();
        if vm.struct_key_exists(id, "url") {
            url = vm.to_string(vm.struct_get(id, "url"));
            let m = vm.to_string(vm.struct_get(id, "method"));
            if !m.is_empty() && m != "null" { method = m.to_uppercase(); }
            let p = vm.to_string(vm.struct_get(id, "path"));
            if !p.is_empty() && p != "null" { path = Some(p); }
        } else {
            url = vm.to_string(args[0]);
        }
    } else if !args.is_empty() {
            url = vm.to_string(args[0]);
            if args.len() > 1 { method = vm.to_string(args[1]).to_uppercase(); }
            if args.len() > 2 { path = Some(vm.to_string(args[2])); }
    }

    if url.is_empty() || url == "null" {
        return Err("http() requires a URL".to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let client = reqwest::blocking::Client::new();
        let request = match method.as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            _ => return Err(format!("Unsupported HTTP method: {}", method)),
        };

        let mut response = request.send().map_err(|e| format!("HTTP request failed: {}", e))?;
        
        let status = response.status().as_u16();
        
        let result_id = vm.struct_new();
        vm.struct_set(result_id, "status", BxValue::new_number(status as f64));
        
        if let Some(p) = path {
            let mut file = File::create(p).map_err(|e| format!("Failed to create file: {}", e))?;
            copy(&mut response, &mut file).map_err(|e| format!("Failed to download file: {}", e))?;
            let s_id = vm.string_new(url);
            vm.struct_set(result_id, "file_path", BxValue::new_ptr(s_id));
        } else {
            let text = response.text().map_err(|e| format!("Failed to read response body: {}", e))?;
            let s_id = vm.string_new(text);
            vm.struct_set(result_id, "file_content", BxValue::new_ptr(s_id));
        }

        Ok(BxValue::new_ptr(result_id))
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut opts = RequestInit::new();
        opts.method(&method);
        opts.mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| format!("Failed to create request: {:?}", e))?;

        let window = web_sys::window().ok_or("No global window object found")?;
        let _request_promise = window.fetch_with_request(&request);
        
        Err("http() on WASM (fetch) is only supported in async contexts. Return values are not yet synchronous on the web.".to_string())
    }
}
