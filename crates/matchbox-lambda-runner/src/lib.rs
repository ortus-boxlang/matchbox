use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose};
use matchbox_compiler::{
    ast::{Statement, StatementKind},
    parser,
};
use matchbox_vm::{
    types::{BxNativeObject, BxVM, BxValue},
    vm::VM,
};
use serde_json::Value as JsonValue;

pub mod deploy;
pub mod packaging;
pub mod runtime_api;

const DEFAULT_LAMBDA_FILE: &str = "Lambda.bx";

#[derive(Debug, Clone)]
pub struct LambdaRuntime {
    root: PathBuf,
    lambda_path: PathBuf,
    application_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct LambdaContextSeed {
    pub aws_request_id: String,
    pub function_name: String,
    pub function_version: String,
    pub invoked_function_arn: String,
    pub memory_limit_in_mb: i32,
    pub log_group_name: String,
    pub log_stream_name: String,
    pub remaining_time_in_millis: i32,
}

impl Default for LambdaContextSeed {
    fn default() -> Self {
        Self {
            aws_request_id: "local-request".to_string(),
            function_name: std::env::var("AWS_LAMBDA_FUNCTION_NAME")
                .unwrap_or_else(|_| "matchbox-local".to_string()),
            function_version: std::env::var("AWS_LAMBDA_FUNCTION_VERSION")
                .unwrap_or_else(|_| "$LATEST".to_string()),
            invoked_function_arn: std::env::var("AWS_LAMBDA_FUNCTION_ARN").unwrap_or_default(),
            memory_limit_in_mb: std::env::var("AWS_LAMBDA_FUNCTION_MEMORY_SIZE")
                .ok()
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(128),
            log_group_name: std::env::var("AWS_LAMBDA_LOG_GROUP_NAME").unwrap_or_default(),
            log_stream_name: std::env::var("AWS_LAMBDA_LOG_STREAM_NAME").unwrap_or_default(),
            remaining_time_in_millis: 30_000,
        }
    }
}

#[derive(Debug)]
struct LambdaContextObject {
    seed: LambdaContextSeed,
}

#[derive(Debug)]
struct LambdaLoggerObject;

impl LambdaRuntime {
    pub fn discover(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let (root, lambda_path) = if path.is_file() {
            let root = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            (root, path.to_path_buf())
        } else {
            let root = path.to_path_buf();
            let lambda_path = find_lambda_file(&root)?;
            (root, lambda_path)
        };
        let application_path = find_application_file(&root);

        Ok(Self {
            root,
            lambda_path,
            application_path,
        })
    }

    pub fn invoke_json(&self, event: JsonValue) -> Result<JsonValue> {
        self.invoke_json_with_context(event, LambdaContextSeed::default())
    }

