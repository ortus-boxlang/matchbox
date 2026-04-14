use axum::{
    body::{to_bytes, Body},
    extract::{
        State,
    },
    http::{header, Request, Response, StatusCode},
    response::IntoResponse,
    routing::{any, get},
    Router,
};
use hmac::{Hmac, Mac};
use matchbox_compiler::{compiler::Compiler, parser};
use matchbox_vm::{
    vm::{chunk::Chunk, VM},
    types::{BxNativeObject, BxVM, BxValue},
};
use serde_json::Value as JsonValue;
use sha2::Sha256;
use std::{
    cell::RefCell,
    collections::HashMap,
    fs as stdfs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc::{self},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::fs;
use url::form_urlencoded;
use uuid::Uuid;
use crate::{RequestData, request_data_from_parts, bx_to_json, json_to_bx};
use crate::websocket::{
    WebSocketConfig, WebSocketRuntimeHandle,
    websocket_handler, websocket_runtime_main
};

const SESSION_COOKIE_NAME: &str = "MBX_SESSION_ID";
const DEFAULT_WEBHOOK_REPLAY_TTL_SECONDS: i64 = 3600;

#[derive(Clone)]
pub struct CompiledScriptApp {
    path: PathBuf,
    chunk: Chunk,
    web_imported: bool,
    session_store: Arc<Mutex<HashMap<String, JsonValue>>>,
    webhook_replay_store: Arc<Mutex<HashMap<String, i64>>>,
}

#[derive(Clone)]
pub struct ScriptServerState {
    compiled: Arc<CompiledScriptApp>,
    websocket: Option<Arc<WebSocketRuntimeHandle>>,
}

#[derive(Clone, Debug)]
pub struct ListenConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ListenConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppDefinition {
    middleware: Vec<BxValue>,
    routes: Vec<RouteDefinition>,
    websocket: Option<WebSocketConfig>,
    listen: Option<ListenConfig>,
}

#[derive(Clone, Debug)]
struct RouteDefinition {
    method: String,
    path: String,
    middleware: Vec<BxValue>,
    webhook: Option<WebhookConfig>,
    handler: BxValue,
}

#[derive(Clone, Debug)]
struct WebhookConfig {
    secret: String,
    signature_header: String,
    prefix: Option<String>,
    timestamp_header: Option<String>,
    tolerance_seconds: Option<i64>,
    replay_header: Option<String>,
    replay_ttl_seconds: Option<i64>,
}

#[derive(Default, Debug)]
pub struct BuildState {
    pub apps: Vec<Arc<Mutex<AppDefinition>>>,
}

#[derive(Clone, Debug)]
pub struct ResponseData {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub cookies: Vec<String>,
    pub body: Vec<u8>,
}

#[derive(Default, Debug)]
struct ResponseState {
    status: u16,
    headers: HashMap<String, String>,
    cookies: Vec<String>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct WebNamespace {
    build_state: Arc<Mutex<BuildState>>,
    app_root: PathBuf,
}

#[derive(Debug)]
struct ServerAppObject {
    app: Arc<Mutex<AppDefinition>>,
    middleware_namespace: BxValue,
}

#[derive(Debug)]
struct RouteGroupObject {
    app: Arc<Mutex<AppDefinition>>,
    prefix: String,
    middleware: Vec<BxValue>,
}

#[derive(Debug, Default)]
struct WebhookBuilderObject {
    path: Option<String>,
    secret: Option<String>,
    signature_header: Option<String>,
    prefix: Option<String>,
    timestamp_header: Option<String>,
    tolerance_seconds: Option<i64>,
    replay_header: Option<String>,
    replay_ttl_seconds: Option<i64>,
}

#[derive(Debug)]
struct AppMiddlewareNamespaceObject {
    app_root: PathBuf,
}

#[derive(Debug)]
struct StaticFilesMiddlewareObject {
    mount: String,
    directory: PathBuf,
}

#[derive(Debug)]
struct RequestContextObject {
    request: RequestData,
    rc_id: usize,
    prc_id: usize,
    session_id: usize,
    app_root: PathBuf,
    response: Arc<Mutex<ResponseState>>,
}

#[derive(Debug)]
struct NextMiddlewareObject {
    middleware: Vec<BxValue>,
    index: usize,
    handler: BxValue,
    event: BxValue,
    rc: BxValue,
    prc: BxValue,
}

#[derive(Debug)]
struct NotFoundHandlerObject;

impl BxNativeObject for WebNamespace {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "server" => {
                let app = Arc::new(Mutex::new(AppDefinition {
                    middleware: Vec::new(),
                    routes: Vec::new(),
                    websocket: None,
                    listen: None,
                }));
                self.build_state.lock().unwrap().apps.push(app.clone());
                let middleware_id = vm.native_object_new(Rc::new(RefCell::new(
                    AppMiddlewareNamespaceObject {
                        app_root: self.app_root.clone(),
                    },
                )));
                let object_id = vm.native_object_new(Rc::new(RefCell::new(ServerAppObject {
                    app,
                    middleware_namespace: BxValue::new_ptr(middleware_id),
                })));
                Ok(BxValue::new_ptr(object_id))
            }
            _ => Err(format!("Method {} not found on web namespace.", name)),
        }
    }
}

impl BxNativeObject for ServerAppObject {
    fn get_property(&self, name: &str) -> BxValue {
        match name {
            "middleware" => self.middleware_namespace,
            _ => BxValue::new_null(),
        }
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "get" | "post" | "put" | "patch" | "delete" => {
                register_route(vm, &self.app, "", name, args)?;
                Ok(BxValue::new_null())
            }
            "webhook" => {
                register_webhook_route(vm, &self.app, "", args)?;
                Ok(BxValue::new_null())
            }
            "buildwebhook" => {
                let object_id = vm.native_object_new(Rc::new(RefCell::new(WebhookBuilderObject::default())));
                Ok(BxValue::new_ptr(object_id))
            }
            "use" => {
                register_app_middleware(&self.app, args)?;
                Ok(BxValue::new_null())
            }
            "group" => {
                let prefix = parse_group_prefix(vm, args)?;
                let object_id = vm.native_object_new(Rc::new(RefCell::new(RouteGroupObject {
                    app: self.app.clone(),
                    prefix,
                    middleware: Vec::new(),
                })));
                Ok(BxValue::new_ptr(object_id))
            }
            "listen" => {
                let config = parse_listen_config(vm, args)?;
                self.app.lock().unwrap().listen = Some(config);
                Ok(BxValue::new_null())
            }
            "enablewebsockets" => {
                register_websocket_listener(vm, &self.app, args)?;
                Ok(BxValue::new_null())
            }
            _ => Err(format!("Method {} not found on server app.", name)),
        }
    }
}

impl BxNativeObject for RouteGroupObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "get" | "post" | "put" | "patch" | "delete" => {
                register_group_route(vm, self, name, args)?;
                Ok(BxValue::new_null())
            }
            "webhook" => {
                register_group_webhook_route(vm, self, args)?;
                Ok(BxValue::new_null())
            }
            "buildwebhook" => {
                let object_id = vm.native_object_new(Rc::new(RefCell::new(WebhookBuilderObject::default())));
                Ok(BxValue::new_ptr(object_id))
            }
            "use" => {
                self.middleware.extend(args.iter().copied());
                Ok(BxValue::new_null())
            }
            "group" => {
                let suffix = parse_group_prefix(vm, args)?;
                let prefix = join_route_paths(&self.prefix, &suffix);
                let object_id = vm.native_object_new(Rc::new(RefCell::new(RouteGroupObject {
                    app: self.app.clone(),
                    prefix,
                    middleware: self.middleware.clone(),
                })));
                Ok(BxValue::new_ptr(object_id))
            }
            _ => Err(format!("Method {} not found on route group.", name)),
        }
    }
}

