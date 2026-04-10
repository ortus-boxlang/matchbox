use crate::features::BundledFeatures;
use crate::profile::StrictProfile;
use crate::wifi::WifiState;
use anyhow::Result;
use embedded_svc::http::Method;
use embedded_svc::http::server::Request;
use embedded_svc::io::Read as _;
use embedded_svc::io::Write as _;
use esp_idf_svc::http::server::{Configuration as HttpConfiguration, EspHttpConnection, EspHttpServer};
use matchbox_vm::{types::{BxVM, BxValue}, vm::VM, Chunk};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

static EMBEDDED_ROUTE_TABLE_JSON: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/embedded-route-table.json"));

// ESP32-specific warmed VM holder. This is intentionally separate from the
// general MatchBox runtime model so the embedded runner can reuse one warmed VM
// across requests without reintroducing per-request VM construction.
struct SharedEsp32Vm {
    ptr: usize,
}

impl SharedEsp32Vm {
    fn new() -> Self {
        let vm = Box::new(VM::new_with_bifs(crate::esp32_bifs::register_bifs(), HashMap::new()));
        Self {
            ptr: Box::into_raw(vm) as usize,
        }
    }

    fn with_vm<R>(&mut self, f: impl FnOnce(&mut VM) -> R) -> R {
        let vm = unsafe { &mut *(self.ptr as *mut VM) };
        f(vm)
    }
}

// This wrapper is an embedded-only escape hatch. Access is serialized through
// a Mutex in the runner, and it should be unified with a cleaner shared runtime
// model once the main VM grows one.
unsafe impl Send for SharedEsp32Vm {}
unsafe impl Sync for SharedEsp32Vm {}

#[derive(Clone, Debug, Default, Deserialize)]
struct EmbeddedRouteTable {
    routes: Vec<EmbeddedRouteTableEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EmbeddedRouteTableEntry {
    method: String,
    path: String,
    source_kind: String,
    source_path: String,
    bytecode: Vec<u8>,
}

#[derive(Clone, Debug)]
struct ExecutableRouteTable {
    routes: Vec<ExecutableRouteTableEntry>,
}

#[derive(Clone, Debug)]
struct ExecutableRouteTableEntry {
    method: String,
    path: String,
    source_kind: String,
    source_path: String,
    chunk: Arc<Chunk>,
}

#[derive(Clone, Debug, Default)]
struct RequestContextData {
    method: String,
    path: String,
    url: HashMap<String, String>,
    form: HashMap<String, String>,
    request: HashMap<String, String>,
    cgi: HashMap<String, String>,
}

pub fn serve(
    profile: &StrictProfile,
    features: BundledFeatures,
    wifi_state: &WifiState,
) -> Result<()> {
    let mut config = HttpConfiguration::default();
    config.http_port = profile.web_port;
    config.stack_size = 16384;
    config.max_sessions = 2;
    config.max_open_sockets = 2;
    config.max_uri_handlers = 12;
    config.lru_purge_enable = true;
    config.uri_match_wildcard = true;

    let mut server = EspHttpServer::new(&config)?;
    let hostname = profile.wifi_hostname.to_string();
    let ip = wifi_state.ip.clone();
    let feature_summary = features.describe();
    let route_table = Arc::new(load_executable_route_table());
    let shared_vm = Arc::new(Mutex::new(SharedEsp32Vm::new()));
    let route_count = route_table.routes.len();

    server.fn_handler("/__matchbox", Method::Get, move |request| {
        let html = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{hostname}</title></head><body><main><h1>{hostname}</h1><p>Bundled ESP32 runner is online.</p><p>IP: {ip}</p><p>Features: {feature_summary}</p><p>Embedded routes: {route_count}</p></main></body></html>"
        );
        request
            .into_ok_response()?
            .write_all(html.as_bytes())
            .map(|_| ())
    })?;

