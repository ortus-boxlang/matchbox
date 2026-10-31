#[cfg(feature = "bif-http")]
use crate::types::{BxVM, BxValue, NativeFutureValue};

#[cfg(all(feature = "bif-http", not(target_arch = "wasm32")))]
use std::fs::File;
#[cfg(feature = "bif-http")]
use std::time::Duration;

#[cfg(all(feature = "bif-http", target_arch = "wasm32", feature = "js"))]
use web_sys::{Request, RequestInit, RequestMode};

#[cfg(feature = "bif-http")]
struct RequestOptions {
    timeout: Option<Duration>,
    connection_timeout: Option<Duration>,
    redirect: bool,
    no_proxy: bool,
    ipv4_only: bool,
}

#[cfg(feature = "bif-http")]
impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            timeout: Some(Duration::from_secs(30)),
            connection_timeout: None,
            redirect: true,
            no_proxy: false,
            ipv4_only: false,
        }
    }
}

#[cfg(feature = "bif-http")]
fn request_bool(vm: &dyn BxVM, spec: usize, key: &str, default: bool) -> Result<bool, String> {
    if !vm.struct_key_exists(spec, key) {
        return Ok(default);
    }
    let value = vm.struct_get(spec, key);
    if !value.is_bool() {
        return Err(format!("http() option '{key}' must be a boolean"));
    }
    Ok(value.as_bool())
}

#[cfg(feature = "bif-http")]
fn request_timeout(
    vm: &dyn BxVM,
    spec: usize,
    key: &str,
    default: Option<Duration>,
) -> Result<Option<Duration>, String> {
    if !vm.struct_key_exists(spec, key) {
        return Ok(default);
    }
    let seconds = vm.struct_get(spec, key).as_number();
    let invalid = || {
        format!(
            "http() option '{key}' must be a non-negative finite number of seconds within the supported clock range"
        )
    };
    let duration = Duration::try_from_secs_f64(seconds).map_err(|_| invalid())?;
    #[cfg(not(target_arch = "wasm32"))]
    if std::time::Instant::now().checked_add(duration).is_none() {
        return Err(invalid());
    }
    Ok((seconds != 0.0).then_some(duration))
}

#[cfg(feature = "bif-http")]
fn request_headers(vm: &mut dyn BxVM, spec: usize) -> Vec<(String, String)> {
    let headers = vm.struct_get(spec, "headers");
    let Some(id) = headers.as_gc_id() else {
        return Vec::new();
    };
    if !vm.is_struct_value(headers) {
        return Vec::new();
    }
    vm.struct_key_array(id)
        .into_iter()
        .map(|key| {
            let value = vm.to_string(vm.struct_get(id, &key));
            (key, value)
        })
        .collect()
}

#[cfg(feature = "bif-http")]
fn request_body(vm: &mut dyn BxVM, spec: usize) -> Option<String> {
    if !vm.struct_key_exists(spec, "body") {
        return None;
    }
    let body = vm.struct_get(spec, "body");
    if body.is_null() {
        return None;
    }
    Some(vm.to_string(body))
}

#[cfg(all(feature = "bif-http", not(target_arch = "wasm32")))]
fn request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "HTTP request timed out".to_string()
    } else {
        format!("HTTP request failed: {}", error.without_url())
    }
}

#[cfg(all(feature = "bif-http", not(target_arch = "wasm32")))]
fn send_http(
    url: &str,
    method: &str,
    headers: &[(String, String)],
    body: Option<String>,
    options: &RequestOptions,
) -> Result<reqwest::blocking::Response, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(options.timeout)
        .connect_timeout(options.connection_timeout)
        .redirect(if options.redirect {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        });
    if options.no_proxy {
        builder = builder.no_proxy();
    }
    if options.ipv4_only {
        builder = builder.local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    }
    let client = builder
        .build()
        .map_err(|e| format!("HTTP client configuration failed: {}", e.without_url()))?;
    let mut request = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        _ => return Err(format!("Unsupported HTTP method: {method}")),
    };
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    if let Some(body) = body {
        request = request.body(body);
    }
    if let Some(timeout) = options.timeout {
        // The per-request deadline also bounds streamed bodies, not just each blocking read.
        request = request.timeout(timeout);
    }
    request.send().map_err(request_error)
}