impl BxNativeObject for WebhookBuilderObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "path" => {
                let path = args
                    .first()
                    .copied()
                    .filter(|value| vm.is_string_value(*value))
                    .map(|value| vm.to_string(value))
                    .ok_or_else(|| "buildWebhook().path() requires a string path".to_string())?;
                self.path = Some(path);
                Ok(BxValue::new_ptr(id))
            }
            "secret" => {
                let secret = args
                    .first()
                    .copied()
                    .filter(|value| vm.is_string_value(*value))
                    .map(|value| vm.to_string(value))
                    .ok_or_else(|| "buildWebhook().secret() requires a string secret".to_string())?;
                self.secret = Some(secret);
                Ok(BxValue::new_ptr(id))
            }
            "signatureheader" => {
                let header = args
                    .first()
                    .copied()
                    .filter(|value| vm.is_string_value(*value))
                    .map(|value| vm.to_string(value))
                    .ok_or_else(|| "buildWebhook().signatureHeader() requires a string header name".to_string())?;
                self.signature_header = Some(header);
                Ok(BxValue::new_ptr(id))
            }
            "prefix" => {
                let prefix = args
                    .first()
                    .copied()
                    .filter(|value| vm.is_string_value(*value))
                    .map(|value| vm.to_string(value))
                    .ok_or_else(|| "buildWebhook().prefix() requires a string prefix".to_string())?;
                self.prefix = Some(prefix);
                Ok(BxValue::new_ptr(id))
            }
            "timestampheader" => {
                let header = args
                    .first()
                    .copied()
                    .filter(|value| vm.is_string_value(*value))
                    .map(|value| vm.to_string(value))
                    .ok_or_else(|| "buildWebhook().timestampHeader() requires a string header name".to_string())?;
                self.timestamp_header = Some(header);
                Ok(BxValue::new_ptr(id))
            }
            "toleranceseconds" => {
                let tolerance = args
                    .first()
                    .copied()
                    .ok_or_else(|| "buildWebhook().toleranceSeconds() requires a number".to_string())?;
                let value = if tolerance.is_number() || tolerance.is_int() {
                    tolerance.as_number() as i64
                } else {
                    vm.to_string(tolerance)
                        .parse::<i64>()
                        .map_err(|_| "buildWebhook().toleranceSeconds() requires a number".to_string())?
                };
                self.tolerance_seconds = Some(value);
                Ok(BxValue::new_ptr(id))
            }
            "replayheader" => {
                let header = args
                    .first()
                    .copied()
                    .filter(|value| vm.is_string_value(*value))
                    .map(|value| vm.to_string(value))
                    .ok_or_else(|| "buildWebhook().replayHeader() requires a string header name".to_string())?;
                self.replay_header = Some(header);
                Ok(BxValue::new_ptr(id))
            }
            "replayttlseconds" => {
                let ttl = args
                    .first()
                    .copied()
                    .ok_or_else(|| "buildWebhook().replayTtlSeconds() requires a number".to_string())?;
                let value = if ttl.is_number() || ttl.is_int() {
                    ttl.as_number() as i64
                } else {
                    vm.to_string(ttl)
                        .parse::<i64>()
                        .map_err(|_| "buildWebhook().replayTtlSeconds() requires a number".to_string())?
                };
                self.replay_ttl_seconds = Some(value.max(0));
                Ok(BxValue::new_ptr(id))
            }
            "__webhookconfig" => {
                let path = self
                    .path
                    .clone()
                    .ok_or_else(|| "buildWebhook() requires path()".to_string())?;
                let secret = self
                    .secret
                    .clone()
                    .ok_or_else(|| "buildWebhook() requires secret()".to_string())?;
                let signature_header = self
                    .signature_header
                    .clone()
                    .ok_or_else(|| "buildWebhook() requires signatureHeader()".to_string())?;
                if self.timestamp_header.is_some() ^ self.tolerance_seconds.is_some() {
                    return Err(
                        "buildWebhook() requires timestampHeader() and toleranceSeconds() together"
                            .to_string(),
                    );
                }
                if self.replay_ttl_seconds.is_some() && self.replay_header.is_none() {
                    return Err(
                        "buildWebhook() requires replayHeader() when replayTtlSeconds() is set"
                            .to_string(),
                    );
                }
                let config_id = vm.struct_new();
                let path_id = vm.string_new(path);
                let secret_id = vm.string_new(secret);
                let header_id = vm.string_new(signature_header);
                vm.struct_set(config_id, "path", BxValue::new_ptr(path_id));
                vm.struct_set(config_id, "secret", BxValue::new_ptr(secret_id));
                vm.struct_set(
                    config_id,
                    "signatureHeader",
                    BxValue::new_ptr(header_id),
                );
                if let Some(prefix) = self.prefix.clone().filter(|value| !value.is_empty()) {
                    let prefix_id = vm.string_new(prefix);
                    vm.struct_set(config_id, "prefix", BxValue::new_ptr(prefix_id));
                }
                if let Some(timestamp_header) = self
                    .timestamp_header
                    .clone()
                    .filter(|value| !value.is_empty())
                {
                    let timestamp_header_id = vm.string_new(timestamp_header);
                    vm.struct_set(
                        config_id,
                        "timestampHeader",
                        BxValue::new_ptr(timestamp_header_id),
                    );
                }
                if let Some(tolerance_seconds) = self.tolerance_seconds {
                    let tolerance_id = vm.string_new(tolerance_seconds.to_string());
                    vm.struct_set(
                        config_id,
                        "toleranceSeconds",
                        BxValue::new_ptr(tolerance_id),
                    );
                }
                if let Some(replay_header) =
                    self.replay_header.clone().filter(|value| !value.is_empty())
                {
                    let replay_header_id = vm.string_new(replay_header);
                    vm.struct_set(
                        config_id,
                        "replayHeader",
                        BxValue::new_ptr(replay_header_id),
                    );
                }
                if let Some(replay_ttl_seconds) = self.replay_ttl_seconds {
                    let replay_ttl_id = vm.string_new(replay_ttl_seconds.to_string());
                    vm.struct_set(
                        config_id,
                        "replayTtlSeconds",
                        BxValue::new_ptr(replay_ttl_id),
                    );
                }
                Ok(BxValue::new_ptr(config_id))
            }
            _ => Err(format!("Method {} not found on webhook builder.", name)),
        }
    }
}

impl BxNativeObject for AppMiddlewareNamespaceObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "buildstaticfiles" => {
                if args.len() < 2 {
                    return Err(
                        "buildStaticFiles() requires a mount path and directory".to_string(),
                    );
                }
                let mount = args
                    .first()
                    .copied()
                    .filter(|value| vm.is_string_value(*value))
                    .map(|value| normalize_route_path(&vm.to_string(value)))
                    .ok_or_else(|| "buildStaticFiles() mount must be a string".to_string())?;
                let directory = args
                    .get(1)
                    .copied()
                    .filter(|value| vm.is_string_value(*value))
                    .map(|value| vm.to_string(value))
                    .ok_or_else(|| "buildStaticFiles() directory must be a string".to_string())?;
                let resolved_directory = resolve_static_directory(&self.app_root, &directory)?;
                let middleware_id = vm.native_object_new(Rc::new(RefCell::new(
                    StaticFilesMiddlewareObject {
                        mount,
                        directory: resolved_directory,
                    },
                )));
                Ok(BxValue::new_ptr(middleware_id))
            }
            _ => Err(format!("Method {} not found on app middleware.", name)),
        }
    }
}

impl BxNativeObject for StaticFilesMiddlewareObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "__runmiddleware" => {
                if args.len() < 4 {
                    return Err("__runmiddleware requires event, rc, prc, next".to_string());
                }
                let event_id = args[0]
                    .as_gc_id()
                    .ok_or_else(|| "Static file middleware requires an event object".to_string())?;
                let path_value = vm.native_object_call_method(event_id, "__requestpath", &[])?;
                let path = vm.to_string(path_value);
                if !path_matches_mount(&path, &self.mount) {
                    return vm.native_object_call_method(
                        args[3]
                            .as_gc_id()
                            .ok_or_else(|| "Static file middleware requires a next object".to_string())?,
                        "run",
                        &[],
                    );
                }

                let relative_path = path_relative_to_mount(&path, &self.mount);
                let resolved_path = match resolve_static_file_path(&self.directory, &relative_path)
                {
                    Ok(path) => path,
                    Err(err) if err.contains("outside the mounted directory") => {
                        return vm.native_object_call_method(
                            args[3]
                                .as_gc_id()
                                .ok_or_else(|| "Static file middleware requires a next object".to_string())?,
                            "run",
                            &[],
                        );
                    }
                    Err(err) => return Err(err),
                };
                if !resolved_path.is_file() {
                    return vm.native_object_call_method(
                        args[3]
                            .as_gc_id()
                            .ok_or_else(|| "Static file middleware requires a next object".to_string())?,
                        "run",
                        &[],
                    );
                }

                let body = stdfs::read(&resolved_path).map_err(|e| {
                    format!("Failed to read static file '{}': {}", resolved_path.display(), e)
                })?;
                let mime = mime_guess::from_path(&resolved_path)
                    .first_or_octet_stream()
                    .to_string();
                let bytes_id = vm.bytes_new(body);
                let mime_id = vm.string_new(mime);
                vm.native_object_call_method(
                    event_id,
                    "__renderbytes",
                    &[BxValue::new_ptr(mime_id), BxValue::new_ptr(bytes_id)],
                )?;
                Ok(BxValue::new_null())
            }
            _ => Err(format!("Method {} not found on static files middleware.", name)),
        }
    }
}