    pub fn invoke_json_with_context(
        &self,
        event: JsonValue,
        context_seed: LambdaContextSeed,
    ) -> Result<JsonValue> {
        let target_path = self.resolve_invocation_class(&event);
        let target_class_name = class_name_from_path(&target_path)?;
        let source = std::fs::read_to_string(&target_path)
            .with_context(|| format!("failed to read {}", target_path.display()))?;

        let mut vm = VM::new();
        let application = if let Some(application_path) = &self.application_path {
            let source = std::fs::read_to_string(application_path)
                .with_context(|| format!("failed to read {}", application_path.display()))?;
            let chunk = compile_source(application_path, &source)?;
            vm.interpret(chunk)?;
            let application = vm.construct_global_class("Application", Vec::new())?;
            call_optional_method(&mut vm, application, "onApplicationStart", Vec::new())?;
            Some(application)
        } else {
            None
        };

        let chunk = compile_source(&target_path, &source)?;
        vm.interpret(chunk)?;

        let lambda = vm.construct_global_class(&target_class_name, Vec::new())?;
        let event = json_to_bx(&mut vm, &event)?;
        let context_id = vm.native_object_new(Rc::new(RefCell::new(LambdaContextObject {
            seed: context_seed,
        })));
        let context = BxValue::new_ptr(context_id);
        let response_id = default_response(&mut vm);
        let response = BxValue::new_ptr(response_id);

        let lambda_path =
            BxValue::new_ptr(vm.string_new(target_path.to_string_lossy().to_string()));
        if let Some(application) = application {
            call_optional_method(
                &mut vm,
                application,
                "onRequestStart",
                vec![lambda_path, event, context],
            )?;
        }

        let result = match vm.call_method_value(lambda, "run", vec![event, context, response]) {
            Ok(result) => result,
            Err(error) => {
                if let Some(application) = application {
                    let error_value = BxValue::new_ptr(vm.string_new(error.to_string()));
                    let event_name = BxValue::new_ptr(vm.string_new(String::new()));
                    let _ = call_optional_method(
                        &mut vm,
                        application,
                        "onError",
                        vec![error_value, event_name, event, context],
                    );
                    let _ = call_optional_method(
                        &mut vm,
                        application,
                        "onRequestEnd",
                        vec![lambda_path, event, context],
                    );
                }
                return Err(error);
            }
        };
        if let Some(application) = application {
            call_optional_method(
                &mut vm,
                application,
                "onRequestEnd",
                vec![lambda_path, event, context],
            )?;
        }
        let response = if is_response_struct(&vm, result) {
            result
        } else {
            if !result.is_null() {
                vm.struct_set(response_id, "body", result);
            }
            response
        };

        normalize_response(&vm, response)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn lambda_path(&self) -> &Path {
        &self.lambda_path
    }

    fn resolve_invocation_class(&self, event: &JsonValue) -> PathBuf {
        let Some(path) = extract_uri_path(event) else {
            return self.lambda_path.clone();
        };
        let Some(class_name) = class_name_from_uri_path(path) else {
            return self.lambda_path.clone();
        };

        for candidate in self.class_file_candidates(&class_name) {
            if candidate.is_file() {
                return candidate;
            }
        }

        self.lambda_path.clone()
    }

    fn class_file_candidates(&self, class_name: &str) -> Vec<PathBuf> {
        vec![
            self.root.join(class_name),
            self.root
                .join("src")
                .join("main")
                .join("bx")
                .join(class_name),
        ]
    }
}

impl BxNativeObject for LambdaContextObject {
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
    ) -> std::result::Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "getawsrequestid" => Ok(BxValue::new_ptr(
                vm.string_new(self.seed.aws_request_id.clone()),
            )),
            "getfunctionname" => Ok(BxValue::new_ptr(
                vm.string_new(self.seed.function_name.clone()),
            )),
            "getfunctionversion" => Ok(BxValue::new_ptr(
                vm.string_new(self.seed.function_version.clone()),
            )),
            "getinvokedfunctionarn" => Ok(BxValue::new_ptr(
                vm.string_new(self.seed.invoked_function_arn.clone()),
            )),
            "getmemorylimitinmb" => Ok(BxValue::new_int(self.seed.memory_limit_in_mb)),
            "getloggroupname" => Ok(BxValue::new_ptr(
                vm.string_new(self.seed.log_group_name.clone()),
            )),
            "getlogstreamname" => Ok(BxValue::new_ptr(
                vm.string_new(self.seed.log_stream_name.clone()),
            )),
            "getremainingtimeinmillis" => {
                Ok(BxValue::new_int(self.seed.remaining_time_in_millis.max(0)))
            }
            "getlogger" => {
                let logger_id = vm.native_object_new(Rc::new(RefCell::new(LambdaLoggerObject)));
                Ok(BxValue::new_ptr(logger_id))
            }
            "getclientcontext" | "getidentity" => {
                let id = vm.struct_new();
                Ok(BxValue::new_ptr(id))
            }
            _ => Err(format!("Method {} not found on Lambda context.", name)),
        }
    }
}

impl BxNativeObject for LambdaLoggerObject {
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
    ) -> std::result::Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "log" => {
                let message = args
                    .first()
                    .map(|value| vm.to_string(*value))
                    .unwrap_or_default();
                let level = args
                    .get(1)
                    .map(|value| vm.to_string(*value))
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "INFO".to_string());
                vm.write_output(&format!("[{}] {}\n", level, message));
                Ok(BxValue::new_null())
            }
            _ => Err(format!("Method {} not found on Lambda logger.", name)),
        }
    }
}