    let hostname = profile.wifi_hostname.to_string();
    let ip = wifi_state.ip.clone();
    let feature_summary = features.describe();
    let status_routes: Vec<_> = route_table
        .routes
        .iter()
        .map(|route| {
            json!({
                "method": route.method,
                "path": route.path,
                "sourceKind": route.source_kind,
                "sourcePath": route.source_path,
            })
        })
        .collect();
    server.fn_handler("/__matchbox/status", Method::Get, move |request| {
        let payload = json!({
            "ok": true,
            "hostname": hostname,
            "ip": ip,
            "features": feature_summary,
            "routes": status_routes,
        });
        let body = serde_json::to_vec(&payload).unwrap_or_else(|_| br#"{"ok":false}"#.to_vec());
        let mut response = request
            .into_response(200, Some("OK"), &[("content-type", "application/json")])
            .map_err(anyhow::Error::msg)?;
        response.write_all(&body).map(|_| ()).map_err(anyhow::Error::msg)
    })?;

    for method in [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
        Method::Head,
        Method::Options,
    ] {
        let route_table = Arc::clone(&route_table);
        let shared_vm = Arc::clone(&shared_vm);
        server.fn_handler("/*", method, move |request| {
            respond_with_embedded_route(request, route_table.as_ref(), shared_vm.as_ref())
        })?;
    }

    println!(
        "[matchbox] Embedded app server listening on http://{}:{}",
        wifi_state.ip, profile.web_port
    );
    core::mem::forget(server);
    Ok(())
}

fn load_route_table() -> EmbeddedRouteTable {
    if let Some(table) = load_route_table_from_storage() {
        return table;
    }

    if EMBEDDED_ROUTE_TABLE_JSON.is_empty() {
        return EmbeddedRouteTable::default();
    }

    postcard::from_bytes(EMBEDDED_ROUTE_TABLE_JSON).unwrap_or_default()
}

fn load_executable_route_table() -> ExecutableRouteTable {
    let route_table = load_route_table();
    let mut routes = Vec::with_capacity(route_table.routes.len());

    for route in route_table.routes {
        let mut chunk: Chunk = match postcard::from_bytes(&route.bytecode) {
            Ok(chunk) => chunk,
            Err(error) => {
                println!(
                    "[matchbox] Failed to deserialize route bytecode for {} {} ({}): {}",
                    route.method, route.path, route.source_path, error
                );
                continue;
            }
        };
        chunk.reconstruct_functions();

        routes.push(ExecutableRouteTableEntry {
            method: route.method,
            path: route.path,
            source_kind: route.source_kind,
            source_path: route.source_path,
            chunk: Arc::new(chunk),
        });
    }

    println!(
        "[matchbox] Prepared {} executable embedded routes",
        routes.len()
    );

    ExecutableRouteTable { routes }
}

fn load_route_table_from_storage() -> Option<EmbeddedRouteTable> {
    unsafe {
        let partition = esp_idf_sys::esp_partition_find_first(
            esp_idf_sys::esp_partition_type_t_ESP_PARTITION_TYPE_DATA,
            0x81,
            std::ptr::null(),
        );
        if partition.is_null() {
            return None;
        }

        let size = (*partition).size as usize;
        if size < 4 {
            return None;
        }

        let mut map_handle: esp_idf_sys::esp_partition_mmap_handle_t = 0;
        let mut map_ptr: *const c_void = std::ptr::null();
        let err = esp_idf_sys::esp_partition_mmap(
            partition,
            0,
            size,
            esp_idf_sys::esp_partition_mmap_memory_t_ESP_PARTITION_MMAP_DATA,
            &mut map_ptr,
            &mut map_handle,
        );
        if err != 0 || map_ptr.is_null() {
            return None;
        }

        let data_ptr = map_ptr as *const u8;
        let len = u32::from_le_bytes([
            *data_ptr,
            *data_ptr.add(1),
            *data_ptr.add(2),
            *data_ptr.add(3),
        ]) as usize;

        if len == 0 || len > size.saturating_sub(4) {
            esp_idf_sys::esp_partition_munmap(map_handle);
            return None;
        }

        let payload = std::slice::from_raw_parts(data_ptr.add(4), len);
        let parsed = postcard::from_bytes(payload).ok();
        esp_idf_sys::esp_partition_munmap(map_handle);

        if parsed.is_some() {
            println!("[matchbox] Loaded embedded app artifact from storage partition");
        }
        parsed
    }
}

fn respond_with_embedded_route(
    mut request: Request<&mut EspHttpConnection<'_>>,
    route_table: &ExecutableRouteTable,
    shared_vm: &Mutex<SharedEsp32Vm>,
) -> anyhow::Result<()> {
    let method = method_name(request.method());
    let request_uri = request.uri().to_string();
    let request_path = request_uri
        .split_once('?')
        .map(|(path, _)| path.to_string())
        .unwrap_or(request_uri);
    let query_params = parse_query_params(request.uri());
    let form_fields = read_form_fields(&mut request);

    if let Some((route, params)) = match_route(route_table, method, &request_path) {
        log_heap(&format!("route-match {} {}", method, request_path));
        let context = build_request_context(method, &request_path, params, query_params, form_fields);
        return match execute_embedded_route(route, &context, shared_vm) {
            Ok(RouteExecution::Html(body)) => request
                .into_response(200, Some("OK"), &[("content-type", "text/html; charset=utf-8")])
                .map_err(anyhow::Error::msg)?
                .write_all(body.as_bytes())
                .map(|_| ())
                .map_err(anyhow::Error::msg),
            Ok(RouteExecution::Json(body)) => {
                let mut response = request
                    .into_response(200, Some("OK"), &[("content-type", "application/json")])
                    .map_err(anyhow::Error::msg)?;
                response
                    .write_all(body.as_bytes())
                    .map(|_| ())
                    .map_err(anyhow::Error::msg)
            }
            Err(error) => {
                let body = format!("Embedded route execution failed: {}", error);
                request
                    .into_response(
                        500,
                        Some("Internal Server Error"),
                        &[("content-type", "text/plain; charset=utf-8")],
                    )
                    .map_err(anyhow::Error::msg)?
                    .write_all(body.as_bytes())
                    .map(|_| ())
                    .map_err(anyhow::Error::msg)
            }
        };
    }

    request
        .into_response(404, Some("Not Found"), &[("content-type", "text/plain; charset=utf-8")])
        .map_err(anyhow::Error::msg)?
        .write_all(b"Embedded route not found")
        .map(|_| ())
        .map_err(anyhow::Error::msg)
}

fn method_name(method: Method) -> &'static str {
    match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Patch => "PATCH",
        Method::Delete => "DELETE",
        Method::Head => "HEAD",
        Method::Options => "OPTIONS",
        _ => "GET",
    }
}