impl BxNativeObject for RequestContextObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "getcollection" => Ok(BxValue::new_ptr(self.rc_id)),
            "getprivatecollection" => Ok(BxValue::new_ptr(self.prc_id)),
            "getsession" => Ok(BxValue::new_ptr(self.session_id)),
            "valueexists" => Ok(BxValue::new_bool(struct_key_exists(vm, self.rc_id, args))),
            "privatevalueexists" => Ok(BxValue::new_bool(struct_key_exists(vm, self.prc_id, args))),
            "sessionexists" => Ok(BxValue::new_bool(struct_key_exists(vm, self.session_id, args))),
            "getvalue" => Ok(struct_lookup(vm, self.rc_id, args)),
            "getprivatevalue" => Ok(struct_lookup(vm, self.prc_id, args)),
            "getsessionvalue" => Ok(struct_lookup(vm, self.session_id, args)),
            "setvalue" => {
                set_value(vm, self.rc_id, self.prc_id, args);
                Ok(BxValue::new_null())
            }
            "paramvalue" => {
                param_value(vm, self.rc_id, args);
                Ok(BxValue::new_null())
            }
            "paramprivatevalue" => {
                param_value(vm, self.prc_id, args);
                Ok(BxValue::new_null())
            }
            "paramsessionvalue" | "setsessionvalue" => {
                param_value(vm, self.session_id, args);
                Ok(BxValue::new_null())
            }
            "gethttpheader" => {
                let key = arg_string(vm, args, 0).to_lowercase();
                let value = self
                    .request
                    .headers
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| arg_default_string(vm, args, 1));
                Ok(BxValue::new_ptr(vm.string_new(value)))
            }
            "gethttpcookie" | "getcookie" => {
                let key = arg_string(vm, args, 0);
                let value = self
                    .request
                    .cookies
                    .get(&key)
                    .cloned()
                    .or_else(|| self.request.cookies.get(&key.to_lowercase()).cloned())
                    .unwrap_or_else(|| arg_default_string(vm, args, 1));
                Ok(BxValue::new_ptr(vm.string_new(value)))
            }
            "getrouteparams" => {
                let result_id = vm.struct_new();
                for (key, value) in &self.request.route_params {
                    let value_id = vm.string_new(value.clone());
                    vm.struct_set(result_id, key, BxValue::new_ptr(value_id));
                }
                Ok(BxValue::new_ptr(result_id))
            }
            "getcurrentroute" => Ok(BxValue::new_ptr(vm.string_new(
                self.request.matched_route.clone().unwrap_or_default(),
            ))),
            "gethttpmethod" => Ok(BxValue::new_ptr(vm.string_new(self.request.method.clone()))),
            "getrequestmethod" => Ok(BxValue::new_ptr(vm.string_new(self.request.method.clone()))),
            "isjson" => Ok(BxValue::new_bool(is_json_body(&self.request.headers))),
            "isajax" => {
                let requested_with = self
                    .request
                    .headers
                    .get("x-requested-with")
                    .map(|value| value.eq_ignore_ascii_case("XMLHttpRequest"))
                    .unwrap_or(false);
                Ok(BxValue::new_bool(requested_with))
            }
            "gethttpcontent" => {
                let as_json = arg_bool(vm, args, 0);
                if as_json {
                    let value: JsonValue = serde_json::from_slice(&self.request.body)
                        .map_err(|e| format!("Bad Request: Invalid JSON body: {}", e))?;
                    json_to_bx(vm, &value)
                } else {
                    let text = String::from_utf8(self.request.body.clone())
                        .map_err(|e| format!("Request body is not valid UTF-8: {}", e))?;
                    Ok(BxValue::new_ptr(vm.string_new(text)))
                }
            }
            "getpath" => {
                let with_query = args.is_empty() || arg_bool(vm, args, 0);
                let mut value = self.request.path.clone();
                if with_query {
                    if let Some(raw) = &self.request.raw_query {
                        value.push('?');
                        value.push_str(raw);
                    }
                }
                Ok(BxValue::new_ptr(vm.string_new(value)))
            }
            "geturl" => {
                let with_query = args.is_empty() || arg_bool(vm, args, 0);
                let value = if with_query {
                    self.request.full_url.clone()
                } else {
                    self.request.path.clone()
                };
                Ok(BxValue::new_ptr(vm.string_new(value)))
            }
            "__requestpath" => Ok(BxValue::new_ptr(vm.string_new(self.request.path.clone()))),
            "sethttpheader" => {
                let name = arg_string(vm, args, 0);
                let value = arg_string(vm, args, 1);
                self.response
                    .lock()
                    .unwrap()
                    .headers
                    .insert(name, value);
                Ok(BxValue::new_null())
            }
            "sethttpcookie" | "setcookie" => {
                if args.len() < 2 {
                    return Err("setHTTPCookie() requires a name and value".to_string());
                }
                let name = arg_string(vm, args, 0);
                let value = arg_string(vm, args, 1);
                let mut cookie = format!("{}={}", name, value);
                if let Some(path) = args.get(2) {
                    let path = vm.to_string(*path);
                    if !path.is_empty() {
                        cookie.push_str("; Path=");
                        cookie.push_str(&path);
                    }
                } else {
                    cookie.push_str("; Path=/");
                }
                if let Some(http_only) = args.get(3) {
                    let is_http_only = if http_only.is_bool() {
                        http_only.as_bool()
                    } else {
                        matches!(vm.to_string(*http_only).to_lowercase().as_str(), "true" | "yes" | "1")
                    };
                    if is_http_only {
                        cookie.push_str("; HttpOnly");
                    }
                }
                self.response.lock().unwrap().cookies.push(cookie);
                Ok(BxValue::new_null())
            }
            "sethttpstatus" => {
                let code = args.first().map(|v| v.as_number() as u16).unwrap_or(200);
                self.response.lock().unwrap().status = code;
                Ok(BxValue::new_null())
            }
            "getonly" => scoped_struct_subset(vm, self.rc_id, args, true),
            "getexcept" => scoped_struct_subset(vm, self.rc_id, args, false),
            "clearsession" | "invalidatesession" => {
                vm.struct_clear(self.session_id);
                Ok(BxValue::new_null())
            }
            "rendertext" => {
                let text = arg_string(vm, args, 0);
                render_response(&self.response, "text/plain; charset=utf-8", text.into_bytes());
                Ok(BxValue::new_null())
            }
            "renderhtml" => {
                let html = arg_string(vm, args, 0);
                render_response(&self.response, "text/html; charset=utf-8", html.into_bytes());
                Ok(BxValue::new_null())
            }
            "renderjson" => {
                let value = args.first().copied().unwrap_or(BxValue::new_null());
                let json = bx_to_json(vm, value)?;
                let body = serde_json::to_vec(&json).map_err(|e| e.to_string())?;
                render_response(&self.response, "application/json", body);
                Ok(BxValue::new_null())
            }
            "rendertemplate" | "setview" => {
                let template_path = arg_string(vm, args, 0);
                if template_path.is_empty() {
                    return Err("renderTemplate() requires a template path".to_string());
                }
                let view_args = args.get(1).copied().unwrap_or(BxValue::new_null());
                let html = render_template(
                    vm,
                    self.request.clone(),
                    self.rc_id,
                    self.prc_id,
                    self.session_id,
                    self.app_root.clone(),
                    self.response.clone(),
                    &template_path,
                    view_args,
                )?;
                render_response(&self.response, "text/html; charset=utf-8", html.into_bytes());
                Ok(BxValue::new_null())
            }
            "renderdata" => {
                if args.len() < 2 {
                    return Err("renderData() requires type and data arguments".to_string());
                }
                let render_type = arg_string(vm, args, 0).to_lowercase();
                match render_type.as_str() {
                    "json" => {
                        let json = bx_to_json(vm, args[1])?;
                        let body = serde_json::to_vec(&json).map_err(|e| e.to_string())?;
                        render_response(&self.response, "application/json", body);
                    }
                    "html" => {
                        render_response(
                            &self.response,
                            "text/html; charset=utf-8",
                            arg_string(vm, args, 1).into_bytes(),
                        );
                    }
                    "text" | "plain" => {
                        render_response(
                            &self.response,
                            "text/plain; charset=utf-8",
                            arg_string(vm, args, 1).into_bytes(),
                        );
                    }
                    _ => return Err(format!("Unsupported renderData() type '{}'", render_type)),
                }
                Ok(BxValue::new_null())
            }
            "__renderbytes" => {
                let content_type = arg_string(vm, args, 0);
                let body = args
                    .get(1)
                    .copied()
                    .map(|value| vm.to_bytes(value))
                    .transpose()?
                    .unwrap_or_default();
                render_response(&self.response, &content_type, body);
                Ok(BxValue::new_null())
            }
            _ => Err(format!("Method {} not found on request context.", name)),
        }
    }
}

impl BxNativeObject for NextMiddlewareObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "run" => {
                execute_middleware_chain(
                    vm,
                    &self.middleware,
                    self.index,
                    self.handler,
                    self.event,
                    self.rc,
                    self.prc,
                )?;
                Ok(BxValue::new_null())
            }
            _ => Err(format!("Method {} not found on middleware next object.", name)),
        }
    }
}

impl BxNativeObject for NotFoundHandlerObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        _vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "__runhandler" => Err("Not Found".to_string()),
            _ => Err(format!("Method {} not found on not found handler.", name)),
        }
    }
}



pub async fn run_script_server(script_path: &Path) -> anyhow::Result<()> {
    let compiled = Arc::new(compile_script_app(script_path).await?);
    let listen = load_listen_config(&compiled)?;
    let websocket = load_websocket_runtime(compiled.clone())?;
    let state = Arc::new(ScriptServerState { compiled, websocket });
    let router = build_script_router(state.clone());
    let addr: std::net::SocketAddr = format!("{}:{}", listen.host, listen.port).parse()?;
    println!("MatchBox App Server listening on http://{}", addr);
    println!("App script: {}", state.compiled.path.display());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

pub fn build_script_router(state: Arc<ScriptServerState>) -> Router {
    let mut router = Router::new();
    if let Some(runtime) = state.websocket.clone() {
        router = router.route(&runtime.uri, get(websocket_handler).with_state(runtime.clone()));
    }
    router
        .route("/", any(script_handler))
        .route("/*path", any(script_handler))
        .with_state(state)
}

pub async fn compile_script_app(script_path: &Path) -> anyhow::Result<CompiledScriptApp> {
    let path = script_path.to_path_buf();
    let source = fs::read_to_string(&path).await?;
    let ast = parser::parse(&source)?;
    let web_imported = ast.iter().any(|stmt| {
        matches!(
            &stmt.kind,
            matchbox_compiler::ast::StatementKind::Import { path, alias }
                if path.eq_ignore_ascii_case("boxlang.web")
                    && alias
                        .as_ref()
                        .map(|value| value.eq_ignore_ascii_case("web"))
                        .unwrap_or(true)
        )
    });
    let compiler = Compiler::new(&path.to_string_lossy());
    let chunk = compiler.compile(&ast, &source)?;
    Ok(CompiledScriptApp {
        path,
        chunk,
        web_imported,
        session_store: Arc::new(Mutex::new(HashMap::new())),
        webhook_replay_store: Arc::new(Mutex::new(HashMap::new())),
    })
}

fn instantiate_app_definition(compiled: &CompiledScriptApp) -> anyhow::Result<AppDefinition> {
    let build_state = Arc::new(Mutex::new(BuildState::default()));
    let mut vm = VM::new();
    if compiled.web_imported {
        install_web_namespace(&mut vm, build_state.clone(), compiled_app_root(compiled));
    }
    vm.interpret(compiled.chunk.clone())?;
    pick_primary_app(&build_state)
}

pub fn load_listen_config(compiled: &CompiledScriptApp) -> anyhow::Result<ListenConfig> {
    Ok(
        instantiate_app_definition(compiled)?
            .listen
            .unwrap_or_default(),
    )
}

fn load_websocket_runtime(
    compiled: Arc<CompiledScriptApp>,
) -> anyhow::Result<Option<Arc<WebSocketRuntimeHandle>>> {
    let app = instantiate_app_definition(&compiled)?;
    let Some(config) = app.websocket else {
        return Ok(None);
    };

    let (commands_tx, commands_rx) = mpsc::channel();
    let runtime = Arc::new(WebSocketRuntimeHandle {
        uri: config.uri.clone(),
        commands: commands_tx,
    });
    let runtime_compiled = compiled.clone();
    let runtime_app_root = compiled_app_root(&compiled);
    thread::Builder::new()
        .name("matchbox-websocket-runtime".to_string())
        .spawn(move || {
            let web_context = if runtime_compiled.web_imported {
                Some((Arc::new(Mutex::new(BuildState::default())), runtime_app_root))
            } else {
                None
            };
            if let Err(err) = websocket_runtime_main(runtime_compiled.chunk.clone(), config, commands_rx, web_context) {
                eprintln!("WebSocket runtime stopped: {}", err);
            }
        })?;

    Ok(Some(runtime))
}

pub fn dispatch_script_request(
    compiled: &CompiledScriptApp,
    mut request: RequestData,
) -> anyhow::Result<ResponseData> {
    let build_state = Arc::new(Mutex::new(BuildState::default()));
    let mut vm = VM::new();
    if compiled.web_imported {
        install_web_namespace(&mut vm, build_state.clone(), compiled_app_root(compiled));
    }
    vm.interpret(compiled.chunk.clone())?;

    let app = pick_primary_app(&build_state)?;
    let matched_route = match_route(&app, &request.method, &request.path);
    let (route, params) = if let Some((route, params)) = matched_route {
        request.matched_route = Some(route.path.clone());
        request.route_params = params.clone();
        if let Some(webhook) = &route.webhook {
            verify_webhook_signature(compiled, &request, &route.path, webhook)
                .map_err(anyhow::Error::msg)?;
        }
        (Some(route), params)
    } else {
        (None, HashMap::new())
    };

    let rc_id = vm.struct_new();
    for (key, value) in request.query.iter() {
        let value_id = vm.string_new(value.clone());
        vm.struct_set(rc_id, key, BxValue::new_ptr(value_id));
    }

    if is_form_encoded(&request.headers) {
        for (key, value) in parse_form_body(&request.body) {
            let value_id = vm.string_new(value);
            vm.struct_set(rc_id, &key, BxValue::new_ptr(value_id));
        }
    }

    if is_json_body(&request.headers) {
        if let Ok(JsonValue::Object(object)) = serde_json::from_slice::<JsonValue>(&request.body) {
            for (key, value) in object {
                let bx = json_to_bx(&mut vm, &value).map_err(anyhow::Error::msg)?;
                vm.struct_set(rc_id, &key, bx);
            }
        }
    }

    for (key, value) in params {
        let value_id = vm.string_new(value);
        vm.struct_set(rc_id, &key, BxValue::new_ptr(value_id));
    }

    let prc_id = vm.struct_new();
    let existing_session_cookie = request
        .cookies
        .get(SESSION_COOKIE_NAME)
        .cloned()
        .or_else(|| request.cookies.get(&SESSION_COOKIE_NAME.to_lowercase()).cloned());
    let session_key = existing_session_cookie
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let session_id = vm.struct_new();
    if let Some(JsonValue::Object(values)) = compiled
        .session_store
        .lock()
        .unwrap()
        .get(&session_key)
        .cloned()
    {
        for (key, value) in values {
            let bx = json_to_bx(&mut vm, &value).map_err(anyhow::Error::msg)?;
            vm.struct_set(session_id, &key, bx);
        }
    }
    vm.insert_global("session".to_string(), BxValue::new_ptr(session_id));

    let response = Arc::new(Mutex::new(ResponseState {
        status: 200,
        headers: HashMap::new(),
        cookies: Vec::new(),
        body: Vec::new(),
    }));
    let app_root = compiled_app_root(compiled);
    let event_id = vm.native_object_new(Rc::new(RefCell::new(RequestContextObject {
        request,
        rc_id,
        prc_id,
        session_id,
        app_root,
        response: response.clone(),
    })));
    let dummy_chunk = Rc::new(RefCell::new(Chunk::default()));
    drop(dummy_chunk);
    let mut middleware_chain = app.middleware.clone();
    let handler = if let Some(route) = route {
        middleware_chain.extend(route.middleware.clone());
        route.handler
    } else {
        let not_found_id = vm.native_object_new(Rc::new(RefCell::new(NotFoundHandlerObject)));
        BxValue::new_ptr(not_found_id)
    };
    execute_middleware_chain(
        &mut vm,
        &middleware_chain,
        0,
        handler,
        BxValue::new_ptr(event_id),
        BxValue::new_ptr(rc_id),
        BxValue::new_ptr(prc_id),
    )
    .map_err(anyhow::Error::msg)?;

    let session_json = bx_to_json(&vm, BxValue::new_ptr(session_id)).map_err(anyhow::Error::msg)?;
    compiled
        .session_store
        .lock()
        .unwrap()
        .insert(session_key.clone(), session_json);

    if existing_session_cookie.is_none() {
        let mut response_guard = response.lock().unwrap();
        let session_cookie = format!("{}={}; Path=/; HttpOnly", SESSION_COOKIE_NAME, session_key);
        response_guard.cookies.push(session_cookie);
    }

    let response = response.lock().unwrap();
    Ok(ResponseData {
        status: response.status,
        headers: response.headers.clone(),
        cookies: response.cookies.clone(),
        body: response.body.clone(),
    })
}

async fn script_handler(
    State(state): State<Arc<ScriptServerState>>,
    request: Request<Body>,
) -> impl IntoResponse {
    let (parts, body) = request.into_parts();
    let body = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Failed to read request body: {}", err),
            )
                .into_response()
        }
    };

    let request = request_data_from_parts(parts.method, parts.uri.path(), parts.uri.query(), &parts.headers, body.to_vec());
    match dispatch_script_request(&state.compiled, request.clone()) {
        Ok(response) => response_to_http(response),
        Err(err) if err.to_string() == "No route matched" => {
            (StatusCode::NOT_FOUND, "Not Found").into_response()
        }
        Err(err) if err.to_string() == "Not Found" => {
            (StatusCode::NOT_FOUND, "Not Found").into_response()
        }
        Err(err) if err.to_string().contains("Bad Request:") => {
            error_response(StatusCode::BAD_REQUEST, &request, err.to_string())
        }
        Err(err) if err.to_string().contains("Unauthorized:") => {
            error_response(StatusCode::UNAUTHORIZED, &request, err.to_string())
        }
        Err(err) if err.to_string().contains("Conflict:") => {
            error_response(StatusCode::CONFLICT, &request, err.to_string())
        }
        Err(err) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &request,
            format!("Error: {}", err),
        ),
    }
}