/// Builds the `headers` struct for an http() result: lowercase header name -> array of values.
/// Scripts can then inspect content types, redirect locations, cookies, and other metadata
/// that were previously discarded. Every value is an array, even for single-value headers,
/// so scripts always see the same shape.
#[cfg(all(feature = "bif-http", not(target_arch = "wasm32")))]
fn response_headers(headers: &reqwest::header::HeaderMap) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    // HeaderMap yields one entry per received value, in order, so repeated headers such as
    // Set-Cookie stay separate instead of being joined or overwritten. Names are already
    // lowercase in HeaderMap.
    for (name, value) in headers {
        // Header values are raw bytes and may not be valid UTF-8. Lossy decoding substitutes
        // U+FFFD for invalid bytes instead of failing or panicking.
        let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
        // Create the array the first time a name appears, then append each later value.
        if let serde_json::Value::Array(values) = result
            .entry(name.as_str())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        {
            values.push(value.into());
        }
    }
    serde_json::Value::Object(result)
}

#[cfg(feature = "bif-http")]
pub fn http_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let mut url = String::new();
    let mut method = "GET".to_string();
    #[cfg(not(target_arch = "wasm32"))]
    let mut path = None;
    let mut headers = Vec::new();
    let mut body = None;
    let mut options = RequestOptions::default();

    if args.len() == 1 && args[0].as_gc_id().is_some() {
        let id = args[0].as_gc_id().unwrap();
        if vm.struct_key_exists(id, "url") {
            url = vm.to_string(vm.struct_get(id, "url"));
            let m = vm.to_string(vm.struct_get(id, "method"));
            if !m.is_empty() && m != "null" {
                method = m.to_uppercase();
            }
            let p = vm.to_string(vm.struct_get(id, "path"));
            if !p.is_empty() && p != "null" {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    path = Some(p);
                }
            }
            options.timeout = request_timeout(vm, id, "timeout", options.timeout)?;
            options.connection_timeout =
                request_timeout(vm, id, "connectionTimeout", options.connection_timeout)?;
            options.redirect = request_bool(vm, id, "redirect", options.redirect)?;
            options.no_proxy = request_bool(vm, id, "noProxy", options.no_proxy)?;
            options.ipv4_only = request_bool(vm, id, "ipv4Only", options.ipv4_only)?;
            headers = request_headers(vm, id);
            body = request_body(vm, id);
        } else {
            url = vm.to_string(args[0]);
        }
    } else if !args.is_empty() {
        url = vm.to_string(args[0]);
        if args.len() > 1 {
            method = vm.to_string(args[1]).to_uppercase();
        }
        if args.len() > 2 {
            #[cfg(not(target_arch = "wasm32"))]
            {
                path = Some(vm.to_string(args[2]));
            }
        }
    }

    if url.is_empty() || url == "null" {
        return Err("http() requires a URL".to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let handle = vm.native_future_new();
        let future = handle.future();
        std::thread::spawn(move || {
            let result = (|| {
                let mut response = send_http(&url, &method, &headers, body, &options)?;
                let status = response.status().as_u16();
                // Capture headers before reading the body: copy_to()/text() consume the
                // response. reqwest returns the final response when redirects are followed
                // and the original 3xx when they are disabled, so these are the correct
                // headers in both cases. Error statuses (4xx/5xx) also reach this point.
                let response_headers = response_headers(response.headers());
                if let Some(p) = path {
                    let mut file =
                        File::create(&p).map_err(|e| format!("Failed to create file: {e}"))?;
                    response.copy_to(&mut file).map_err(request_error)?;
                    Ok(serde_json::json!({
                        "status": status,
                        // Downloads expose headers too (e.g. content-type of the saved file).
                        "headers": response_headers,
                        "file_path": p
                    }))
                } else {
                    let text = response.text().map_err(request_error)?;
                    Ok(serde_json::json!({
                        "status": status,
                        // Text responses expose the same headers struct as downloads.
                        "headers": response_headers,
                        "file_content": text,
                        "body": text
                    }))
                }
            })();
            match result {
                Ok(value) => {
                    let _ = handle.resolve(NativeFutureValue::String(value.to_string()));
                }
                Err(error) => {
                    let _ = handle.reject(NativeFutureValue::Error { message: error });
                }
            }
        });
        Ok(future)
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    {
        let _ = (headers, body, options);
        let opts = RequestInit::new();
        opts.set_method(&method);
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| format!("Failed to create request: {:?}", e))?;

        let window = web_sys::window().ok_or("No global window object found")?;
        let _request_promise = window.fetch_with_request(&request);

        Err("http() on WASM (fetch) is only supported in async contexts. Return values are not yet synchronous on the web.".to_string())
    }

    #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
    {
        let _ = (method, headers, body, options);
        Err("http() on wasm requires the `js` feature for browser fetch support.".to_string())
    }
}