fn normalize_response(vm: &dyn BxVM, response: BxValue) -> Result<JsonValue> {
    let response_id = response
        .as_gc_id()
        .filter(|id| vm.is_struct_value(BxValue::new_ptr(*id)))
        .ok_or_else(|| anyhow!("Lambda response must be a struct"))?;

    let mut object = serde_json::Map::new();
    object.insert(
        "statusCode".to_string(),
        JsonValue::from(response_status_code(vm, response_id)),
    );
    object.insert(
        "headers".to_string(),
        JsonValue::Object(response_headers(vm, response_id)),
    );
    object.insert(
        "cookies".to_string(),
        JsonValue::Array(response_cookies(vm, response_id)),
    );

    let mut is_base64_encoded = response_bool(vm, response_id, "isBase64Encoded").unwrap_or(false);
    let body = vm.struct_get(response_id, "body");
    let body = if body.is_null() {
        String::new()
    } else if vm.is_bytes(body) {
        is_base64_encoded = true;
        general_purpose::STANDARD.encode(vm.to_bytes(body).map_err(|err| anyhow!(err))?)
    } else if vm.is_string_value(body) {
        vm.to_string(body)
    } else {
        serde_json::to_string(&bx_to_json(vm, body)?)?
    };
    object.insert("body".to_string(), JsonValue::String(body));
    object.insert(
        "isBase64Encoded".to_string(),
        JsonValue::Bool(is_base64_encoded),
    );

    Ok(JsonValue::Object(object))
}

fn response_status_code(vm: &dyn BxVM, response_id: usize) -> i64 {
    let value = vm.struct_get(response_id, "statusCode");
    if value.is_number() {
        return value.as_number() as i64;
    }
    vm.to_string(value).parse::<i64>().unwrap_or(200)
}

fn response_headers(vm: &dyn BxVM, response_id: usize) -> serde_json::Map<String, JsonValue> {
    let headers = vm.struct_get(response_id, "headers");
    let mut object = serde_json::Map::new();
    if let Some(headers_id) = headers
        .as_gc_id()
        .filter(|id| vm.is_struct_value(BxValue::new_ptr(*id)))
    {
        for key in vm.struct_key_array(headers_id) {
            object.insert(
                key.clone(),
                JsonValue::String(vm.to_string(vm.struct_get(headers_id, &key))),
            );
        }
    }
    if object.is_empty() {
        object.insert(
            "Content-Type".to_string(),
            JsonValue::String("application/json".to_string()),
        );
        object.insert(
            "Access-Control-Allow-Origin".to_string(),
            JsonValue::String("*".to_string()),
        );
    }
    object
}

fn response_cookies(vm: &dyn BxVM, response_id: usize) -> Vec<JsonValue> {
    let cookies = vm.struct_get(response_id, "cookies");
    let Some(cookies_id) = cookies
        .as_gc_id()
        .filter(|id| vm.is_array_value(BxValue::new_ptr(*id)))
    else {
        return Vec::new();
    };

    let mut values = Vec::new();
    for index in 0..vm.array_len(cookies_id) {
        values.push(JsonValue::String(
            vm.to_string(vm.array_get(cookies_id, index)),
        ));
    }
    values
}

fn response_bool(vm: &dyn BxVM, response_id: usize, key: &str) -> Option<bool> {
    let value = vm.struct_get(response_id, key);
    if value.is_null() {
        return None;
    }
    if value.is_bool() {
        return Some(value.as_bool());
    }
    Some(matches!(
        vm.to_string(value).to_lowercase().as_str(),
        "true" | "yes" | "1"
    ))
}

fn is_response_struct(vm: &dyn BxVM, value: BxValue) -> bool {
    let Some(id) = value.as_gc_id() else {
        return false;
    };
    if !vm.is_struct_value(value) {
        return false;
    }

    [
        "statusCode",
        "headers",
        "cookies",
        "body",
        "isBase64Encoded",
    ]
    .iter()
    .any(|key| vm.struct_key_exists(id, key))
}

fn extract_uri_path(event: &JsonValue) -> Option<&str> {
    let request_context = event.get("requestContext");

    if let Some(path) = request_context
        .and_then(|value| value.get("http"))
        .and_then(|value| value.get("path"))
        .and_then(JsonValue::as_str)
    {
        return Some(path);
    }

    if request_context
        .and_then(|value| value.get("domainName"))
        .is_some()
    {
        if let Some(path) = event.get("rawPath").and_then(JsonValue::as_str) {
            return Some(path);
        }
    }

    if let Some(path) = request_context
        .and_then(|value| value.get("resourcePath"))
        .and_then(JsonValue::as_str)
    {
        return Some(path);
    }

    if request_context.and_then(|value| value.get("elb")).is_some() {
        if let Some(path) = event.get("path").and_then(JsonValue::as_str) {
            return Some(path);
        }
    }

    event.get("path").and_then(JsonValue::as_str)
}