fn response_to_http(response: ResponseData) -> Response<Body> {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = Response::builder().status(status);
    for (name, value) in &response.headers {
        builder = builder.header(name, value);
    }
    let mut http_response = builder.body(Body::from(response.body)).unwrap();
    for cookie in response.cookies {
        http_response
            .headers_mut()
            .append(header::SET_COOKIE, cookie.parse().unwrap());
    }
    http_response
}


pub fn install_web_namespace(vm: &mut VM, build_state: Arc<Mutex<BuildState>>, app_root: PathBuf) {
    let web_id = vm.native_object_new(Rc::new(RefCell::new(WebNamespace {
        build_state,
        app_root,
    })));
    vm.insert_global("web".to_string(), BxValue::new_ptr(web_id));
}

fn pick_primary_app(build_state: &Arc<Mutex<BuildState>>) -> anyhow::Result<AppDefinition> {
    let state = build_state.lock().unwrap();
    let app = state
        .apps
        .iter()
        .find(|app| app.lock().unwrap().listen.is_some())
        .or_else(|| state.apps.first())
        .ok_or_else(|| anyhow::anyhow!("No app was created. Use web.server()."))?;
    Ok(app.lock().unwrap().clone())
}

fn register_route(
    vm: &dyn BxVM,
    app: &Arc<Mutex<AppDefinition>>,
    prefix: &str,
    method: &str,
    args: &[BxValue],
) -> Result<(), String> {
    if args.len() < 2 {
        return Err(format!(
            "{}() requires a path and at least one handler function",
            method
        ));
    }
    if !vm.is_string_value(args[0]) {
        return Err(format!("{}() path must be a string", method));
    }
    let path = join_route_paths(prefix, &vm.to_string(args[0]));
    let (middleware, handler) = parse_route_handlers(args)?;
    app.lock().unwrap().routes.push(RouteDefinition {
        method: method.to_uppercase(),
        path,
        middleware,
        webhook: None,
        handler,
    });
    Ok(())
}

fn register_group_route(
    vm: &dyn BxVM,
    group: &RouteGroupObject,
    method: &str,
    args: &[BxValue],
) -> Result<(), String> {
    if args.len() < 2 {
        return Err(format!(
            "{}() requires a path and at least one handler function",
            method
        ));
    }
    if !vm.is_string_value(args[0]) {
        return Err(format!("{}() path must be a string", method));
    }
    let path = join_route_paths(&group.prefix, &vm.to_string(args[0]));
    let (mut middleware, handler) = parse_route_handlers(args)?;
    let mut inherited = group.middleware.clone();
    inherited.append(&mut middleware);
    group.app.lock().unwrap().routes.push(RouteDefinition {
        method: method.to_uppercase(),
        path,
        middleware: inherited,
        webhook: None,
        handler,
    });
    Ok(())
}

fn register_websocket_listener(
    vm: &dyn BxVM,
    app: &Arc<Mutex<AppDefinition>>,
    args: &[BxValue],
) -> Result<(), String> {
    if args.len() < 2 {
        return Err("enableWebSockets() requires a uri and listener object".to_string());
    }
    if !vm.is_string_value(args[0]) {
        return Err("enableWebSockets() uri must be a string".to_string());
    }
    let uri = normalize_route_path(&vm.to_string(args[0]));
    let listener = args[1];
    listener
        .as_gc_id()
        .ok_or_else(|| "enableWebSockets() listener must be an object instance".to_string())?;
    let listener_class = vm.instance_class_name(listener)?;
    let listener_state = vm.instance_variables_json(listener)?;
    app.lock().unwrap().websocket = Some(WebSocketConfig {
        uri,
        listener_class,
        listener_state,
        handler: "WebSocket.bx".to_string(),
    });
    Ok(())
}

fn register_webhook_route(
    vm: &mut dyn BxVM,
    app: &Arc<Mutex<AppDefinition>>,
    prefix: &str,
    args: &[BxValue],
) -> Result<(), String> {
    let (path, webhook, handler) = parse_webhook_definition(vm, prefix, args)?;
    app.lock().unwrap().routes.push(RouteDefinition {
        method: "POST".to_string(),
        path,
        middleware: Vec::new(),
        webhook: Some(webhook),
        handler,
    });
    Ok(())
}

fn register_group_webhook_route(
    vm: &mut dyn BxVM,
    group: &RouteGroupObject,
    args: &[BxValue],
) -> Result<(), String> {
    let (path, webhook, handler) = parse_webhook_definition(vm, &group.prefix, args)?;
    group.app.lock().unwrap().routes.push(RouteDefinition {
        method: "POST".to_string(),
        path,
        middleware: group.middleware.clone(),
        webhook: Some(webhook),
        handler,
    });
    Ok(())
}

fn register_app_middleware(app: &Arc<Mutex<AppDefinition>>, args: &[BxValue]) -> Result<(), String> {
    if args.is_empty() {
        return Err("use() requires at least one middleware function".to_string());
    }
    app.lock().unwrap().middleware.extend(args.iter().copied());
    Ok(())
}

fn parse_route_handlers(args: &[BxValue]) -> Result<(Vec<BxValue>, BxValue), String> {
    if args.len() < 2 {
        return Err("Route requires a path and a handler".to_string());
    }
    let handler = *args.last().unwrap();
    let middleware = if args.len() > 2 {
        args[1..args.len() - 1].to_vec()
    } else {
        Vec::new()
    };
    Ok((middleware, handler))
}

