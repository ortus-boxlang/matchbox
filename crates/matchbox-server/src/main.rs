use axum::{
    extract::{Query, State, Path as AxumPath, Form},
    http::{header, StatusCode, HeaderMap},
    response::{IntoResponse},
    routing::get,
    Router,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use matchbox_vm::vm::VM;
use matchbox_vm::types::BxVM;
use matchbox_compiler::{parser, compiler::Compiler};
use clap::Parser as ClapParser;
use tokio::fs;

#[derive(ClapParser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Host to bind to
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,

    /// Web root directory
    #[arg(short, long, default_value = ".")]
    webroot: String,
}

struct AppState {
    webroot: PathBuf,
    sessions: Mutex<HashMap<String, HashMap<String, String>>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let webroot = PathBuf::from(&args.webroot).canonicalize().unwrap_or_else(|_| PathBuf::from(&args.webroot));
    
    let state = Arc::new(AppState {
        webroot: webroot.clone(),
        sessions: Mutex::new(HashMap::new()),
    });

    let app = Router::new()
        .route("/", get(handler).post(handler))
        .route("/*path", get(handler).post(handler))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse().unwrap();
    println!("MatchBox Server listening on http://{}", addr);
    println!("Web root: {}", webroot.display());

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
    let mut full_path = state.webroot.join(path_str.trim_start_matches('/'));
    
    if full_path.is_dir() {
        let bxm = full_path.join("index.bxm");
        if bxm.exists() {
            full_path = bxm;
        } else {
            let bxs = full_path.join("index.bxs");
            if bxs.exists() {
                full_path = bxs;
            }
        }
    }

    if !full_path.exists() {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
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

    persist_session(&vm, &state, &sid, scopes.session_id)?;

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
    vm: &VM,
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