fn match_route<'a>(
    route_table: &'a ExecutableRouteTable,
    method: &str,
    path: &str,
) -> Option<(&'a ExecutableRouteTableEntry, HashMap<String, String>)> {
    let normalized_path = normalize_route_path(path);
    for route in &route_table.routes {
        if route.method != method {
            continue;
        }
        if let Some(params) = match_path_pattern(&route.path, &normalized_path) {
            return Some((route, params));
        }
    }
    None
}

enum RouteExecution {
    Html(String),
    Json(String),
}

fn execute_embedded_route(
    route: &ExecutableRouteTableEntry,
    context: &RequestContextData,
    shared_vm: &Mutex<SharedEsp32Vm>,
) -> anyhow::Result<RouteExecution> {
    println!(
        "[matchbox] Executing embedded route method={} path={} kind={}",
        route.method, route.path, route.source_kind
    );
    // New borrowed execution path for ESP32. This avoids the old per-request
    // route chunk clone and lets the runner reuse preloaded route programs.
    log_heap("before-route-chunk-borrowed");

    let mut shared_vm = shared_vm.lock().unwrap();
    shared_vm.with_vm(|vm| {
        log_heap("after-vm-acquire");
        install_scope(vm, "url", &context.url);
        install_scope(vm, "form", &context.form);
        install_scope(vm, "request", &context.request);
        install_scope(vm, "cgi", &context.cgi);
        log_heap("after-scope-install");

        vm.begin_output_capture();
        log_heap("before-interpret-chunk");
        let result = vm
            .interpret_chunk_borrowed(route.chunk.as_ref())
            .map_err(anyhow::Error::msg)?;
        log_heap("after-interpret-chunk");
        let output = vm.end_output_capture().unwrap_or_default();

        if route.source_kind == "template" {
            return Ok(RouteExecution::Html(output));
        }

        if !output.is_empty() {
            return Ok(RouteExecution::Html(output));
        }

        let json = vm.bx_to_json(&result);
        Ok(RouteExecution::Json(serde_json::to_string(&json)?))
    })
}