fn parse_webhook_definition(
    vm: &mut dyn BxVM,
    prefix: &str,
    args: &[BxValue],
) -> Result<(String, WebhookConfig, BxValue), String> {
    if args.len() < 2 {
        return Err("webhook() requires a builder or path/config plus a handler".to_string());
    }
    let handler = *args.last().unwrap();

    if args.len() == 2 {
        let builder_id = args[0]
            .as_gc_id()
            .ok_or_else(|| "webhook() requires a webhook builder as the first argument".to_string())?;
        let config = vm
            .native_object_call_method(builder_id, "__webhookconfig", &[])
            .map_err(|_| "webhook() requires a webhook builder or path/config arguments".to_string())?;
        let config_id = config
            .as_gc_id()
            .ok_or_else(|| "webhook() builder did not produce a valid config".to_string())?;
        let path = join_route_paths(prefix, &required_struct_string(vm, config_id, "path")?);
        let secret = required_struct_string(vm, config_id, "secret")?;
        let signature_header = required_struct_string(vm, config_id, "signatureHeader")?;
        let prefix = optional_struct_string(vm, config_id, "prefix");
        let timestamp_header = optional_struct_string(vm, config_id, "timestampHeader")
            .map(|value| value.to_lowercase());
        let tolerance_seconds = optional_struct_i64(vm, config_id, "toleranceSeconds")?;
        let replay_header = optional_struct_string(vm, config_id, "replayHeader")
            .map(|value| value.to_lowercase());
        let replay_ttl_seconds = optional_struct_i64(vm, config_id, "replayTtlSeconds")?;
        return Ok((
            path,
            WebhookConfig {
                secret,
                signature_header: signature_header.to_lowercase(),
                prefix,
                timestamp_header,
                tolerance_seconds,
                replay_header,
                replay_ttl_seconds,
            },
            handler,
        ));
    }

    if !vm.is_string_value(args[0]) {
        return Err("webhook() path must be a string".to_string());
    }
    if !vm.is_struct_value(args[1]) {
        return Err("webhook() config must be a struct".to_string());
    }
    let path = join_route_paths(prefix, &vm.to_string(args[0]));
    let config_id = args[1]
        .as_gc_id()
        .ok_or_else(|| "webhook() config must be a struct".to_string())?;
    let secret = required_struct_string(vm, config_id, "secret")?;
    let signature_header = required_struct_string(vm, config_id, "signatureHeader")?;
    let prefix = optional_struct_string(vm, config_id, "prefix");
    let timestamp_header =
        optional_struct_string(vm, config_id, "timestampHeader").map(|value| value.to_lowercase());
    let tolerance_seconds = optional_struct_i64(vm, config_id, "toleranceSeconds")?;
    let replay_header =
        optional_struct_string(vm, config_id, "replayHeader").map(|value| value.to_lowercase());
    let replay_ttl_seconds = optional_struct_i64(vm, config_id, "replayTtlSeconds")?;
    Ok((
        path,
        WebhookConfig {
            secret,
            signature_header: signature_header.to_lowercase(),
            prefix,
            timestamp_header,
            tolerance_seconds,
            replay_header,
            replay_ttl_seconds,
        },
        handler,
    ))
}

fn parse_group_prefix(vm: &dyn BxVM, args: &[BxValue]) -> Result<String, String> {
    if args.is_empty() || !vm.is_string_value(args[0]) {
        return Err("group() requires a string prefix".to_string());
    }
    Ok(normalize_route_path(&vm.to_string(args[0])))
}

fn required_struct_string(vm: &dyn BxVM, id: usize, key: &str) -> Result<String, String> {
    if !vm.struct_key_exists(id, key) {
        return Err(format!("webhook() config requires '{}'", key));
    }
    Ok(vm.to_string(vm.struct_get(id, key)))
}

fn optional_struct_string(vm: &dyn BxVM, id: usize, key: &str) -> Option<String> {
    vm.struct_key_exists(id, key)
        .then(|| vm.to_string(vm.struct_get(id, key)))
        .filter(|value| !value.is_empty())
}

fn optional_struct_i64(vm: &dyn BxVM, id: usize, key: &str) -> Result<Option<i64>, String> {
    if !vm.struct_key_exists(id, key) {
        return Ok(None);
    }
    let raw = vm.to_string(vm.struct_get(id, key));
    raw.parse::<i64>()
        .map(Some)
        .map_err(|_| format!("webhook() config '{}' must be an integer", key))
}

fn parse_listen_config(vm: &dyn BxVM, args: &[BxValue]) -> Result<ListenConfig, String> {
    if args.is_empty() {
        return Ok(ListenConfig::default());
    }
    let first = args[0];
    if first.is_number() || first.is_int() {
        return Ok(ListenConfig {
            host: "127.0.0.1".to_string(),
            port: first.as_number() as u16,
        });
    }
    if vm.is_struct_value(first) {
        let id = first.as_gc_id().unwrap();
        let host = if vm.struct_key_exists(id, "host") {
            vm.to_string(vm.struct_get(id, "host"))
        } else {
            "127.0.0.1".to_string()
        };
        let port = if vm.struct_key_exists(id, "port") {
            vm.to_string(vm.struct_get(id, "port"))
                .parse::<u16>()
                .map_err(|e| format!("Invalid listen() port: {}", e))?
        } else {
            8080
        };
        return Ok(ListenConfig { host, port });
    }
    Err("listen() expects a port number or config struct".to_string())
}

fn arg_string(vm: &dyn BxVM, args: &[BxValue], index: usize) -> String {
    args.get(index)
        .copied()
        .map(|v| vm.to_string(v))
        .unwrap_or_default()
}

fn arg_default_string(vm: &dyn BxVM, args: &[BxValue], index: usize) -> String {
    args.get(index)
        .copied()
        .map(|v| vm.to_string(v))
        .unwrap_or_default()
}

fn arg_bool(vm: &dyn BxVM, args: &[BxValue], index: usize) -> bool {
    match args.get(index).copied() {
        Some(val) if val.is_bool() => val.as_bool(),
        Some(val) if val.is_number() || val.is_int() => val.as_number() != 0.0,
        Some(val) => matches!(vm.to_string(val).to_lowercase().as_str(), "true" | "yes" | "1"),
        None => false,
    }
}

fn struct_key_exists(vm: &dyn BxVM, scope_id: usize, args: &[BxValue]) -> bool {
    let key = arg_string(vm, args, 0);
    !key.is_empty() && vm.struct_key_exists(scope_id, &key)
}

fn struct_lookup(vm: &dyn BxVM, scope_id: usize, args: &[BxValue]) -> BxValue {
    let key = arg_string(vm, args, 0);
    if vm.struct_key_exists(scope_id, &key) {
        vm.struct_get(scope_id, &key)
    } else {
        args.get(1).copied().unwrap_or(BxValue::new_null())
    }
}

fn set_value(vm: &mut dyn BxVM, rc_id: usize, prc_id: usize, args: &[BxValue]) {
    if args.len() < 2 {
        return;
    }
    let key = vm.to_string(args[0]);
    let target_scope = if arg_bool(vm, args, 2) { prc_id } else { rc_id };
    vm.struct_set(target_scope, &key, args[1]);
}

fn param_value(vm: &mut dyn BxVM, scope_id: usize, args: &[BxValue]) {
    if args.len() < 2 {
        return;
    }
    let key = vm.to_string(args[0]);
    if !vm.struct_key_exists(scope_id, &key) {
        vm.struct_set(scope_id, &key, args[1]);
    }
}

fn render_response(response: &Arc<Mutex<ResponseState>>, content_type: &str, body: Vec<u8>) {
    let mut response = response.lock().unwrap();
    response
        .headers
        .insert(header::CONTENT_TYPE.to_string(), content_type.to_string());
    response.body = body;
}

fn error_response(status: StatusCode, request: &RequestData, message: String) -> Response<Body> {
    let accepts_json = request
        .headers
        .get("accept")
        .map(|accept| accept.to_lowercase().contains("application/json"))
        .unwrap_or(false);

    if accepts_json {
        let body = serde_json::to_vec(&serde_json::json!({ "error": message })).unwrap();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    } else {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Body::from(message))
            .unwrap()
    }
}

fn verify_webhook_signature(
    compiled: &CompiledScriptApp,
    request: &RequestData,
    route_path: &str,
    webhook: &WebhookConfig,
) -> Result<(), String> {
    let received = request
        .headers
        .get(&webhook.signature_header)
        .cloned()
        .ok_or_else(|| format!("Unauthorized: Missing {}", webhook.signature_header))?;
    let expected = compute_hmac_sha256_hex(&webhook.secret, &request.body);
    let normalized_received = normalize_signature(&received, webhook.prefix.as_deref());
    let normalized_expected = normalize_signature(&expected, webhook.prefix.as_deref());
    if normalized_received == normalized_expected {
        verify_webhook_timestamp(request, webhook)?;
        verify_webhook_replay(compiled, request, route_path, webhook)
    } else {
        Err("Unauthorized: Invalid webhook signature".to_string())
    }
}

fn verify_webhook_timestamp(request: &RequestData, webhook: &WebhookConfig) -> Result<(), String> {
    let Some(timestamp_header) = webhook.timestamp_header.as_ref() else {
        return Ok(());
    };
    let tolerance_seconds = webhook.tolerance_seconds.unwrap_or_default();
    let raw_timestamp = request
        .headers
        .get(timestamp_header)
        .cloned()
        .ok_or_else(|| format!("Unauthorized: Missing {}", timestamp_header))?;
    let timestamp = raw_timestamp
        .trim()
        .parse::<i64>()
        .map_err(|_| format!("Unauthorized: Invalid {} timestamp", timestamp_header))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Unauthorized: System clock error".to_string())?
        .as_secs() as i64;
    if (now - timestamp).abs() > tolerance_seconds {
        return Err("Unauthorized: Webhook timestamp outside tolerance".to_string());
    }
    Ok(())
}

fn verify_webhook_replay(
    compiled: &CompiledScriptApp,
    request: &RequestData,
    route_path: &str,
    webhook: &WebhookConfig,
) -> Result<(), String> {
    let Some(replay_header) = webhook.replay_header.as_ref() else {
        return Ok(());
    };
    let ttl_seconds = webhook
        .replay_ttl_seconds
        .unwrap_or(DEFAULT_WEBHOOK_REPLAY_TTL_SECONDS)
        .max(0);
    let delivery_id = request
        .headers
        .get(replay_header)
        .cloned()
        .ok_or_else(|| format!("Unauthorized: Missing {}", replay_header))?;
    let replay_key = format!("{}:{}", route_path, delivery_id.trim());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Unauthorized: System clock error".to_string())?
        .as_secs() as i64;
    let mut replay_store = compiled.webhook_replay_store.lock().unwrap();
    replay_store.retain(|_, seen_at| now.saturating_sub(*seen_at) <= ttl_seconds);
    if replay_store.contains_key(&replay_key) {
        return Err("Conflict: Duplicate webhook delivery".to_string());
    }
    replay_store.insert(replay_key, now);
    Ok(())
}

fn normalize_signature(signature: &str, prefix: Option<&str>) -> String {
    let trimmed = signature.trim();
    if let Some(prefix) = prefix {
        if let Some(stripped) = trimmed.strip_prefix(prefix) {
            return stripped.trim().to_lowercase();
        }
    }
    trimmed.to_lowercase()
}

fn compute_hmac_sha256_hex(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let bytes = mac.finalize().into_bytes();
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn scoped_struct_subset(
    vm: &mut dyn BxVM,
    scope_id: usize,
    args: &[BxValue],
    include: bool,
) -> Result<BxValue, String> {
    let keys = parse_key_list(vm, args);
    let result_id = vm.struct_new();
    for key in vm.struct_key_array(scope_id) {
        let listed = keys.iter().any(|candidate| candidate.eq_ignore_ascii_case(&key));
        if (include && listed) || (!include && !listed) {
            let value = vm.struct_get(scope_id, &key);
            vm.struct_set(result_id, &key, value);
        }
    }
    Ok(BxValue::new_ptr(result_id))
}

fn parse_key_list(vm: &dyn BxVM, args: &[BxValue]) -> Vec<String> {
    let raw = arg_string(vm, args, 0);
    raw.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_string())
        .collect()
}