fn class_name_from_uri_path(path: &str) -> Option<String> {
    let segment = path.trim_start_matches('/').split('/').next()?.trim();
    if segment.is_empty() {
        return None;
    }
    let class_name = pascal_case(segment);
    if class_name.is_empty() {
        None
    } else {
        Some(format!("{class_name}.bx"))
    }
}

fn pascal_case(value: &str) -> String {
    let mut output = String::new();
    let mut uppercase_next = true;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                output.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                output.push(ch);
            }
        } else {
            uppercase_next = true;
        }
    }
    output
}

fn find_lambda_file(root: &Path) -> Result<PathBuf> {
    let root_lambda = root.join(DEFAULT_LAMBDA_FILE);
    if root_lambda.is_file() {
        return Ok(root_lambda);
    }

    let starter_lambda = root
        .join("src")
        .join("main")
        .join("bx")
        .join(DEFAULT_LAMBDA_FILE);
    if starter_lambda.is_file() {
        return Ok(starter_lambda);
    }

    bail!(
        "Lambda.bx not found in {} or {}",
        root_lambda.display(),
        starter_lambda.display()
    )
}

fn find_application_file(root: &Path) -> Option<PathBuf> {
    let root_application = root.join("Application.bx");
    if root_application.is_file() {
        return Some(root_application);
    }

    let starter_application = root
        .join("src")
        .join("main")
        .join("bx")
        .join("Application.bx");
    if starter_application.is_file() {
        return Some(starter_application);
    }

    None
}

fn class_name_from_path(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("failed to infer class name from {}", path.display()))
}

fn compile_source(path: &Path, source: &str) -> Result<matchbox_vm::Chunk> {
    let filename = path.to_string_lossy();
    let mut ast = parser::parse(source, Some(&filename))
        .map_err(|err| anyhow!("Parse Error in {}: {}", path.display(), err))?;
    infer_class_name_from_filename(&mut ast, path);
    let mut chunk = matchbox_compiler::compile_with_treeshaking(
        &filename,
        &ast,
        source,
        Vec::new(),
        false,
        false,
        &[],
        &[],
    )
    .map_err(|err| anyhow!("Compiler Error in {}: {}", path.display(), err))?;
    chunk.reconstruct_functions();
    Ok(chunk)
}

