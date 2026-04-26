mod app_server;
pub mod webroot_core;
mod websocket;
#[cfg(test)]
mod websocket_tests;

use axum::{
    Router,
    extract::{Form, Path as AxumPath, Query, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use clap::Parser as ClapParser;
use matchbox_compiler::{compiler::Compiler, parser};
use matchbox_vm::types::{BxVM, BxValue};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;
use tokio::fs;
use webroot_core::{
    EmbeddedAssetPackage, EmbeddedAssetStore, FileSystemAssetStore, WebrootConfig, WebrootEngine,
    WebrootRequest, WebrootResponse,
};
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

fn default_rewrites() -> bool {
    false
}
fn default_rewrite_file_name() -> String {
    "index.bxm".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rewrites: default_rewrites(),
            rewrite_file_name: default_rewrite_file_name(),
            websocket: None,
        }
    }
}

pub struct WasiHttpWebrootPackage {
    pub engine: WebrootEngine<EmbeddedAssetStore>,
    pub config: WebrootConfig,
    pub assets: EmbeddedAssetPackage,
    pub warnings: Vec<String>,
}

pub fn prepare_wasi_http_webroot(
    webroot: impl AsRef<Path>,
    config_path: Option<&Path>,
) -> anyhow::Result<WasiHttpWebrootPackage> {
    let webroot = webroot.as_ref();
    let config_path = config_path
        .map(PathBuf::from)
        .unwrap_or_else(|| webroot.join("boxlang.json"));
    let config = read_webroot_config_sync(&config_path)?;
    let webroot_config = WebrootConfig {
        rewrites: config.rewrites,
        rewrite_file_name: config.rewrite_file_name.clone(),
    };
    let mut warnings = Vec::new();
    if config.websocket.is_some() {
        warnings.push(
            "WASI HTTP v1 does not support websocket configuration; websocket settings were ignored."
                .to_string(),
        );
    }

    validate_wasi_http_webroot(webroot)?;
    let store = EmbeddedAssetStore::from_directory(webroot)?;
    let assets = store.clone().into_package();
    Ok(WasiHttpWebrootPackage {
        engine: WebrootEngine::new(store, webroot_config.clone()),
        config: webroot_config,
        assets,
        warnings,
    })
}

fn read_webroot_config_sync(config_path: &Path) -> anyhow::Result<Config> {
    if !config_path.exists() {
        return Ok(Config::default());
    }

    let content = std::fs::read_to_string(config_path)?;
    let mut config: Config = serde_json::from_str(&content).unwrap_or_default();
    if config.rewrite_file_name.contains("..") || config.rewrite_file_name.starts_with('/') {
        config.rewrite_file_name = default_rewrite_file_name();
    }
    Ok(config)
}

fn validate_wasi_http_webroot(webroot: &Path) -> anyhow::Result<()> {
    let native_dir = webroot.join("native");
    if native_dir.is_dir() {
        anyhow::bail!(
            "WASI HTTP webroot builds do not support native fusion directories: {}",
            native_dir.display()
        );
    }
    validate_wasi_http_native_module_dirs(webroot)?;
    validate_wasi_http_sources(webroot, webroot)?;
    Ok(())
}

fn validate_wasi_http_native_module_dirs(webroot: &Path) -> anyhow::Result<()> {
    let modules_dir = webroot.join("boxlang_modules");
    if !modules_dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&modules_dir)? {
        let entry = entry?;
        let native_dir = entry.path().join("native");
        if native_dir.is_dir() {
            anyhow::bail!(
                "WASI HTTP webroot builds do not support native module directories: {}",
                native_dir.display()
            );
        }
    }

    Ok(())
}

fn validate_wasi_http_sources(webroot: &Path, current: &Path) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            validate_wasi_http_sources(webroot, &path)?;
            continue;
        }

        if !is_boxlang_source_path(&path) {
            continue;
        }

        let source = std::fs::read_to_string(&path)?;
        let lower = source.to_ascii_lowercase();
        let unsupported = if lower.contains("java:") {
            Some("java:")
        } else if lower.contains("rust:") {
            Some("rust:")
        } else {
            None
        };

        if let Some(feature) = unsupported {
            let relative = path.strip_prefix(webroot).unwrap_or(&path);
            anyhow::bail!(
                "WASI HTTP webroot builds do not support native interop reference '{}' in {}",
                feature,
                relative.display()
            );
        }
    }
    Ok(())
}

fn is_boxlang_source_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("bx") | Some("bxm") | Some("bxs")
    )
}