fn render_template(
    vm: &mut dyn BxVM,
    request: RequestData,
    rc_id: usize,
    prc_id: usize,
    session_id: usize,
    app_root: PathBuf,
    response: Arc<Mutex<ResponseState>>,
    template_path: &str,
    view_args: BxValue,
) -> Result<String, String> {
    let resolved_path = resolve_template_path(&app_root, template_path)?;
    let source = stdfs::read_to_string(&resolved_path).map_err(|e| e.to_string())?;
    let ext = resolved_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let ast = if ext.eq_ignore_ascii_case("bxm") {
        parser::parse_bxm(&source).map_err(|e| e.to_string())?
    } else {
        parser::parse(&source).map_err(|e| e.to_string())?
    };

    let compiler = Compiler::new(&resolved_path.to_string_lossy());
    let chunk = compiler.compile(&ast, &source).map_err(|e| e.to_string())?;

    let rc_json = bx_to_json(vm, BxValue::new_ptr(rc_id))?;
    let prc_json = bx_to_json(vm, BxValue::new_ptr(prc_id))?;
    let session_json = bx_to_json(vm, BxValue::new_ptr(session_id))?;
    let view_args_json = bx_to_json(vm, view_args)?;

    let mut template_vm = VM::new();
    let template_rc = json_to_bx(&mut template_vm, &rc_json)?;
    let template_prc = json_to_bx(&mut template_vm, &prc_json)?;
    let template_session = json_to_bx(&mut template_vm, &session_json)?;
    let template_view_args = json_to_bx(&mut template_vm, &view_args_json)?;
    let event_id = template_vm.native_object_new(Rc::new(RefCell::new(RequestContextObject {
        request,
        rc_id: template_rc.as_gc_id().unwrap(),
        prc_id: template_prc.as_gc_id().unwrap(),
        session_id: template_session.as_gc_id().unwrap(),
        app_root,
        response,
    })));

    template_vm.insert_global("event".to_string(), BxValue::new_ptr(event_id));
    template_vm.insert_global("rc".to_string(), template_rc);
    template_vm.insert_global("prc".to_string(), template_prc);
    template_vm.insert_global("session".to_string(), template_session);
    template_vm.insert_global("viewArgs".to_string(), template_view_args);
    template_vm.begin_output_capture();
    template_vm.interpret_chunk(chunk)?;
    let html = template_vm.end_output_capture().unwrap_or_default();

    sync_struct_from_vm(&template_vm, vm, template_rc, rc_id)?;
    sync_struct_from_vm(&template_vm, vm, template_prc, prc_id)?;
    sync_struct_from_vm(&template_vm, vm, template_session, session_id)?;

    Ok(html)
}

fn resolve_template_path(app_root: &Path, template_path: &str) -> Result<PathBuf, String> {
    let candidate = if Path::new(template_path).is_absolute() {
        PathBuf::from(template_path)
    } else {
        app_root.join(template_path.trim_start_matches('/'))
    };

    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Template '{}' could not be resolved: {}", template_path, e))?;
    let root = app_root
        .canonicalize()
        .unwrap_or_else(|_| app_root.to_path_buf());

    if !canonical.starts_with(&root) {
        return Err(format!(
            "Template '{}' resolves outside the app root",
            template_path
        ));
    }

    Ok(canonical)
}