fn call_optional_method(
    vm: &mut VM,
    receiver: BxValue,
    name: &str,
    args: Vec<BxValue>,
) -> Result<Option<BxValue>> {
    match vm.call_method_value(receiver, name, args) {
        Ok(value) => Ok(Some(value)),
        Err(error)
            if error.to_string().contains("Method ")
                && error.to_string().contains(" not found on instance") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn infer_class_name_from_filename(ast: &mut [Statement], path: &Path) {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return;
    };
    for statement in ast {
        match &mut statement.kind {
            StatementKind::ClassDecl { name, .. } | StatementKind::InterfaceDecl { name, .. } => {
                if name.is_empty() {
                    *name = stem.to_string();
                }
            }
            _ => {}
        }
    }
}

fn default_response(vm: &mut VM) -> usize {
    let response_id = vm.struct_new();
    vm.struct_set(response_id, "statusCode", BxValue::new_int(200));

    let headers_id = vm.struct_new();
    let content_type = vm.string_new("application/json".to_string());
    vm.struct_set(headers_id, "Content-Type", BxValue::new_ptr(content_type));
    let cors_origin = vm.string_new("*".to_string());
    vm.struct_set(
        headers_id,
        "Access-Control-Allow-Origin",
        BxValue::new_ptr(cors_origin),
    );
    vm.struct_set(response_id, "headers", BxValue::new_ptr(headers_id));

    let body = vm.string_new(String::new());
    vm.struct_set(response_id, "body", BxValue::new_ptr(body));
    let cookies_id = vm.array_new();
    vm.struct_set(response_id, "cookies", BxValue::new_ptr(cookies_id));
    response_id
}

fn json_to_bx(vm: &mut dyn BxVM, value: &JsonValue) -> Result<BxValue> {
    match value {
        JsonValue::Null => Ok(BxValue::new_null()),
        JsonValue::Bool(value) => Ok(BxValue::new_bool(*value)),
        JsonValue::Number(value) => {
            if let Some(i) = value.as_i64() {
                if let Ok(i) = i32::try_from(i) {
                    Ok(BxValue::new_int(i))
                } else {
                    Ok(BxValue::new_number(i as f64))
                }
            } else {
                Ok(BxValue::new_number(
                    value
                        .as_f64()
                        .ok_or_else(|| anyhow!("unsupported JSON number"))?,
                ))
            }
        }
        JsonValue::String(value) => Ok(BxValue::new_ptr(vm.string_new(value.clone()))),
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

fn bx_to_json(vm: &dyn BxVM, value: BxValue) -> Result<JsonValue> {
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
            vm.to_bytes(value)
                .map_err(|err| anyhow!(err))?
                .into_iter()
                .map(JsonValue::from)
                .collect(),
        ));
    }
    if vm.is_array_value(value) {
        let id = value
            .as_gc_id()
            .ok_or_else(|| anyhow!("array value does not have a GC id"))?;
        let mut items = Vec::new();
        for index in 0..vm.array_len(id) {
            items.push(bx_to_json(vm, vm.array_get(id, index))?);
        }
        return Ok(JsonValue::Array(items));
    }
    if vm.is_struct_value(value) {
        let id = value
            .as_gc_id()
            .ok_or_else(|| anyhow!("struct value does not have a GC id"))?;
        let mut object = serde_json::Map::new();
        for key in vm.struct_key_array(id) {
            object.insert(key.clone(), bx_to_json(vm, vm.struct_get(id, &key))?);
        }
        return Ok(JsonValue::Object(object));
    }
    Ok(JsonValue::String(vm.to_string(value)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn invokes_root_lambda_that_mutates_response() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        response.body = event.message;
                        response.statusCode = 201;
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime.invoke_json(json!({ "message": "hello" })).unwrap();

        assert_eq!(response["statusCode"], 201);
        assert_eq!(response["body"], "hello");
        assert_eq!(response["headers"]["Content-Type"], "application/json");
        assert_eq!(response["cookies"], json!([]));
        assert_eq!(response["isBase64Encoded"], false);
    }

    #[test]
    fn invokes_starter_layout_lambda_that_returns_value() {
        let dir = tempfile::tempdir().unwrap();
        let bx_dir = dir.path().join("src").join("main").join("bx");
        std::fs::create_dir_all(&bx_dir).unwrap();
        std::fs::write(
            bx_dir.join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        return event.message;
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime
            .invoke_json(json!({ "message": "returned" }))
            .unwrap();

        assert_eq!(response["statusCode"], 200);
        assert_eq!(response["body"], "returned");
    }

    #[test]
    fn json_encodes_non_string_body_values() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        response.body = {
                            ok: true,
                            count: 2
                        };
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime.invoke_json(json!({})).unwrap();

        assert_eq!(response["statusCode"], 200);
        assert_eq!(response["body"], r#"{"count":2.0,"ok":true}"#);
        assert_eq!(response["isBase64Encoded"], false);
    }

    #[test]
    fn full_response_return_replaces_default_response() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        return {
                            statusCode: 202,
                            headers: {
                                "x-test": "yes"
                            },
                            cookies: [ "a=b" ],
                            body: {
                                accepted: true
                            },
                            isBase64Encoded: false
                        };
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime.invoke_json(json!({})).unwrap();

        assert_eq!(response["statusCode"], 202);
        assert_eq!(response["headers"], json!({ "x-test": "yes" }));
        assert_eq!(response["cookies"], json!(["a=b"]));
        assert_eq!(response["body"], r#"{"accepted":true}"#);
        assert_eq!(response["isBase64Encoded"], false);
    }

    #[test]
    fn byte_body_is_base64_encoded() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        response.body = bytesNew([1, 2, 3]);
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime.invoke_json(json!({})).unwrap();

        assert_eq!(response["body"], "AQID");
        assert_eq!(response["isBase64Encoded"], true);
    }

    #[test]
    fn exposes_lambda_context_methods() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        response.body = {
                            requestId: context.getAwsRequestId(),
                            functionName: context.getFunctionName(),
                            functionVersion: context.getFunctionVersion(),
                            invokedArn: context.getInvokedFunctionArn(),
                            memory: context.getMemoryLimitInMB(),
                            logGroup: context.getLogGroupName(),
                            logStream: context.getLogStreamName(),
                            remaining: context.getRemainingTimeInMillis()
                        };
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime
            .invoke_json_with_context(
                json!({}),
                LambdaContextSeed {
                    aws_request_id: "req-123".to_string(),
                    function_name: "fn".to_string(),
                    function_version: "42".to_string(),
                    invoked_function_arn: "arn:aws:lambda:test".to_string(),
                    memory_limit_in_mb: 256,
                    log_group_name: "/aws/lambda/fn".to_string(),
                    log_stream_name: "stream".to_string(),
                    remaining_time_in_millis: 1500,
                },
            )
            .unwrap();

        let body: JsonValue = serde_json::from_str(response["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["requestId"], "req-123");
        assert_eq!(body["functionName"], "fn");
        assert_eq!(body["functionVersion"], "42");
        assert_eq!(body["invokedArn"], "arn:aws:lambda:test");
        assert_eq!(body["memory"], 256);
        assert_eq!(body["logGroup"], "/aws/lambda/fn");
        assert_eq!(body["logStream"], "stream");
        assert_eq!(body["remaining"], 1500);
    }

    #[test]
    fn exposes_lambda_logger_object() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        context.getLogger().log("hello");
                        context.getLogger().log("bad", "ERROR");
                        response.body = "logged";
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime.invoke_json(json!({})).unwrap();

        assert_eq!(response["body"], "logged");
    }

    #[test]
    fn application_request_hooks_wrap_lambda_invocation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Application.bx"),
            r#"
                class {
                    function onApplicationStart() {
                        return true;
                    }

                    function onRequestStart(lambdaPath, event, context) {
                        event.started = true;
                        event.lambdaPath = lambdaPath;
                    }

                    function onRequestEnd(lambdaPath, event, context) {
                        event.ended = true;
                    }
                }
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        response.body = event;
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime.invoke_json(json!({ "name": "test" })).unwrap();
        let body: JsonValue = serde_json::from_str(response["body"].as_str().unwrap()).unwrap();

        assert_eq!(body["name"], "test");
        assert_eq!(body["started"], true);
        assert_eq!(body["ended"], true);
        assert!(body["lambdaPath"].as_str().unwrap().ends_with("Lambda.bx"));
    }

    #[test]
    fn missing_application_hooks_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Application.bx"), "class {}").unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        return "ok";
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime.invoke_json(json!({})).unwrap();

        assert_eq!(response["body"], "ok");
    }

    #[test]
    fn application_on_error_observes_lambda_failure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Application.bx"),
            r#"
                class {
                    function onError(exception, eventName, event, context) {
                        context.getLogger().log("saw error", "ERROR");
                    }
                }
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        throw "boom";
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let error = runtime.invoke_json(json!({})).unwrap_err().to_string();

        assert!(error.contains("boom"));
    }

    #[test]
    fn routes_api_gateway_v2_path_to_matching_class() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        return "fallback";
                    }
                }
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Products.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        return "products";
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime
            .invoke_json(json!({
                "requestContext": {
                    "http": {
                        "path": "/products/123"
                    }
                }
            }))
            .unwrap();

        assert_eq!(response["body"], "products");
    }

    #[test]
    fn routes_function_url_raw_path_to_matching_class() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        return "fallback";
                    }
                }
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("OrderItems.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        return "orders";
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let response = runtime
            .invoke_json(json!({
                "rawPath": "/order-items/42",
                "requestContext": {
                    "domainName": "abc.lambda-url.us-east-1.on.aws"
                }
            }))
            .unwrap();

        assert_eq!(response["body"], "orders");
    }

    #[test]
    fn routes_starter_layout_class_and_falls_back_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let bx_dir = dir.path().join("src").join("main").join("bx");
        std::fs::create_dir_all(&bx_dir).unwrap();
        std::fs::write(
            bx_dir.join("Lambda.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        return "fallback";
                    }
                }
            "#,
        )
        .unwrap();
        std::fs::write(
            bx_dir.join("Customers.bx"),
            r#"
                class {
                    function run(event, context, response) {
                        return "customers";
                    }
                }
            "#,
        )
        .unwrap();

        let runtime = LambdaRuntime::discover(dir.path()).unwrap();
        let routed = runtime
            .invoke_json(json!({
                "requestContext": {
                    "resourcePath": "/customers/{id}"
                }
            }))
            .unwrap();
        let fallback = runtime
            .invoke_json(json!({
                "requestContext": {
                    "elb": {}
                },
                "path": "/missing"
            }))
            .unwrap();

        assert_eq!(routed["body"], "customers");
        assert_eq!(fallback["body"], "fallback");
    }
}