struct AppState {
    engine: WebrootEngine<FileSystemAssetStore>,
    websocket: Option<Arc<WebSocketRuntimeHandle>>,
}

pub fn find_file_case_insensitive(
    parent: &std::path::Path,
    target_name: &str,
) -> Option<std::path::PathBuf> {
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

    let webroot = PathBuf::from(&args.webroot)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&args.webroot));

    let config_path = args
        .config
        .map(PathBuf::from)
        .unwrap_or_else(|| webroot.join("boxlang.json"));
    let config = if config_path.exists() {
        match fs::read_to_string(&config_path).await {
            Ok(content) => {
                let mut c: Config = serde_json::from_str(&content).unwrap_or_default();
                // Sanitize rewrite_file_name
                if c.rewrite_file_name.contains("..") || c.rewrite_file_name.starts_with('/') {
                    c.rewrite_file_name = default_rewrite_file_name();
                }
                c
            }
            Err(_) => Config::default(),
        }
    } else {
        Config::default()
    };

    let websocket = if let Some(ws_config) = &config.websocket {
        if let Some(ws_script_path) = find_file_case_insensitive(&webroot, &ws_config.handler) {
            match std::fs::read_to_string(&ws_script_path) {
                Ok(source) => {
                    let ast = parser::parse(&source, Some(&ws_config.handler))
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to parse {}: {}", ws_config.handler, e)
                        })
                        .ok();
                    let chunk = ast.and_then(|ast| {
                        let mut compiler = Compiler::new(&ws_config.handler);
                        compiler
                            .compile(&ast, &source)
                            .map_err(|e| {
                                eprintln!("Failed to compile {}: {}", ws_config.handler, e)
                            })
                            .ok()
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
                                if let Err(err) =
                                    websocket_runtime_main(chunk, config_clone, commands_rx, None)
                                {
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
        engine: WebrootEngine::new(
            FileSystemAssetStore::new(webroot.clone()),
            WebrootConfig {
                rewrites: config.rewrites,
                rewrite_file_name: config.rewrite_file_name.clone(),
            },
        ),
        websocket,
    });

    let mut router = Router::new()
        .route("/", get(handler).post(handler))
        .route("/*path", get(handler).post(handler));

    if let Some(runtime) = &state.websocket {
        router = router.route(
            &runtime.uri,
            get(websocket_handler).with_state(runtime.clone()),
        );
    }

    let app = router.with_state(state.clone());

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse().unwrap();
    println!("MatchBox Server listening on http://{}", addr);
    println!("Web root: {}", webroot.display());
    println!(
        "Rewrites: {}",
        if config.rewrites {
            "Enabled"
        } else {
            "Disabled"
        }
    );

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handler(
    State(state): State<Arc<AppState>>,
    method: Method,
    path: Option<AxumPath<String>>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    form_data: Option<Form<HashMap<String, String>>>,
) -> impl IntoResponse {
    let path_str = path.map(|AxumPath(p)| p).unwrap_or_default();
    let path = if path_str.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", path_str)
    };
    let mut header_map = HashMap::new();
    for (name, value) in headers.iter() {
        if let Ok(value) = value.to_str() {
            header_map.insert(name.as_str().to_lowercase(), value.to_string());
        }
    }

    let form = form_data.map(|Form(data)| data).unwrap_or_default();
    match state.engine.handle(WebrootRequest {
        method: method.to_string(),
        path,
        query: params,
        form,
        headers: header_map,
        body: Vec::new(),
    }) {
        Ok(response) => webroot_response_to_axum(response),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", err)).into_response(),
    }
}

fn webroot_response_to_axum(response: WebrootResponse) -> axum::response::Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut axum_response = (status, response.body).into_response();
    for (name, value) in response.headers {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::from_bytes(name.as_bytes()),
            header::HeaderValue::from_str(&value),
        ) {
            axum_response.headers_mut().insert(name, value);
        }
    }
    axum_response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_state(webroot: PathBuf) -> Arc<AppState> {
        Arc::new(AppState {
            engine: WebrootEngine::new(
                FileSystemAssetStore::new(webroot),
                WebrootConfig::default(),
            ),
            websocket: None,
        })
    }

    #[tokio::test]
    async fn test_not_found() {
        let temp = tempfile::tempdir().unwrap();
        let state = setup_test_state(temp.path().to_path_buf());

        let res = handler(
            State(state),
            Method::GET,
            Some(AxumPath("non-existent.txt".to_string())),
            Query(HashMap::new()),
            HeaderMap::new(),
            None,
        )
        .await
        .into_response();

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
            Method::GET,
            None, // root path
            Query(HashMap::new()),
            HeaderMap::new(),
            None,
        )
        .await
        .into_response();

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
            Method::GET,
            Some(AxumPath(".env".to_string())),
            Query(HashMap::new()),
            HeaderMap::new(),
            None,
        )
        .await
        .into_response();

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
            Method::GET,
            Some(AxumPath("../outside.txt".to_string())),
            Query(HashMap::new()),
            HeaderMap::new(),
            None,
        )
        .await
        .into_response();

        // Should be 403 Forbidden
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_url_rewrites() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().to_path_buf().canonicalize().unwrap();
        std::fs::write(webroot.join("index.bxm"), "<h1>Index</h1>").unwrap();

        let state = Arc::new(AppState {
            engine: WebrootEngine::new(
                FileSystemAssetStore::new(webroot),
                WebrootConfig {
                    rewrites: true,
                    rewrite_file_name: "index.bxm".to_string(),
                },
            ),
            websocket: None,
        });

        let res = handler(
            State(state),
            Method::GET,
            Some(AxumPath("non-existent".to_string())),
            Query(HashMap::new()),
            HeaderMap::new(),
            None,
        )
        .await
        .into_response();

        // Should be 200 OK because of rewrite
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[test]
    fn test_prepare_wasi_http_webroot_warns_and_ignores_websocket_config() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().to_path_buf().canonicalize().unwrap();
        std::fs::write(webroot.join("index.bxm"), "<bx:output>wasi</bx:output>").unwrap();
        std::fs::write(
            webroot.join("boxlang.json"),
            r#"{
                "rewrites": true,
                "rewriteFileName": "index.bxm",
                "websocket": {
                    "uri": "/ws",
                    "handler": "WebSocket.bx",
                    "listenerClass": "EchoListener",
                    "listenerState": {}
                }
            }"#,
        )
        .unwrap();

        let package = prepare_wasi_http_webroot(&webroot, None).unwrap();
        let response = package
            .engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/missing".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert!(package.config.rewrites);
        assert_eq!(package.warnings.len(), 1);
        assert!(package.warnings[0].contains("websocket"));
        assert_eq!(String::from_utf8(response.body).unwrap(), "wasi");
    }

    #[test]
    fn test_prepare_wasi_http_webroot_rejects_native_directory() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().to_path_buf().canonicalize().unwrap();
        std::fs::write(webroot.join("index.bxm"), "<bx:output>wasi</bx:output>").unwrap();
        std::fs::create_dir(webroot.join("native")).unwrap();

        let err = match prepare_wasi_http_webroot(&webroot, None) {
            Ok(_) => panic!("expected native directory to be rejected"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("native"));
        assert!(err.to_string().contains("WASI HTTP"));
    }

    #[test]
    fn test_prepare_wasi_http_webroot_rejects_native_module_directory() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().to_path_buf().canonicalize().unwrap();
        std::fs::write(webroot.join("index.bxm"), "<bx:output>wasi</bx:output>").unwrap();
        let module_native = webroot
            .join("boxlang_modules")
            .join("crypto")
            .join("native");
        std::fs::create_dir_all(&module_native).unwrap();

        let err = match prepare_wasi_http_webroot(&webroot, None) {
            Ok(_) => panic!("expected native module directory to be rejected"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("native module"));
        assert!(err.to_string().contains("WASI HTTP"));
    }

    #[test]
    fn test_prepare_wasi_http_webroot_rejects_java_interop_imports() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().to_path_buf().canonicalize().unwrap();
        std::fs::write(
            webroot.join("index.bxs"),
            "import java:java.util.ArrayList as List;\nwriteOutput(\"bad\");",
        )
        .unwrap();

        let err = match prepare_wasi_http_webroot(&webroot, None) {
            Ok(_) => panic!("expected Java interop import to be rejected"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("java:"));
        assert!(err.to_string().contains("WASI HTTP"));
        assert!(err.to_string().contains("index.bxs"));
    }

    #[test]
    fn test_prepare_wasi_http_webroot_rejects_rust_interop_imports() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().to_path_buf().canonicalize().unwrap();
        std::fs::write(
            webroot.join("index.bxs"),
            "import rust:crypto.Vault;\nwriteOutput(\"bad\");",
        )
        .unwrap();

        let err = match prepare_wasi_http_webroot(&webroot, None) {
            Ok(_) => panic!("expected Rust interop import to be rejected"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("rust:"));
        assert!(err.to_string().contains("WASI HTTP"));
        assert!(err.to_string().contains("index.bxs"));
    }
}
