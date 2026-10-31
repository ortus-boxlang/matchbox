use crate::camera::with_photo;
use crate::features::BundledFeatures;
use crate::profile::StrictProfile;
use anyhow::Result;
use embedded_svc::http::Method;
use embedded_svc::http::server::Request;
use embedded_svc::io::Read as _;
use embedded_svc::io::Write as _;
use esp_idf_svc::http::server::{
    Configuration as HttpConfiguration, EspHttpConnection, EspHttpServer,
};
use matchbox_vm::{
    Chunk,
    types::{BxVM, BxValue},
    vm::VM,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Esp32AppConfig {
    pub wifi_ssid: String,
    pub wifi_password: String,
    pub wifi_hostname: String,
    pub web_port: u16,
}

impl Default for Esp32AppConfig {
    fn default() -> Self {
        Self {
            wifi_ssid: option_env!("MATCHBOX_ESP32_WIFI_SSID")
                .unwrap_or("")
                .to_string(),
            wifi_password: option_env!("MATCHBOX_ESP32_WIFI_PASSWORD")
                .unwrap_or("")
                .to_string(),
            wifi_hostname: option_env!("MATCHBOX_ESP32_WIFI_HOSTNAME")
                .unwrap_or("matchbox-esp32")
                .to_string(),
            web_port: option_env!("MATCHBOX_ESP32_WEB_PORT")
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(80),
        }
    }
}

static EMBEDDED_ROUTE_TABLE_JSON: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/embedded-route-table.json"));

struct ActiveHttpServer(EspHttpServer<'static>);

// The server is created once on startup and stored only to keep ESP-IDF's
// server handle and URI registrations alive for the process lifetime.
unsafe impl Send for ActiveHttpServer {}

fn active_server() -> &'static Mutex<Option<ActiveHttpServer>> {
    static ACTIVE_SERVER: OnceLock<Mutex<Option<ActiveHttpServer>>> = OnceLock::new();
    ACTIVE_SERVER.get_or_init(|| Mutex::new(None))
}

struct ApplicationVm {
    ptr: usize,
}

impl ApplicationVm {
    fn new() -> Self {
        let vm = Box::new(VM::new_with_bifs(
            crate::esp32_bifs::register_bifs(),
            HashMap::new(),
        ));
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
unsafe impl Send for ApplicationVm {}
unsafe impl Sync for ApplicationVm {}

static APPLICATION_VM: OnceLock<Arc<Mutex<ApplicationVm>>> = OnceLock::new();

fn application_vm() -> Arc<Mutex<ApplicationVm>> {
    APPLICATION_VM
        .get_or_init(|| Arc::new(Mutex::new(ApplicationVm::new())))
        .clone()
}

#[derive(Clone, Debug, Default, Deserialize)]
struct EmbeddedRouteTable {
    #[serde(default)]
    application: Option<EmbeddedApplicationEntry>,
    #[serde(default)]
    routes: Vec<EmbeddedRouteTableEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EmbeddedApplicationEntry {
    source_path: String,
    bytecode: Vec<u8>,
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
pub(crate) struct ExecutableRouteTable {
    application: Option<ExecutableApplicationEntry>,
    routes: Vec<ExecutableRouteTableEntry>,
}

#[derive(Clone, Debug)]
struct ExecutableApplicationEntry {
    source_path: String,
    chunk: Arc<Chunk>,
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

#[derive(Clone, Debug, Serialize)]
struct HeapSnapshot {
    free: usize,
    largest_internal_8bit_block: usize,
    free_internal_8bit: usize,
}

#[derive(Clone, Debug, Serialize)]
struct PartitionSnapshot {
    label: String,
    partition_type: String,
    subtype: u32,
    address: usize,
    size: usize,
}

pub fn serve(profile: &StrictProfile, features: BundledFeatures, ip: &str) -> Result<()> {
    let route_table = load_executable_route_table();
    serve_with_route_table(profile, features, ip, route_table)
}

pub fn serve_with_route_table(
    profile: &StrictProfile,
    features: BundledFeatures,
    ip: &str,
    route_table: ExecutableRouteTable,
) -> Result<()> {
    let mut config = HttpConfiguration::default();
    config.http_port = profile.web_port;
    config.stack_size = 6144;
    config.max_sessions = 2;
    config.max_open_sockets = 2;
    config.max_uri_handlers = 20;
    config.lru_purge_enable = true;
    config.uri_match_wildcard = true;

    let mut server = EspHttpServer::new(&config)?;
    let hostname = profile.wifi_hostname.to_string();
    let ip = ip.to_string();
    let feature_summary = features.describe();
    let route_table = Arc::new(route_table);
    let shared_vm = application_vm();
    let route_count = route_table.routes.len();

    let index_hostname = hostname.clone();
    let index_ip = ip.clone();
    let index_feature_summary = feature_summary.clone();
    for path in [
        "/generate_204",
        "/gen_204",
        "/hotspot-detect.html",
        "/library/test/success.html",
        "/connecttest.txt",
        "/ncsi.txt",
    ] {
        server.fn_handler(path, Method::Get, move |request| {
            println!("[matchbox] captive portal redirect {}", request.uri());
            request
                .into_response(
                    302,
                    Some("Found"),
                    &[
                        ("location", "http://192.168.4.1/"),
                        ("content-type", "text/plain; charset=utf-8"),
                        ("cache-control", "no-store"),
                    ],
                )?
                .write_all(b"Open http://192.168.4.1/")
                .map(|_| ())
        })?;
    }

    server.fn_handler("/__matchbox/ping", Method::Get, move |request| {
        println!("[matchbox] HTTP GET /__matchbox/ping");
        request
            .into_response(200, Some("OK"), &[("content-type", "text/plain")])?
            .write_all(b"pong")
            .map(|_| ())
    })?;

    server.fn_handler("/__matchbox", Method::Get, move |request| {
        println!("[matchbox] HTTP GET /__matchbox");
        let html = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{index_hostname}</title></head><body><main><h1>{index_hostname}</h1><p>Bundled ESP32 runner is online.</p><p>IP: {index_ip}</p><p>Features: {index_feature_summary}</p><p>Embedded routes: {route_count}</p></main></body></html>"
        );
        request
            .into_ok_response()?
            .write_all(html.as_bytes())
            .map(|_| ())
    })?;

    let hostname = profile.wifi_hostname.to_string();
    let status_ip = ip.clone();
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
        let heap = heap_snapshot();
        let partitions = partition_snapshots();
        println!("[matchbox] HTTP GET /__matchbox/status");
        let payload = json!({
            "ok": true,
            "hostname": hostname,
            "ip": status_ip,
            "features": feature_summary,
            "heap": heap,
            "diagnostics": crate::diagnostics::snapshot(),
            "partitions": partitions,
            "routes": status_routes,
        });
        let body = serde_json::to_vec(&payload).unwrap_or_else(|_| br#"{"ok":false}"#.to_vec());
        let mut response = request
            .into_response(200, Some("OK"), &[("content-type", "application/json")])
            .map_err(anyhow::Error::msg)?;
        response
            .write_all(&body)
            .map(|_| ())
            .map_err(anyhow::Error::msg)
    })?;

    server.fn_handler("/__matchbox/photo/*", Method::Get, move |request| {
        let path = request
            .uri()
            .split_once('?')
            .map(|(path, _)| path)
            .unwrap_or(request.uri());
        let Some(photo_id) = path
            .strip_prefix("/__matchbox/photo/")
            .and_then(|segment| segment.parse::<u64>().ok())
        else {
            return request
                .into_response(
                    404,
                    Some("Not Found"),
                    &[("content-type", "text/plain; charset=utf-8")],
                )
                .map_err(anyhow::Error::msg)?
                .write_all(b"Photo not found")
                .map(|_| ())
                .map_err(anyhow::Error::msg);
        };

        with_photo(photo_id, |capture| match capture {
            Some((format, bytes, _, _, _)) => {
                let content_type = if format.eq_ignore_ascii_case("jpeg") {
                    "image/jpeg"
                } else {
                    "application/octet-stream"
                };
                let mut response = request
                    .into_response(
                        200,
                        Some("OK"),
                        &[
                            ("content-type", content_type),
                            ("cache-control", "no-store, no-cache, must-revalidate"),
                            ("pragma", "no-cache"),
                        ],
                    )
                    .map_err(anyhow::Error::msg)?;
                response
                    .write_all(bytes)
                    .map(|_| ())
                    .map_err(anyhow::Error::msg)
            }
            None => request
                .into_response(
                    404,
                    Some("Not Found"),
                    &[("content-type", "text/plain; charset=utf-8")],
                )
                .map_err(anyhow::Error::msg)?
                .write_all(b"No captured image is available yet")
                .map(|_| ())
                .map_err(anyhow::Error::msg),
        })
        .map_err(anyhow::Error::msg)?
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
        ip, profile.web_port
    );
    *active_server()
        .lock()
        .map_err(|_| anyhow::anyhow!("HTTP server state lock poisoned"))? =
        Some(ActiveHttpServer(server));
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

pub fn load_executable_route_table() -> ExecutableRouteTable {
    let route_table = load_route_table();
    let application = route_table.application.and_then(|application| {
        let mut chunk: Chunk = match postcard::from_bytes(&application.bytecode) {
            Ok(chunk) => chunk,
            Err(error) => {
                println!(
                    "[matchbox] Failed to deserialize application bytecode for {}: {}",
                    application.source_path, error
                );
                return None;
            }
        };
        chunk.reconstruct_functions();
        Some(ExecutableApplicationEntry {
            source_path: application.source_path,
            chunk: Arc::new(chunk),
        })
    });
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

    ExecutableRouteTable {
        application,
        routes,
    }
}

pub fn run_application_start(route_table: &ExecutableRouteTable) -> Result<Esp32AppConfig> {
    let Some(application) = route_table.application.as_ref() else {
        return Ok(Esp32AppConfig::default());
    };

    println!(
        "[matchbox] Executing Application.onApplicationStart source={}",
        application.source_path
    );
    let mut vm = VM::new_with_bifs(crate::esp32_bifs::register_bifs(), HashMap::new());
    vm.interpret_chunk_borrowed(application.chunk.as_ref())
        .map_err(anyhow::Error::msg)?;
    let application = vm
        .construct_global_class("Application", Vec::new())
        .map_err(anyhow::Error::msg)?;
    vm.insert_empty_struct_global("application");

    // Populate application.esp32 with defaults from environment/profile
    populate_application_esp32(&mut vm);

    match vm.call_method_value(application, "onApplicationStart", Vec::new()) {
        Ok(_) => {
            // Read back the config (may have been modified by BoxLang)
            let config = read_esp32_config(&vm, application);

            let runtime = Arc::new(Mutex::new(ApplicationVm {
                ptr: Box::into_raw(Box::new(vm)) as usize,
            }));
            let _ = APPLICATION_VM.set(Arc::clone(&runtime));
            start_application_fiber_scheduler(runtime);
            Ok(config)
        }
        Err(error)
            if error.to_string().contains("Method ")
                && error.to_string().contains(" not found on instance") =>
        {
            Ok(Esp32AppConfig::default())
        }
        Err(error) => Err(anyhow::Error::msg(error)),
    }
}

/// Populate `application.esp32` with default configuration values
/// before `onApplicationStart()` runs. BoxLang code can then modify
/// these values during startup.
fn populate_application_esp32(vm: &mut VM) {
    let app_global = vm.get_global("application");
    let app_id = match app_global.and_then(|v| v.as_gc_id()) {
        Some(id) => id,
        None => return,
    };

    // Create the esp32 config struct
    let esp32_id = vm.struct_new();

    // wifi sub-struct
    let wifi_id = vm.struct_new();

    // Create strings separately to avoid mutable borrow conflicts
    let ssid_id = vm.string_new(
        option_env!("MATCHBOX_ESP32_WIFI_SSID")
            .unwrap_or("")
            .to_string(),
    );
    vm.struct_set(wifi_id, "ssid", BxValue::new_ptr(ssid_id));

    let password_id = vm.string_new(
        option_env!("MATCHBOX_ESP32_WIFI_PASSWORD")
            .unwrap_or("")
            .to_string(),
    );
    vm.struct_set(wifi_id, "password", BxValue::new_ptr(password_id));

    let hostname_id = vm.string_new(
        option_env!("MATCHBOX_ESP32_WIFI_HOSTNAME")
            .unwrap_or("matchbox-esp32")
            .to_string(),
    );
    vm.struct_set(wifi_id, "hostname", BxValue::new_ptr(hostname_id));

    vm.struct_set(esp32_id, "wifi", BxValue::new_ptr(wifi_id));

    // web sub-struct
    let web_id = vm.struct_new();
    vm.struct_set(
        web_id,
        "port",
        BxValue::new_number(
            option_env!("MATCHBOX_ESP32_WEB_PORT")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(80.0),
        ),
    );
    vm.struct_set(esp32_id, "web", BxValue::new_ptr(web_id));

    // Attach esp32 config to application struct
    vm.struct_set(app_id, "esp32", BxValue::new_ptr(esp32_id));
}

/// Read configuration from `application.esp32` after BoxLang
/// `onApplicationStart()` may have modified the default values.
fn read_esp32_config(vm: &VM, _application_instance: BxValue) -> Esp32AppConfig {
    let mut config = Esp32AppConfig::default();

    let app_val = match vm.get_global("application") {
        Some(v) => v,
        None => return config,
    };
    let app_id = match app_val.as_gc_id() {
        Some(id) => id,
        None => return config,
    };

    // Read application.esp32.wifi
    let esp32_val = vm.struct_get(app_id, "esp32");
    if let Some(esp32_id) = esp32_val.as_gc_id() {
        // wifi.ssid
        let ssid_val = vm.struct_get(esp32_id, "ssid");
        if !ssid_val.is_null() {
            config.wifi_ssid = vm.to_string(ssid_val);
        }
        let wifi_val = vm.struct_get(esp32_id, "wifi");
        if let Some(wifi_id) = wifi_val.as_gc_id() {
            let ssid_val = vm.struct_get(wifi_id, "ssid");
            if !ssid_val.is_null() {
                config.wifi_ssid = vm.to_string(ssid_val);
            }
            let password_val = vm.struct_get(wifi_id, "password");
            if !password_val.is_null() {
                config.wifi_password = vm.to_string(password_val);
            }
            let hostname_val = vm.struct_get(wifi_id, "hostname");
            if !hostname_val.is_null() {
                config.wifi_hostname = vm.to_string(hostname_val);
            }
        }

        // web.port
        let web_val = vm.struct_get(esp32_id, "web");
        if let Some(web_id) = web_val.as_gc_id() {
            let port_val = vm.struct_get(web_id, "port");
            if port_val.is_number() {
                let port = port_val.as_number() as u16;
                if port > 0 {
                    config.web_port = port;
                }
            }
        }
    }

    config
}

fn start_application_fiber_scheduler(vm: Arc<Mutex<ApplicationVm>>) {
    let has_fibers = vm
        .lock()
        .map(|mut vm| vm.with_vm(|vm| !vm.fibers.is_empty()))
        .unwrap_or(false);
    if !has_fibers {
        return;
    }

    let builder = thread::Builder::new()
        .name("matchbox-app-fibers".to_string())
        .stack_size(8192);
    match builder.spawn(move || {
        println!("[matchbox] Application fiber scheduler started");
        loop {
            match vm.lock() {
                Ok(mut vm) => {
                    let result = vm.with_vm(|vm| {
                        if vm.fibers.is_empty() {
                            return Ok(false);
                        }
                        vm.pump_until_blocked()?;
                        Ok::<bool, anyhow::Error>(true)
                    });
                    match result {
                        Ok(true) => {}
                        Ok(false) => {
                            println!("[matchbox] Application fiber scheduler stopped");
                            break;
                        }
                        Err(error) => {
                            println!("[matchbox] Application fiber scheduler error: {}", error);
                            break;
                        }
                    }
                }
                Err(_) => {
                    println!("[matchbox] Application fiber scheduler lock poisoned");
                    break;
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }) {
        Ok(_) => {}
        Err(error) => println!(
            "[matchbox] Application fiber scheduler failed to start: {}",
            error
        ),
    }
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
    shared_vm: &Mutex<ApplicationVm>,
) -> anyhow::Result<()> {
    let method = method_name(request.method());
    let request_uri = request.uri().to_string();
    let heap = heap_snapshot();
    println!(
        "[matchbox] HTTP {} {} free={} largest={}",
        method, request_uri, heap.free, heap.largest_internal_8bit_block
    );
    let request_path = request_uri
        .split_once('?')
        .map(|(path, _)| path.to_string())
        .unwrap_or(request_uri);
    let query_params = parse_query_params(request.uri());
    let form_fields = read_form_fields(&mut request);

    if let Some((route, params)) = match_route(route_table, method, &request_path) {
        let context =
            build_request_context(method, &request_path, params, query_params, form_fields);
        return match execute_embedded_route(route, &context, shared_vm) {
            Ok(RouteExecution::Html(body)) => request
                .into_response(
                    200,
                    Some("OK"),
                    &[("content-type", "text/html; charset=utf-8")],
                )
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
        .into_response(
            404,
            Some("Not Found"),
            &[("content-type", "text/plain; charset=utf-8")],
        )
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
    shared_vm: &Mutex<ApplicationVm>,
) -> anyhow::Result<RouteExecution> {
    println!(
        "[matchbox] Executing embedded route method={} path={} kind={} free={} largest={}",
        route.method,
        route.path,
        route.source_kind,
        heap_snapshot().free,
        heap_snapshot().largest_internal_8bit_block
    );
    // ESP32 memory model: each route is compiled once and stored as an
    // `Arc<Chunk>`. The immutable program data (bytecode, constants, line
    // table, filename and source text) is shared across requests; only a
    // lightweight per-request runtime cache is allocated. This prevents the
    // previous behavior of cloning the entire route chunk on every HTTP
    // request, which could exhaust the ESP32-S3 heap for routes with large
    // numeric constant tables (e.g. HID drawing instructions parsed from SVG).

    let mut shared_vm = shared_vm.lock().unwrap();
    shared_vm.with_vm(|vm| {
        // Collect garbage BEFORE execution to reduce heap fragmentation
        vm.collect_garbage_now();

        install_scope(vm, "url", &context.url);
        install_scope(vm, "form", &context.form);
        install_scope(vm, "request", &context.request);
        install_scope(vm, "cgi", &context.cgi);

        vm.begin_output_capture();
        let result = vm.interpret_chunk_borrowed_current_task(route.chunk.as_ref());
        let output = vm.end_output_capture().unwrap_or_default();
        println!(
            "[matchbox] Embedded route completed method={} path={} ok={} output_bytes={} free={} largest={}",
            route.method,
            route.path,
            result.is_ok(),
            output.len(),
            heap_snapshot().free,
            heap_snapshot().largest_internal_8bit_block
        );

        let execution = match result {
            Ok(result) if route.source_kind == "template" => Ok(RouteExecution::Html(output)),
            Ok(_) if !output.is_empty() => Ok(RouteExecution::Html(output)),
            Ok(result) => {
                let json = vm.bx_to_json(&result);
                Ok(RouteExecution::Json(serde_json::to_string(&json)?))
            }
            Err(error) => Err(anyhow::Error::msg(error)),
        };

        clear_request_scopes(vm);
        vm.collect_garbage_now();

        execution
    })
}

fn heap_snapshot() -> HeapSnapshot {
    unsafe {
        HeapSnapshot {
            free: esp_idf_sys::esp_get_free_heap_size() as usize,
            largest_internal_8bit_block: esp_idf_sys::heap_caps_get_largest_free_block(
                esp_idf_sys::MALLOC_CAP_INTERNAL | esp_idf_sys::MALLOC_CAP_8BIT,
            ) as usize,
            free_internal_8bit: esp_idf_sys::heap_caps_get_free_size(
                esp_idf_sys::MALLOC_CAP_INTERNAL | esp_idf_sys::MALLOC_CAP_8BIT,
            ) as usize,
        }
    }
}

fn partition_snapshots() -> Vec<PartitionSnapshot> {
    unsafe {
        let mut partitions = Vec::new();

        if let Some(snapshot) = find_partition_snapshot(
            esp_idf_sys::esp_partition_type_t_ESP_PARTITION_TYPE_APP,
            esp_idf_sys::esp_partition_subtype_t_ESP_PARTITION_SUBTYPE_APP_FACTORY as u32,
            "app",
        ) {
            partitions.push(snapshot);
        }

        if let Some(snapshot) = find_partition_snapshot(
            esp_idf_sys::esp_partition_type_t_ESP_PARTITION_TYPE_DATA,
            0x81,
            "storage",
        ) {
            partitions.push(snapshot);
        }

        partitions
    }
}

unsafe fn find_partition_snapshot(
    partition_type: esp_idf_sys::esp_partition_type_t,
    subtype: u32,
    type_label: &str,
) -> Option<PartitionSnapshot> {
    let partition =
        esp_idf_sys::esp_partition_find_first(partition_type, subtype, std::ptr::null());
    if partition.is_null() {
        return None;
    }

    let label = std::ffi::CStr::from_ptr((*partition).label.as_ptr())
        .to_string_lossy()
        .into_owned();

    Some(PartitionSnapshot {
        label,
        partition_type: type_label.to_string(),
        subtype,
        address: (*partition).address as usize,
        size: (*partition).size as usize,
    })
}

fn install_scope(vm: &mut VM, scope_name: &str, values: &HashMap<String, String>) {
    let scope_id = vm.struct_new();
    for (key, value) in values {
        let value_id = vm.string_new(value.clone());
        vm.struct_set(scope_id, key, BxValue::new_ptr(value_id));
    }
    vm.insert_global(scope_name.to_string(), BxValue::new_ptr(scope_id));
}

fn clear_request_scopes(vm: &mut VM) {
    for scope_name in ["url", "form", "request", "cgi"] {
        vm.insert_global(scope_name.to_string(), BxValue::new_null());
    }
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
    let content_type = request
        .header("content-type")
        .unwrap_or_default()
        .to_ascii_lowercase();
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