fn log_heap(label: &str) {
    unsafe {
        let free = esp_idf_sys::esp_get_free_heap_size();
        let largest = esp_idf_sys::heap_caps_get_largest_free_block(
            esp_idf_sys::MALLOC_CAP_INTERNAL | esp_idf_sys::MALLOC_CAP_8BIT,
        );
        println!(
            "[matchbox] heap {} free={} largest={}",
            label, free, largest
        );
    }
}

fn install_scope(vm: &mut VM, scope_name: &str, values: &HashMap<String, String>) {
    let scope_id = vm.struct_new();
    for (key, value) in values {
        let value_id = vm.string_new(value.clone());
        vm.struct_set(scope_id, key, BxValue::new_ptr(value_id));
    }
    vm.insert_global(scope_name.to_string(), BxValue::new_ptr(scope_id));
}

fn build_request_context(
    method: &str,
    path: &str,
    route_params: HashMap<String, String>,
    query_params: HashMap<String, String>,
    form_fields: HashMap<String, String>,
) -> RequestContextData {
    let mut url = query_params;
    for (key, value) in route_params {
        url.insert(key, value);
    }

    let mut cgi = HashMap::new();
    cgi.insert("request_method".to_string(), method.to_string());
    cgi.insert("path_info".to_string(), path.to_string());
    cgi.insert("request_uri".to_string(), path.to_string());

    RequestContextData {
        method: method.to_string(),
        path: path.to_string(),
        url,
        form: form_fields,
        request: HashMap::new(),
        cgi,
    }
}

fn parse_query_params(uri: &str) -> HashMap<String, String> {
    let query = match uri.split_once('?') {
        Some((_, query)) => query,
        None => return HashMap::new(),
    };

    url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

fn read_form_fields(request: &mut Request<&mut EspHttpConnection<'_>>) -> HashMap<String, String> {
    let content_type = request.header("content-type").unwrap_or_default().to_ascii_lowercase();
    let content_length = request
        .header("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);

    if content_length == 0 {
        return HashMap::new();
    }

    let mut body = vec![0u8; content_length];
    if request.read_exact(&mut body).is_err() {
        return HashMap::new();
    }

    if content_type.starts_with("application/x-www-form-urlencoded") {
        return url::form_urlencoded::parse(&body).into_owned().collect();
    }

    HashMap::new()
}

fn normalize_route_path(path: &str) -> String {
    if path == "/" || path.trim().is_empty() {
        return "/".to_string();
    }

    let trimmed = path.trim().trim_matches('/');
    format!("/{}", trimmed)
}

fn match_path_pattern(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let pattern_segments: Vec<_> = pattern.trim_matches('/').split('/').collect();
    let path_segments: Vec<_> = path.trim_matches('/').split('/').collect();

    let pattern_segments = if pattern_segments.len() == 1 && pattern_segments[0].is_empty() {
        Vec::new()
    } else {
        pattern_segments
    };

    let path_segments = if path_segments.len() == 1 && path_segments[0].is_empty() {
        Vec::new()
    } else {
        path_segments
    };

    if pattern_segments.len() != path_segments.len() {
        return None;
    }

    let mut params = HashMap::new();
    for (pattern_segment, path_segment) in pattern_segments.iter().zip(path_segments.iter()) {
        if let Some(name) = pattern_segment.strip_prefix(':') {
            params.insert(name.to_string(), (*path_segment).to_string());
        } else if pattern_segment != path_segment {
            return None;
        }
    }

    Some(params)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
