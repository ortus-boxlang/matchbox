mod app_server;
mod websocket;
#[cfg(test)]
mod websocket_tests;

use axum::{
    extract::{Query, State, Path as AxumPath, Form},
    http::{header, StatusCode, HeaderMap, Method},
    response::{IntoResponse},
    routing::get,
    Router,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use matchbox_vm::vm::VM;
use matchbox_vm::types::{BxVM, BxValue};
use matchbox_compiler::{parser, compiler::Compiler};
use clap::Parser as ClapParser;
use tokio::fs;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use websocket::{WebSocketRuntimeHandle, websocket_handler, websocket_runtime_main};

#[derive(Clone, Debug)]
pub struct RequestData {
    pub method: String,
    pub path: String,
    pub matched_route: Option<String>,
    pub route_params: HashMap<String, String>,
    pub raw_query: Option<String>,
    pub query: HashMap<String, String>,
    pub cookies: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub full_url: String,
}

pub fn request_data_from_parts(
    method: Method,
    path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: Vec<u8>,
) -> RequestData {
    let mut header_map = HashMap::new();
    for (name, value) in headers.iter() {
        if let Ok(value) = value.to_str() {
            header_map.insert(name.as_str().to_lowercase(), value.to_string());
        }
    }
    let cookie_map = header_map
        .get("cookie")
        .map(|raw| parse_cookie_header(raw))
        .unwrap_or_default();
    let query_map = query
        .map(|raw| {
            url::form_urlencoded::parse(raw.as_bytes())
                .into_owned()
                .collect::<HashMap<String, String>>()
        })
        .unwrap_or_default();
    let host = header_map
        .get("host")
        .cloned()
        .unwrap_or_else(|| "localhost".to_string());
    let full_url = if let Some(raw) = query {
        format!("http://{}{}?{}", host, path, raw)
    } else {
        format!("http://{}{}", host, path)
    };

    RequestData {
        method: method.to_string(),
        path: path.to_string(),
        matched_route: None,
        route_params: HashMap::new(),
        raw_query: query.map(|s| s.to_string()),
        query: query_map,
        cookies: cookie_map,
        headers: header_map,
        body,
        full_url,
    }
}

pub fn parse_cookie_header(raw: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for part in raw.split(';') {
        let mut kv = part.splitn(2, '=');
        if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
            let key = k.trim().to_string();
            let val = v.trim().to_string();
            map.insert(key.clone(), val.clone());
            map.insert(key.to_lowercase(), val);
        }
    }
    map
}

pub fn bx_to_json(vm: &dyn BxVM, value: BxValue) -> Result<JsonValue, String> {
    if value.is_null() {
        return Ok(JsonValue::Null);
    }
    if value.is_bool() {
        return Ok(JsonValue::Bool(value.as_bool()));
    }
    if value.is_int() {
        return Ok(JsonValue::from(value.as_int()));
    }
    if value.is_number() {
        return Ok(JsonValue::from(value.as_number()));
    }
    if vm.is_string_value(value) {
        return Ok(JsonValue::String(vm.to_string(value)));
    }
    if vm.is_bytes(value) {
        return Ok(JsonValue::Array(
            vm.to_bytes(value)?
                .into_iter()
                .map(JsonValue::from)
                .collect(),
        ));
    }
    if vm.is_array_value(value) {
        let id = value.as_gc_id().unwrap();
        let mut items = Vec::new();
        for index in 0..vm.array_len(id) {
            items.push(bx_to_json(vm, vm.array_get(id, index))?);
        }
        return Ok(JsonValue::Array(items));
    }
    if vm.is_struct_value(value) {
        let id = value.as_gc_id().unwrap();
        let mut object = serde_json::Map::new();
        for key in vm.struct_key_array(id) {
            object.insert(key.clone(), bx_to_json(vm, vm.struct_get(id, &key))?);
        }
        return Ok(JsonValue::Object(object));
    }
    Ok(JsonValue::String(vm.to_string(value)))
}