fn compiled_app_root(compiled: &CompiledScriptApp) -> PathBuf {
    compiled
        .path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_static_directory(app_root: &Path, directory: &str) -> Result<PathBuf, String> {
    let candidate = if Path::new(directory).is_absolute() {
        PathBuf::from(directory)
    } else {
        app_root.join(directory)
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Static directory '{}' could not be resolved: {}", directory, e))?;
    let root = app_root
        .canonicalize()
        .unwrap_or_else(|_| app_root.to_path_buf());
    if !canonical.starts_with(&root) {
        return Err(format!(
            "Static directory '{}' resolves outside the app root",
            directory
        ));
    }
    Ok(canonical)
}

fn resolve_static_file_path(directory: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let relative = relative_path.trim_start_matches('/');
    let candidate = if relative.is_empty() {
        directory.join("index.html")
    } else {
        directory.join(relative)
    };
    let root = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());
    if !candidate.exists() {
        return Ok(candidate);
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Static file '{}' could not be resolved: {}", relative_path, e))?;
    if !canonical.starts_with(&root) {
        return Err(format!(
            "Static file '{}' resolves outside the mounted directory",
            relative_path
        ));
    }
    Ok(canonical)
}

fn path_matches_mount(path: &str, mount: &str) -> bool {
    if mount == "/" {
        return true;
    }
    path == mount || path.starts_with(&format!("{}/", mount.trim_end_matches('/')))
}

fn path_relative_to_mount(path: &str, mount: &str) -> String {
    if mount == "/" {
        return path.trim_start_matches('/').to_string();
    }
    if path == mount {
        return String::new();
    }
    path
        .strip_prefix(mount.trim_end_matches('/'))
        .unwrap_or(path)
        .trim_start_matches('/')
        .to_string()
}

fn sync_struct_from_vm(
    source_vm: &dyn BxVM,
    target_vm: &mut dyn BxVM,
    source_value: BxValue,
    target_id: usize,
) -> Result<(), String> {
    let json = bx_to_json(source_vm, source_value)?;
    match json {
        JsonValue::Object(values) => {
            target_vm.struct_clear(target_id);
            for (key, value) in values {
                let bx = json_to_bx(target_vm, &value)?;
                target_vm.struct_set(target_id, &key, bx);
            }
            Ok(())
        }
        _ => Err("Expected struct value while syncing template scope".to_string()),
    }
}


fn is_form_encoded(headers: &HashMap<String, String>) -> bool {
    headers
        .get("content-type")
        .map(|ct| ct.starts_with("application/x-www-form-urlencoded"))
        .unwrap_or(false)
}

fn is_json_body(headers: &HashMap<String, String>) -> bool {
    headers
        .get("content-type")
        .map(|ct| ct.starts_with("application/json"))
        .unwrap_or(false)
}

fn parse_form_body(body: &[u8]) -> HashMap<String, String> {
    form_urlencoded::parse(body).into_owned().collect()
}

fn execute_middleware_chain(
    vm: &mut dyn BxVM,
    middleware: &[BxValue],
    index: usize,
    handler: BxValue,
    event: BxValue,
    rc: BxValue,
    prc: BxValue,
) -> Result<(), String> {
    let dummy_chunk = Rc::new(RefCell::new(Chunk::default()));
    if let Some(current) = middleware.get(index).copied() {
        let next_id = vm.native_object_new(Rc::new(RefCell::new(NextMiddlewareObject {
            middleware: middleware.to_vec(),
            index: index + 1,
            handler,
            event,
            rc,
            prc,
        })));
        execute_middleware_entry(
            vm,
            current,
            event,
            rc,
            prc,
            BxValue::new_ptr(next_id),
            dummy_chunk,
        )
    } else {
        execute_handler(vm, handler, event, rc, prc, dummy_chunk)
    }
}

fn execute_middleware_entry(
    vm: &mut dyn BxVM,
    middleware: BxValue,
    event: BxValue,
    rc: BxValue,
    prc: BxValue,
    next: BxValue,
    chunk: Rc<RefCell<Chunk>>,
) -> Result<(), String> {
    if let Some(id) = middleware.as_gc_id() {
        match vm.native_object_call_method(id, "__runmiddleware", &[event, rc, prc, next]) {
            Ok(result) => {
                let _ = result;
                return Ok(());
            }
            Err(err) if err.contains("is not a native object") => {}
            Err(err) => return Err(err),
        }
    }
    vm.call_function_by_value(&middleware, vec![event, rc, prc, next], chunk)
        .map(|_| ())
}

fn execute_handler(
    vm: &mut dyn BxVM,
    handler: BxValue,
    event: BxValue,
    rc: BxValue,
    prc: BxValue,
    chunk: Rc<RefCell<Chunk>>,
) -> Result<(), String> {
    if let Some(id) = handler.as_gc_id() {
        match vm.native_object_call_method(id, "__runhandler", &[event, rc, prc]) {
            Ok(result) => {
                let _ = result;
                return Ok(());
            }
            Err(err) if err.contains("is not a native object") => {}
            Err(err) => return Err(err),
        }
    }
    vm.call_function_by_value(&handler, vec![event, rc, prc], chunk)
        .map(|_| ())
}

fn normalize_route_path(path: &str) -> String {
    if path == "/" || path.is_empty() {
        return "/".to_string();
    }
    let trimmed = path.trim();
    let trimmed = trimmed.trim_matches('/');
    format!("/{}", trimmed)
}

fn join_route_paths(prefix: &str, path: &str) -> String {
    let prefix = normalize_route_path(prefix);
    let path = normalize_route_path(path);
    if prefix == "/" {
        return path;
    }
    if path == "/" {
        return prefix;
    }
    format!(
        "{}/{}",
        prefix.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn match_route(app: &AppDefinition, method: &str, path: &str) -> Option<(RouteDefinition, HashMap<String, String>)> {
    for route in &app.routes {
        if route.method != method.to_uppercase() {
            continue;
        }
        if let Some(params) = match_path_pattern(&route.path, path) {
            return Some((route.clone(), params));
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use futures_util::{SinkExt, StreamExt};
    use tokio::time::{sleep, Duration};
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    fn request(
        method: &str,
        path: &str,
        query: Option<&str>,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> RequestData {
        let mut header_map = HeaderMap::new();
        for (name, value) in headers {
            let header_name: axum::http::HeaderName = name.parse().unwrap();
            header_map.insert(header_name, value.parse().unwrap());
        }
        request_data_from_parts(
            method.parse().unwrap(),
            path,
            query,
            &header_map,
            body.to_vec(),
        )
    }

    fn app_state(compiled: Arc<CompiledScriptApp>) -> Arc<ScriptServerState> {
        Arc::new(ScriptServerState {
            compiled,
            websocket: None,
        })
    }

    fn available_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    #[tokio::test]
    async fn script_routes_json_and_rc_params() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.get("/health", function(event, rc, prc) {
                    event.renderJson({ "ok": true });
                });
                app.get("/users/:id", function(event, rc, prc) {
                    prc.userId = rc.id;
                    event.renderJson({ "id": rc.id, "fromPrc": prc.userId });
                });
                app.listen(8081);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let response = dispatch_script_request(
            &compiled,
            request("GET", "/users/42", None, &[("host", "localhost:8081")], &[]),
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            r#"{"fromprc":"42","id":"42"}"#
        );
    }

    #[tokio::test]
    async fn script_parses_json_body_and_sets_status() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.post("/users", function(event, rc, prc) {
                    body = event.getHTTPContent(true);
                    event.setHTTPStatus(201);
                    event.renderJson(body);
                });
                app.listen(9090);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let response = dispatch_script_request(
            &compiled,
            request(
                "POST",
                "/users",
                None,
                &[
                    ("host", "localhost:9090"),
                    ("content-type", "application/json"),
                ],
                br#"{ "name": "MatchBox" }"#,
            ),
        )
        .unwrap();

        assert_eq!(response.status, 201);
        assert_eq!(String::from_utf8(response.body).unwrap(), r#"{"name":"MatchBox"}"#);
    }

    #[tokio::test]
    async fn script_group_prefixes_routes() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                api = app.group("/api");
                v1 = api.group("/v1");
                v1.get("/health", function(event, rc, prc) {
                    event.renderJson({ "ok": true });
                });
                app.listen(8082);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let response = dispatch_script_request(
            &compiled,
            request("GET", "/api/v1/health", None, &[("host", "localhost:8082")], &[]),
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(String::from_utf8(response.body).unwrap(), r#"{"ok":true}"#);
    }

    #[tokio::test]
    async fn script_runs_app_and_route_middleware() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.use(function(event, rc, prc, next) {
                    prc.order = "app";
                    next.run();
                });
                app.get("/mw", function(event, rc, prc, next) {
                    prc.order = prc.order & ",route";
                    next.run();
                }, function(event, rc, prc) {
                    event.renderJson({ "order": prc.order });
                });
                app.listen(8083);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let response = dispatch_script_request(
            &compiled,
            request("GET", "/mw", None, &[("host", "localhost:8083")], &[]),
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(String::from_utf8(response.body).unwrap(), r#"{"order":"app,route"}"#);
    }

    #[tokio::test]
    async fn script_bad_json_returns_400() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.post("/json", function(event, rc, prc) {
                    body = event.getHTTPContent(true);
                    event.renderJson(body);
                });
                app.listen(8084);
            "#,
        )
        .unwrap();

        let compiled = Arc::new(compile_script_app(&script_path).await.unwrap());
        let request = Request::builder()
            .method("POST")
            .uri("/json")
            .header("host", "localhost:8084")
            .header("content-type", "application/json")
            .body(Body::from(br#"{ "broken": "#.to_vec()))
            .unwrap();

        let response = script_handler(State(app_state(compiled)), request).await.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn script_reads_request_cookie() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.get("/cookie", function(event, rc, prc) {
                    event.renderJson({ "theme": event.getHTTPCookie("theme", "default") });
                });
                app.listen(8085);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let response = dispatch_script_request(
            &compiled,
            request(
                "GET",
                "/cookie",
                None,
                &[("host", "localhost:8085"), ("cookie", "theme=dark; session=abc")],
                &[],
            ),
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(String::from_utf8(response.body).unwrap(), r#"{"theme":"dark"}"#);
    }

    #[tokio::test]
    async fn script_sets_response_cookie() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.get("/cookie", function(event, rc, prc) {
                    event.setHTTPCookie("theme", "dark", "/", true);
                    event.renderText("ok");
                });
                app.listen(8086);
            "#,
        )
        .unwrap();

        let compiled = Arc::new(compile_script_app(&script_path).await.unwrap());
        let request = Request::builder()
            .method("GET")
            .uri("/cookie")
            .header("host", "localhost:8086")
            .body(Body::empty())
            .unwrap();

        let response = script_handler(State(app_state(compiled)), request).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert_eq!(cookies.len(), 2);
        assert!(cookies.contains(&"theme=dark; Path=/; HttpOnly".to_string()));
        assert!(
            cookies
                .iter()
                .any(|cookie| cookie.starts_with("MBX_SESSION_ID=") && cookie.ends_with("; Path=/; HttpOnly"))
        );
    }

    #[tokio::test]
    async fn script_persists_session_scope_across_requests() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.get("/session", function(event, rc, prc) {
                    session.count = (session.count ?: 0) + 1;
                    event.renderJson({ "count": session.count });
                });
                app.listen(8087);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();

        let first = dispatch_script_request(
            &compiled,
            request("GET", "/session", None, &[("host", "localhost:8087")], &[]),
        )
        .unwrap();
        assert_eq!(String::from_utf8(first.body.clone()).unwrap(), r#"{"count":1.0}"#);
        assert_eq!(first.cookies.len(), 1);

        let second = dispatch_script_request(
            &compiled,
            request(
                "GET",
                "/session",
                None,
                &[("host", "localhost:8087"), ("cookie", &first.cookies[0])],
                &[],
            ),
        )
        .unwrap();
        assert_eq!(String::from_utf8(second.body).unwrap(), r#"{"count":2.0}"#);
    }

    #[tokio::test]
    async fn script_renders_template_with_event_rc_prc_and_view_args() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        let views_dir = dir.path().join("views");
        std::fs::create_dir_all(&views_dir).unwrap();
        std::fs::write(
            views_dir.join("hello.bxm"),
            r#"<bx:output>#viewArgs.greeting# #rc.name# / #prc.role# / #event.getHTTPMethod()#</bx:output>"#,
        )
        .unwrap();
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.get("/hello/:name", function(event, rc, prc) {
                    prc.role = "admin";
                    event.renderTemplate("views/hello.bxm", { "greeting": "Hello" });
                });
                app.listen(8088);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let response = dispatch_script_request(
            &compiled,
            request("GET", "/hello/Jacob", None, &[("host", "localhost:8088")], &[]),
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            "Hello Jacob / admin / GET"
        );
    }

    #[tokio::test]
    async fn script_formats_handler_errors_based_on_accept_header() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.get("/boom", function(event, rc, prc) {
                    event.getHTTPContent(true);
                });
                app.listen(8089);
            "#,
        )
        .unwrap();

        let compiled = Arc::new(compile_script_app(&script_path).await.unwrap());

        let json_request = Request::builder()
            .method("GET")
            .uri("/boom")
            .header("host", "localhost:8089")
            .header("accept", "application/json")
            .body(Body::empty())
            .unwrap();

        let json_response = script_handler(State(app_state(compiled.clone())), json_request)
            .await
            .into_response();
        assert_eq!(json_response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );

        let text_request = Request::builder()
            .method("GET")
            .uri("/boom")
            .header("host", "localhost:8089")
            .body(Body::empty())
            .unwrap();

        let text_response = script_handler(State(app_state(compiled)), text_request)
            .await
            .into_response();
        assert_eq!(text_response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            text_response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/plain; charset=utf-8")
        );
    }

    #[tokio::test]
    async fn script_supports_request_and_session_convenience_methods() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.get("/convenience/:id", function(event, rc, prc) {
                    event.setValue("fromSetValue", "yes");
                    event.setValue("secret", "private", true);
                    session.count = 7;
                    output = {
                        "hasId": event.valueExists("id"),
                        "hasMissing": event.valueExists("missing"),
                        "only": event.getOnly("id,fromSetValue"),
                        "except": event.getExcept("secret"),
                        "secret": event.getPrivateValue("secret"),
                        "sessionExists": event.sessionExists("count"),
                        "methodAlias": event.getRequestMethod()
                    };
                    event.clearSession();
                    output.cleared = !event.sessionExists("count");
                    event.setSessionValue("keep", "alive");
                    event.invalidateSession();
                    output.invalidated = !event.sessionExists("keep");
                    event.renderJson(output);
                });
                app.listen(8090);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let response = dispatch_script_request(
            &compiled,
            request(
                "GET",
                "/convenience/42",
                Some("extra=1"),
                &[("host", "localhost:8090")],
                &[],
            ),
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            r#"{"cleared":true,"except":{"extra":"1","fromsetvalue":"yes","id":"42"},"hasid":true,"hasmissing":false,"invalidated":true,"methodalias":"GET","only":{"fromsetvalue":"yes","id":"42"},"secret":"private","sessionexists":true}"#
        );
    }

    #[tokio::test]
    async fn script_supports_route_metadata_and_request_introspection_helpers() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.post("/meta/:id", function(event, rc, prc) {
                    event.renderJson({
                        "currentRoute": event.getCurrentRoute(),
                        "routeParams": event.getRouteParams(),
                        "isJson": event.isJSON(),
                        "isAjax": event.isAjax()
                    });
                });
                app.listen(8091);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let response = dispatch_script_request(
            &compiled,
            request(
                "POST",
                "/meta/42",
                None,
                &[
                    ("host", "localhost:8091"),
                    ("content-type", "application/json"),
                    ("x-requested-with", "XMLHttpRequest"),
                ],
                br#"{ "ok": true }"#,
            ),
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            r#"{"currentroute":"/meta/:id","isajax":true,"isjson":true,"routeparams":{"id":"42"}}"#
        );
    }

    #[tokio::test]
    async fn script_registers_socketbox_style_websocket_listener() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                class WebSocket {
                    function onConnect( required channel ) {}
                    function onMessage( required message, required channel ) {}
                    function onClose( required channel ) {}
                }

                import boxlang.web;

                app = web.server();
                app.enableWebSockets( "/ws", new WebSocket() );
                app.listen(8092);
            "#,
        )
        .unwrap();

        let compiled = Arc::new(compile_script_app(&script_path).await.unwrap());
        let listen = load_listen_config(&compiled).unwrap();
        let app = instantiate_app_definition(&compiled).unwrap();

        assert_eq!(listen.port, 8092);
        assert_eq!(app.websocket.as_ref().map(|runtime| runtime.uri.as_str()), Some("/ws"));
        assert_eq!(
            app.websocket
                .as_ref()
                .map(|runtime| runtime.listener_class.as_str()),
            Some("WebSocket")
        );
    }

    #[tokio::test]
    async fn script_requires_boxlang_web_import() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                app = web.server();
                app.listen(8092);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let err = load_listen_config(&compiled).unwrap_err().to_string();
        assert!(
            err.to_lowercase().contains("web") || err.contains("No app was created"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn script_defaults_listen_config_when_omitted() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.get("/", function( event, rc, prc ) {
                    event.renderHtml("<h1>ready</h1>");
                });
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let listen = load_listen_config(&compiled).unwrap();

        assert_eq!(listen.host, "127.0.0.1");
        assert_eq!(listen.port, 8080);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_websocket_listener_receives_messages_and_sends_replies() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        let port = available_port();
        std::fs::write(
            &script_path,
            format!(
                r#"
                class WebSocket {{
                    function configure() {{
                        variables.prefix = "echo:";
                    }}

                    function onConnect( required channel ) {{
                        channel.sendMessage( "welcome" );
                    }}

                    function onMessage( required message, required channel ) {{
                        channel.sendMessage( variables.prefix & message );
                    }}

                    function onClose( required channel ) {{}}
                }}

                import boxlang.web;

                app = web.server();
                listener = new WebSocket();
                listener.configure();
                app.enableWebSockets( "/ws", listener );
                app.listen({port});
            "#
            ),
        )
        .unwrap();

        let server_path = script_path.clone();
        let server = tokio::spawn(async move {
            run_script_server(&server_path).await.unwrap();
        });

        sleep(Duration::from_millis(200)).await;

        let url = format!("ws://127.0.0.1:{port}/ws");
        let (mut stream, _) = connect_async(&url).await.unwrap();

        let welcome = stream.next().await.unwrap().unwrap();
        assert_eq!(welcome.into_text().unwrap(), "welcome");

        stream.send(Message::Text("ping".to_string())).await.unwrap();
        let reply = stream.next().await.unwrap().unwrap();
        assert_eq!(reply.into_text().unwrap(), "echo:ping");

        let _ = stream.close(None).await;
        server.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_websocket_listener_preserves_instance_state_and_broadcasts() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        let port = available_port();
        std::fs::write(
            &script_path,
            format!(
                r#"
                class WebSocket {{
                    function configure( required prefix ) {{
                        variables.prefix = prefix;
                        variables.count = 2;
                    }}

                    function onConnect( required channel ) {{
                        channel.sendJson( {{ "count": variables.count, "prefix": variables.prefix }} );
                    }}

                    function onMessage( required message, required channel ) {{
                        variables.count = variables.count + 1;
                        channel.broadcastJson( {{ "count": variables.count, "prefix": variables.prefix }} );
                    }}

                    function onClose( required channel ) {{}}
                }}

                import boxlang.web;

                app = web.server();
                listener = new WebSocket();
                listener.configure( "clicks:" );
                app.enableWebSockets( "/ws", listener );
                app.listen({port});
            "#
            ),
        )
        .unwrap();

        let server_path = script_path.clone();
        let server = tokio::spawn(async move {
            run_script_server(&server_path).await.unwrap();
        });

        sleep(Duration::from_millis(200)).await;

        let url = format!("ws://127.0.0.1:{port}/ws");
        let (mut first, _) = connect_async(&url).await.unwrap();
        let first_welcome = first.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(first_welcome, r#"{"count":2.0,"prefix":"clicks:"}"#);

        let (mut second, _) = connect_async(&url).await.unwrap();
        let second_welcome = second.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(second_welcome, r#"{"count":2.0,"prefix":"clicks:"}"#);

        first.send(Message::Text("increment".to_string())).await.unwrap();

        let first_broadcast = first.next().await.unwrap().unwrap().into_text().unwrap();
        let second_broadcast = second.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(first_broadcast, r#"{"count":3.0,"prefix":"clicks:"}"#);
        assert_eq!(second_broadcast, r#"{"count":3.0,"prefix":"clicks:"}"#);

        let _ = first.close(None).await;
        let _ = second.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn script_webhook_verifies_hmac_signature() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.webhook(
                    app.buildWebhook()
                        .path("/stripe")
                        .secret("topsecret")
                        .signatureHeader("stripe-signature")
                        .prefix("sha256="),
                    function(event, rc, prc) {
                    event.renderJson(event.getHTTPContent(true));
                    }
                );
                app.listen(8092);
            "#,
        )
        .unwrap();

        let compiled = compile_script_app(&script_path).await.unwrap();
        let body = br#"{"event":"invoice.paid"}"#;
        let signature = format!("sha256={}", test_hmac_sha256_hex("topsecret", body));
        let response = dispatch_script_request(
            &compiled,
            request(
                "POST",
                "/stripe",
                None,
                &[
                    ("host", "localhost:8092"),
                    ("content-type", "application/json"),
                    ("stripe-signature", &signature),
                ],
                body,
            ),
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            r#"{"event":"invoice.paid"}"#
        );
    }

    #[tokio::test]
    async fn script_webhook_rejects_invalid_signature() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                api = app.group("/api");
                api.webhook(
                    api.buildWebhook()
                        .path("/github")
                        .secret("topsecret")
                        .signatureHeader("x-hub-signature-256")
                        .prefix("sha256="),
                    function(event, rc, prc) {
                    event.renderText("ok");
                    }
                );
                app.listen(8093);
            "#,
        )
        .unwrap();

        let compiled = Arc::new(compile_script_app(&script_path).await.unwrap());
        let request = Request::builder()
            .method("POST")
            .uri("/api/github")
            .header("host", "localhost:8093")
            .header("accept", "application/json")
            .header("x-hub-signature-256", "sha256=bad")
            .body(Body::from(br#"{}"#.to_vec()))
            .unwrap();

        let response = script_handler(State(app_state(compiled)), request).await.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
    }

    #[tokio::test]
    async fn script_webhook_enforces_timestamp_tolerance() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.webhook(
                    app.buildWebhook()
                        .path("/signed")
                        .secret("topsecret")
                        .signatureHeader("x-signature")
                        .prefix("sha256=")
                        .timestampHeader("x-timestamp")
                        .toleranceSeconds(300),
                    function(event, rc, prc) {
                        event.renderText("ok");
                    }
                );
                app.listen(8094);
            "#,
        )
        .unwrap();

        let compiled = Arc::new(compile_script_app(&script_path).await.unwrap());
        let body = br#"{"ok":true}"#;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();
        let signature = format!("sha256={}", test_hmac_sha256_hex("topsecret", body));

        let ok_request = Request::builder()
            .method("POST")
            .uri("/signed")
            .header("host", "localhost:8094")
            .header("x-signature", &signature)
            .header("x-timestamp", &now)
            .body(Body::from(body.to_vec()))
            .unwrap();
        let ok_response = script_handler(State(app_state(compiled.clone())), ok_request).await.into_response();
        assert_eq!(ok_response.status(), StatusCode::OK);

        let stale_request = Request::builder()
            .method("POST")
            .uri("/signed")
            .header("host", "localhost:8094")
            .header("accept", "application/json")
            .header("x-signature", &signature)
            .header("x-timestamp", "1")
            .body(Body::from(body.to_vec()))
            .unwrap();
        let stale_response = script_handler(State(app_state(compiled)), stale_request).await.into_response();
        assert_eq!(stale_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn script_webhook_rejects_replayed_delivery() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.webhook(
                    app.buildWebhook()
                        .path("/events")
                        .secret("topsecret")
                        .signatureHeader("x-signature")
                        .prefix("sha256=")
                        .replayHeader("x-delivery-id")
                        .replayTtlSeconds(300),
                    function(event, rc, prc) {
                        event.renderText("ok");
                    }
                );
                app.listen(8095);
            "#,
        )
        .unwrap();

        let compiled = Arc::new(compile_script_app(&script_path).await.unwrap());
        let body = br#"{"ok":true}"#;
        let signature = format!("sha256={}", test_hmac_sha256_hex("topsecret", body));

        let first_request = Request::builder()
            .method("POST")
            .uri("/events")
            .header("host", "localhost:8095")
            .header("x-signature", &signature)
            .header("x-delivery-id", "evt-123")
            .body(Body::from(body.to_vec()))
            .unwrap();
        let first_response = script_handler(State(app_state(compiled.clone())), first_request)
            .await
            .into_response();
        assert_eq!(first_response.status(), StatusCode::OK);

        let replay_request = Request::builder()
            .method("POST")
            .uri("/events")
            .header("host", "localhost:8095")
            .header("accept", "application/json")
            .header("x-signature", &signature)
            .header("x-delivery-id", "evt-123")
            .body(Body::from(body.to_vec()))
            .unwrap();
        let replay_response = script_handler(State(app_state(compiled)), replay_request)
            .await
            .into_response();
        assert_eq!(replay_response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn script_webhook_expires_replay_entries_after_ttl() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.webhook(
                    app.buildWebhook()
                        .path("/events")
                        .secret("topsecret")
                        .signatureHeader("x-signature")
                        .prefix("sha256=")
                        .replayHeader("x-delivery-id")
                        .replayTtlSeconds(60),
                    function(event, rc, prc) {
                        event.renderText("ok");
                    }
                );
                app.listen(8096);
            "#,
        )
        .unwrap();

        let compiled = Arc::new(compile_script_app(&script_path).await.unwrap());
        let body = br#"{"ok":true}"#;
        let signature = format!("sha256={}", test_hmac_sha256_hex("topsecret", body));
        let replay_key = "/events:evt-expired".to_string();
        let stale_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 120;
        compiled
            .webhook_replay_store
            .lock()
            .unwrap()
            .insert(replay_key, stale_time);

        let request = Request::builder()
            .method("POST")
            .uri("/events")
            .header("host", "localhost:8096")
            .header("x-signature", &signature)
            .header("x-delivery-id", "evt-expired")
            .body(Body::from(body.to_vec()))
            .unwrap();
        let response = script_handler(State(app_state(compiled)), request).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn script_serves_static_files_from_app_middleware() {
        let dir = tempfile::tempdir().unwrap();
        let public_dir = dir.path().join("public");
        std::fs::create_dir_all(&public_dir).unwrap();
        std::fs::write(public_dir.join("site.css"), "body { color: red; }").unwrap();

        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.use( app.middleware.buildStaticFiles( "/assets", "public" ) );
                app.get( "/health", function( event, rc, prc ) {
                    event.renderJson( { "ok": true } );
                } );
                app.listen( 8097 );
            "#,
        )
        .unwrap();

        let compiled = Arc::new(compile_script_app(&script_path).await.unwrap());

        let static_response = script_handler(
            State(app_state(compiled.clone())),
            Request::builder()
                .method("GET")
                .uri("/assets/site.css")
                .header("host", "localhost:8097")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .into_response();
        assert_eq!(static_response.status(), StatusCode::OK);
        assert_eq!(
            static_response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/css")
        );

        let health_response = script_handler(
            State(app_state(compiled)),
            Request::builder()
                .method("GET")
                .uri("/health")
                .header("host", "localhost:8097")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .into_response();
        assert_eq!(health_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn script_static_files_middleware_blocks_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let public_dir = dir.path().join("public");
        std::fs::create_dir_all(&public_dir).unwrap();
        std::fs::write(public_dir.join("site.css"), "body { color: red; }").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "secret").unwrap();

        let script_path = dir.path().join("app.bxs");
        std::fs::write(
            &script_path,
            r#"
                import boxlang.web;

                app = web.server();
                app.use( app.middleware.buildStaticFiles( "/assets", "public" ) );
                app.listen( 8098 );
            "#,
        )
        .unwrap();

        let compiled = Arc::new(compile_script_app(&script_path).await.unwrap());
        let response = script_handler(
            State(app_state(compiled)),
            Request::builder()
                .method("GET")
                .uri("/assets/../secret.txt")
                .header("host", "localhost:8098")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    fn test_hmac_sha256_hex(secret: &str, body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let bytes = mac.finalize().into_bytes();
        bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
    }
}