pub fn json_to_bx(vm: &mut dyn BxVM, value: &JsonValue) -> Result<BxValue, String> {
    match value {
        JsonValue::Null => Ok(BxValue::new_null()),
        JsonValue::Bool(val) => Ok(BxValue::new_bool(*val)),
        JsonValue::Number(val) => {
            if let Some(i) = val.as_i64() {
                Ok(BxValue::new_number(i as f64))
            } else {
                Ok(BxValue::new_number(
                    val.as_f64()
                        .ok_or_else(|| "Unsupported JSON number".to_string())?,
                ))
            }
        }
        JsonValue::String(val) => Ok(BxValue::new_ptr(vm.string_new(val.clone()))),
        JsonValue::Array(values) => {
            let id = vm.array_new();
            for value in values {
                let bx = json_to_bx(vm, value)?;
                vm.array_push(id, bx);
            }
            Ok(BxValue::new_ptr(id))
        }
        JsonValue::Object(values) => {
            let id = vm.struct_new();
            for (key, value) in values {
                let bx = json_to_bx(vm, value)?;
                vm.struct_set(id, key, bx);
            }
            Ok(BxValue::new_ptr(id))
        }
    }
}

#[derive(ClapParser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 8080)]
    pub port: u16,

    /// Host to bind to
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    pub host: String,

    /// Web root directory
    #[arg(short, long, default_value = ".")]
    pub webroot: String,

    /// Config file path (defaults to boxlang.json in webroot)
    #[arg(short, long)]
    pub config: Option<String>,

    /// BoxLang app script to run in routed app-server mode
    #[arg(long)]
    pub app: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "default_rewrites")]
    pub rewrites: bool,
    #[serde(default = "default_rewrite_file_name")]
    pub rewrite_file_name: String,
    pub websocket: Option<websocket::WebSocketConfig>,
}

fn default_rewrites() -> bool { false }
fn default_rewrite_file_name() -> String { "index.bxm".to_string() }

impl Default for Config {
    fn default() -> Self {
        Self {
            rewrites: default_rewrites(),
            rewrite_file_name: default_rewrite_file_name(),
            websocket: None,
        }
    }
}

struct AppState {
    webroot: PathBuf,
    config: Config,
    sessions: Mutex<HashMap<String, HashMap<String, String>>>,
    websocket: Option<Arc<WebSocketRuntimeHandle>>,
}

pub fn find_file_case_insensitive(parent: &std::path::Path, target_name: &str) -> Option<std::path::PathBuf> {
    if let Ok(entries) = std::fs::read_dir(parent) {
        let target_lower = target_name.to_lowercase();
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.to_lowercase() == target_lower {
                    return Some(entry.path());
                }
            }
        }
    }
    None
}

pub async fn run_server(args: Args) {
    tracing_subscriber::fmt::init();

    if let Some(app_path) = &args.app {
        if let Err(err) = app_server::run_script_server(Path::new(app_path)).await {
            eprintln!("Error: {}", err);
        }
        return;
    }

    let webroot = PathBuf::from(&args.webroot).canonicalize().unwrap_or_else(|_| PathBuf::from(&args.webroot));
    
    let config_path = args.config.map(PathBuf::from).unwrap_or_else(|| webroot.join("boxlang.json"));
    let config = if config_path.exists() {
        match fs::read_to_string(&config_path).await {
            Ok(content) => {
                let mut c: Config = serde_json::from_str(&content).unwrap_or_default();
                // Sanitize rewrite_file_name
                if c.rewrite_file_name.contains("..") || c.rewrite_file_name.starts_with('/') {
                    c.rewrite_file_name = default_rewrite_file_name();
                }
                c
            },
            Err(_) => Config::default(),
        }
    } else {
        Config::default()
    };

    let websocket = if let Some(ws_config) = &config.websocket {
        if let Some(ws_script_path) = find_file_case_insensitive(&webroot, &ws_config.handler) {
            match std::fs::read_to_string(&ws_script_path) {
                Ok(source) => {
                    let ast = parser::parse(&source).map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", ws_config.handler, e)).ok();
                    let chunk = ast.and_then(|ast| {
                        let compiler = Compiler::new(&ws_config.handler);
                        compiler.compile(&ast, &source).map_err(|e| eprintln!("Failed to compile {}: {}", ws_config.handler, e)).ok()
                    });

                    if let Some(chunk) = chunk {
                        let (commands_tx, commands_rx) = mpsc::channel();
                        let runtime = Arc::new(WebSocketRuntimeHandle {
                            uri: ws_config.uri.clone(),
                            commands: commands_tx,
                        });
                        let config_clone = ws_config.clone();
                        thread::Builder::new()
                            .name("matchbox-websocket-runtime".to_string())
                            .spawn(move || {
                                if let Err(err) = websocket_runtime_main(chunk, config_clone, commands_rx, None) {
                                    eprintln!("WebSocket runtime stopped: {}", err);
                                }
                            })
                            .expect("Failed to spawn websocket runtime thread");
                        Some(runtime)
                    } else {
                        None
                    }
                }
                Err(e) => {
                    eprintln!("Failed to read {}: {}", ws_config.handler, e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let state = Arc::new(AppState {
        webroot: webroot.clone(),
        config,
        sessions: Mutex::new(HashMap::new()),
        websocket,
    });

    let mut router = Router::new()
        .route("/", get(handler).post(handler))
        .route("/*path", get(handler).post(handler));

    if let Some(runtime) = &state.websocket {
        router = router.route(&runtime.uri, get(websocket_handler).with_state(runtime.clone()));
    }

    let app = router.with_state(state.clone());

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse().unwrap();
    println!("MatchBox Server listening on http://{}", addr);
    println!("Web root: {}", webroot.display());
    println!("Rewrites: {}", if state.config.rewrites { "Enabled" } else { "Disabled" });

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handler(
    State(state): State<Arc<AppState>>,
    path: Option<AxumPath<String>>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    form_data: Option<Form<HashMap<String, String>>>,
) -> impl IntoResponse {
    let path_str = path.map(|AxumPath(p)| p).unwrap_or_default();
    
    // Security: Block hidden files or directories starting with a dot
    if path_str.split('/').any(|s| s.starts_with('.')) {
        return (StatusCode::FORBIDDEN, "Forbidden: Access to hidden files is denied.").into_response();
    }

    let mut full_path = state.webroot.join(path_str.trim_start_matches('/'));
    
    // Directory Traversal Protection: Ensure the path is within the webroot
    if let Ok(abs_path) = full_path.canonicalize() {
        if !abs_path.starts_with(&state.webroot) {
            return (StatusCode::FORBIDDEN, "Forbidden: Directory traversal attempt.").into_response();
        }
        full_path = abs_path;
    } else if !state.config.rewrites {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    // Handle directory index
    if full_path.is_dir() {
        let index_files = ["index.bxm", "index.bxs"];
        for index in index_files {
            let index_path = full_path.join(index);
            if index_path.exists() {
                full_path = index_path;
                break;
            }
        }
    }

    // URL Rewrites logic
    if !full_path.exists() {
        if state.config.rewrites {
            let rewrite_path = state.webroot.join(&state.config.rewrite_file_name);
            if rewrite_path.exists() {
                full_path = rewrite_path;
            } else {
                return (StatusCode::NOT_FOUND, "Not Found").into_response();
            }
        } else {
            return (StatusCode::NOT_FOUND, "Not Found").into_response();
        }
    }

    let ext = full_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    
    if ext == "bxm" || ext == "bxs" {
        let mut form_params = HashMap::new();
        if let Some(Form(data)) = form_data {
            form_params = data;
        }
        match execute_template(state, &full_path, &params, &form_params, headers.clone()).await {
            Ok((html, session_id)) => {
                let mut res = (
                    [(header::CONTENT_TYPE, "text/html")],
                    html
                ).into_response();
                
                let cookie = format!("MBX_SESSION_ID={}; Path=/; HttpOnly", session_id);
                res.headers_mut().insert(header::SET_COOKIE, cookie.parse().unwrap());
                
                res
            },
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)).into_response(),
        }
    } else {
        match fs::read(&full_path).await {
            Ok(bytes) => {
                let mime = mime_guess::from_path(&full_path).first_or_octet_stream();
                (
                    [(header::CONTENT_TYPE, mime.to_string())],
                    bytes
                ).into_response()
            }
            Err(_) => (StatusCode::NOT_FOUND, "Not Found").into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn setup_test_state(webroot: PathBuf) -> Arc<AppState> {
        Arc::new(AppState {
            webroot,
            config: Config::default(),
            sessions: Mutex::new(HashMap::new()),
            websocket: None,
        })
    }

    #[tokio::test]
    async fn test_not_found() {
        let temp = tempfile::tempdir().unwrap();
        let state = setup_test_state(temp.path().to_path_buf());
        
        let res = handler(
            State(state),
            Some(AxumPath("non-existent.txt".to_string())),
            Query(HashMap::new()),
            HeaderMap::new(),
            None,
        ).await.into_response();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_directory_index() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().to_path_buf();
        // Canonicalize the temp path to match what run_server does
        let webroot = webroot.canonicalize().unwrap();
        std::fs::write(webroot.join("index.bxm"), "<h1>Index</h1>").unwrap();
        
        let state = setup_test_state(webroot);
        
        let res = handler(
            State(state),
            None, // root path
            Query(HashMap::new()),
            HeaderMap::new(),
            None,
        ).await.into_response();

        // This SHOULD be OK (Green)
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_hidden_files() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().to_path_buf().canonicalize().unwrap();
        std::fs::write(webroot.join(".env"), "SECRET=123").unwrap();
        
        let state = setup_test_state(webroot);
        
        let res = handler(
            State(state),
            Some(AxumPath(".env".to_string())),
            Query(HashMap::new()),
            HeaderMap::new(),
            None,
        ).await.into_response();

        // Should be 403 Forbidden
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_directory_traversal() {
        let temp_root = tempfile::tempdir().unwrap();
        let webroot = temp_root.path().join("webroot");
        std::fs::create_dir(&webroot).unwrap();
        let webroot = webroot.canonicalize().unwrap();
        
        // File outside webroot
        std::fs::write(temp_root.path().join("outside.txt"), "outside").unwrap();
        
        let state = setup_test_state(webroot);
        
        let res = handler(
            State(state),
            Some(AxumPath("../outside.txt".to_string())),
            Query(HashMap::new()),
            HeaderMap::new(),
            None,
        ).await.into_response();

        // Should be 403 Forbidden
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_url_rewrites() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().to_path_buf().canonicalize().unwrap();
        std::fs::write(webroot.join("index.bxm"), "<h1>Index</h1>").unwrap();
        
        let state = Arc::new(AppState {
            webroot,
            config: Config {
                rewrites: true,
                rewrite_file_name: "index.bxm".to_string(),
                websocket: None,
            },
            sessions: Mutex::new(HashMap::new()),
            websocket: None,
        });
        
        let res = handler(
            State(state),
            Some(AxumPath("non-existent".to_string())),
            Query(HashMap::new()),
            HeaderMap::new(),
            None,
        ).await.into_response();

        // Should be 200 OK because of rewrite
        assert_eq!(res.status(), StatusCode::OK);
    }
}

async fn execute_template(
    state: Arc<AppState>,
    path: &Path,
    url_params: &HashMap<String, String>,
    form_params: &HashMap<String, String>,
    headers: HeaderMap,
) -> anyhow::Result<(String, String)> {
    let source = fs::read_to_string(path).await?;
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    
    let ast = if ext == "bxm" {
        parser::parse_bxm(&source)?
    } else {
        parser::parse(&source)?
    };

    let filename = path.to_string_lossy();
    let compiler = Compiler::new(&filename);
    let chunk = compiler.compile(&ast, &source)?;

    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let mut session_id = None;
    if let Some(cookie_header) = headers.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let mut parts = cookie.splitn(2, '=');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    if k.trim() == "MBX_SESSION_ID" {
                        session_id = Some(v.trim().to_string());
                    }
                }
            }
        }
    }
    
    let sid = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let scopes = setup_scopes(&mut vm, &state, &sid, url_params, form_params, headers)?;

    vm.interpret(chunk)?;

    persist_session(&mut vm, &state, &sid, scopes.session_id)?;

    Ok((vm.output_buffer.unwrap_or_default(), sid))
}

struct RequestScopes {
    session_id: usize,
}

fn setup_scopes(
    vm: &mut VM,
    state: &AppState,
    session_id: &str,
    url_params: &HashMap<String, String>,
    form_params: &HashMap<String, String>,
    headers: HeaderMap,
) -> anyhow::Result<RequestScopes> {
    // URL Scope
    let url_scope_id = vm.struct_new();
    for (k, v) in url_params {
        let val_ptr = vm.string_new(v.clone());
        vm.struct_set(url_scope_id, k, matchbox_vm::types::BxValue::new_ptr(val_ptr));
    }
    vm.insert_global("url".to_string(), matchbox_vm::types::BxValue::new_ptr(url_scope_id));

    // FORM Scope
    let form_scope_id = vm.struct_new();
    for (k, v) in form_params {
        let val_ptr = vm.string_new(v.clone());
        vm.struct_set(form_scope_id, k, matchbox_vm::types::BxValue::new_ptr(val_ptr));
    }
    vm.insert_global("form".to_string(), matchbox_vm::types::BxValue::new_ptr(form_scope_id));

    // COOKIE Scope
    let cookie_scope_id = vm.struct_new();
    // Inject session ID first so it's always there
    let sid_ptr = vm.string_new(session_id.to_string());
    vm.struct_set(cookie_scope_id, "MBX_SESSION_ID", matchbox_vm::types::BxValue::new_ptr(sid_ptr));
    
    if let Some(cookie_header) = headers.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let mut parts = cookie.splitn(2, '=');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    let val_ptr = vm.string_new(v.trim().to_string());
                    vm.struct_set(cookie_scope_id, k.trim(), matchbox_vm::types::BxValue::new_ptr(val_ptr));
                }
            }
        }
    }
    vm.insert_global("cookie".to_string(), matchbox_vm::types::BxValue::new_ptr(cookie_scope_id));

    // SESSION Scope
    let session_scope_id = vm.struct_new();
    {
        let sessions = state.sessions.lock().unwrap();
        if let Some(data) = sessions.get(session_id) {
            for (k, v) in data {
                let val_ptr = vm.string_new(v.clone());
                vm.struct_set(session_scope_id, k, matchbox_vm::types::BxValue::new_ptr(val_ptr));
            }
        }
    }
    vm.insert_global("session".to_string(), matchbox_vm::types::BxValue::new_ptr(session_scope_id));

    // CGI Scope
    let cgi_scope_id = vm.struct_new();
    vm.struct_set(cgi_scope_id, "server_port", matchbox_vm::types::BxValue::new_int(8080));
    vm.insert_global("cgi".to_string(), matchbox_vm::types::BxValue::new_ptr(cgi_scope_id));

    Ok(RequestScopes { session_id: session_scope_id })
}

fn persist_session(
    vm: &mut VM,
    state: &AppState,
    session_id: &str,
    scope_id: usize,
) -> anyhow::Result<()> {
    let mut data = HashMap::new();
    for key in vm.struct_key_array(scope_id) {
        let val = vm.struct_get(scope_id, &key);
        data.insert(key, vm.to_string(val));
    }
    let mut sessions = state.sessions.lock().unwrap();
    sessions.insert(session_id.to_string(), data);
    Ok(())
}
