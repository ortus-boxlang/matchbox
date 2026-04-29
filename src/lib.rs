use matchbox_compiler::{ast, compiler, parser};
use matchbox_utility::enable_logging;
use matchbox_vm::{Chunk, types, vm};

use anyhow::{Context, Result, bail};
use colored::*;
use std::collections::HashMap;
use std::env as std_env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(target_arch = "wasm32")]
use js_sys::{Array, Error, Function, Promise};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;
#[cfg(target_arch = "wasm32")]
use web_sys::window;

const MAGIC_FOOTER: &[u8; 8] = b"BOXLANG\x01";
const WASI_HTTP_EMBED_MAGIC: &[u8; 8] = b"MBWH\0\0\0\x01";
const WASI_HTTP_EMBED_CAPACITY: usize = 4 * 1024 * 1024;
const WASI_HTTP_EMBED_DATA_CAPACITY: usize = WASI_HTTP_EMBED_CAPACITY - 12;
const ESP32_PARTITIONS_CSV: &str = include_str!("../crates/matchbox-esp32-runner/partitions.csv");
const ESP32_STORAGE_OFFSET: &str = "0x310000";
static FUSION_BUILD_COUNTER: AtomicUsize = AtomicUsize::new(0);

mod browser;
mod embedded;
mod modules;
mod stubs;

use postcard;

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Esp32BoxConfig {
    board: Option<String>,
    psram: Option<bool>,
    web_port: Option<u16>,
    wifi: Option<Esp32WifiConfig>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Esp32WifiConfig {
    ssid: Option<String>,
    password: Option<String>,
    hostname: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoxProjectConfig {
    esp32: Option<Esp32BoxConfig>,
}

fn read_esp32_box_config(project_dir: &Path) -> Result<Option<Esp32BoxConfig>> {
    let box_json_path = project_dir.join("box.json");
    if !box_json_path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(&box_json_path)?;
    let config: BoxProjectConfig = serde_json::from_str(&text)?;
    Ok(config.esp32)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Esp32UnsupportedFeature {
    line: u32,
    feature: &'static str,
    guidance: &'static str,
}

fn validate_esp32_target(ast: &[ast::Statement], embedded_web_enabled: bool) -> Result<()> {
    let mut findings = Vec::new();
    for stmt in ast {
        collect_esp32_unsupported_features_in_stmt(stmt, &mut findings, embedded_web_enabled);
    }

    if findings.is_empty() {
        return Ok(());
    }

    findings.sort_by_key(|finding| finding.line);
    findings.dedup();

    let mut message = String::from(
        "ESP32 currently supports only the lean routed subset of `web.server()`.\n\
Unsupported features were found:\n",
    );
    for finding in findings {
        message.push_str(&format!(
            "  line {}: {}. {}\n",
            finding.line, finding.feature, finding.guidance
        ));
    }
    message.push_str(
        "Allowed direction for now: route registration, lean middleware definitions, and in-handler request/response helpers such as `event.renderJson()` and `event.renderHtml()`. \
The embedded HTTP transport for `listen()` and heavier server features is not implemented yet.",
    );
    bail!(message)
}

fn collect_esp32_unsupported_features_in_stmt(
    stmt: &ast::Statement,
    findings: &mut Vec<Esp32UnsupportedFeature>,
    embedded_web_enabled: bool,
) {
    use ast::StatementKind;

    match &stmt.kind {
        StatementKind::Import { .. } | StatementKind::Continue | StatementKind::Break => {}
        StatementKind::ClassDecl { members, .. } => {
            for member in members {
                if let ast::ClassMember::Statement(statement) = member {
                    collect_esp32_unsupported_features_in_stmt(
                        statement,
                        findings,
                        embedded_web_enabled,
                    );
                }
            }
        }
        StatementKind::InterfaceDecl { members, .. } => {
            for member in members {
                collect_esp32_unsupported_features_in_stmt(member, findings, embedded_web_enabled);
            }
        }
        StatementKind::FunctionDecl { body, .. } => {
            collect_esp32_unsupported_features_in_function_body(
                body,
                findings,
                embedded_web_enabled,
            );
        }
        StatementKind::ForLoop {
            collection, body, ..
        } => {
            collect_esp32_unsupported_features_in_expr(collection, findings, embedded_web_enabled);
            for statement in body {
                collect_esp32_unsupported_features_in_stmt(
                    statement,
                    findings,
                    embedded_web_enabled,
                );
            }
        }
        StatementKind::ForClassic {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                collect_esp32_unsupported_features_in_stmt(init, findings, embedded_web_enabled);
            }
            if let Some(condition) = condition {
                collect_esp32_unsupported_features_in_expr(
                    condition,
                    findings,
                    embedded_web_enabled,
                );
            }
            if let Some(update) = update {
                collect_esp32_unsupported_features_in_expr(update, findings, embedded_web_enabled);
            }
            for statement in body {
                collect_esp32_unsupported_features_in_stmt(
                    statement,
                    findings,
                    embedded_web_enabled,
                );
            }
        }
        StatementKind::WhileLoop { condition, body } => {
            collect_esp32_unsupported_features_in_expr(condition, findings, embedded_web_enabled);
            for statement in body {
                collect_esp32_unsupported_features_in_stmt(
                    statement,
                    findings,
                    embedded_web_enabled,
                );
            }
        }
        StatementKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_esp32_unsupported_features_in_expr(condition, findings, embedded_web_enabled);
            for statement in then_branch {
                collect_esp32_unsupported_features_in_stmt(
                    statement,
                    findings,
                    embedded_web_enabled,
                );
            }
            if let Some(else_branch) = else_branch {
                for statement in else_branch {
                    collect_esp32_unsupported_features_in_stmt(
                        statement,
                        findings,
                        embedded_web_enabled,
                    );
                }
            }
        }
        StatementKind::Switch {
            value,
            cases,
            default_case,
        } => {
            collect_esp32_unsupported_features_in_expr(value, findings, embedded_web_enabled);
            for case in cases {
                collect_esp32_unsupported_features_in_expr(
                    &case.value,
                    findings,
                    embedded_web_enabled,
                );
                for statement in &case.body {
                    collect_esp32_unsupported_features_in_stmt(
                        statement,
                        findings,
                        embedded_web_enabled,
                    );
                }
            }
            if let Some(default_case) = default_case {
                for statement in default_case {
                    collect_esp32_unsupported_features_in_stmt(
                        statement,
                        findings,
                        embedded_web_enabled,
                    );
                }
            }
        }
        StatementKind::Return(expr) | StatementKind::Throw(expr) => {
            if let Some(expr) = expr {
                collect_esp32_unsupported_features_in_expr(expr, findings, embedded_web_enabled);
            }
        }
        StatementKind::TryCatch {
            try_branch,
            catches,
            finally_branch,
        } => {
            for statement in try_branch {
                collect_esp32_unsupported_features_in_stmt(
                    statement,
                    findings,
                    embedded_web_enabled,
                );
            }
            for catch in catches {
                for statement in &catch.body {
                    collect_esp32_unsupported_features_in_stmt(
                        statement,
                        findings,
                        embedded_web_enabled,
                    );
                }
            }
            if let Some(finally_branch) = finally_branch {
                for statement in finally_branch {
                    collect_esp32_unsupported_features_in_stmt(
                        statement,
                        findings,
                        embedded_web_enabled,
                    );
                }
            }
        }
        StatementKind::VariableDecl { value, .. } | StatementKind::Expression(value) => {
            collect_esp32_unsupported_features_in_expr(value, findings, embedded_web_enabled);
        }
    }
}

fn collect_esp32_unsupported_features_in_function_body(
    body: &ast::FunctionBody,
    findings: &mut Vec<Esp32UnsupportedFeature>,
    embedded_web_enabled: bool,
) {
    match body {
        ast::FunctionBody::Block(statements) => {
            for statement in statements {
                collect_esp32_unsupported_features_in_stmt(
                    statement,
                    findings,
                    embedded_web_enabled,
                );
            }
        }
        ast::FunctionBody::Expression(expr) => {
            collect_esp32_unsupported_features_in_expr(expr, findings, embedded_web_enabled);
        }
        ast::FunctionBody::Abstract => {}
    }
}

fn collect_esp32_unsupported_features_in_expr(
    expr: &ast::Expression,
    findings: &mut Vec<Esp32UnsupportedFeature>,
    embedded_web_enabled: bool,
) {
    use ast::{ExpressionKind, Literal, StringPart};

    match &expr.kind {
        ExpressionKind::New { args, .. } => {
            for arg in args {
                collect_esp32_unsupported_features_in_expr(
                    &arg.value,
                    findings,
                    embedded_web_enabled,
                );
            }
        }
        ExpressionKind::Assignment { target, value } => {
            collect_esp32_unsupported_features_in_target(target, findings, embedded_web_enabled);
            collect_esp32_unsupported_features_in_expr(value, findings, embedded_web_enabled);
        }
        ExpressionKind::Binary { left, right, .. } | ExpressionKind::Elvis { left, right } => {
            collect_esp32_unsupported_features_in_expr(left, findings, embedded_web_enabled);
            collect_esp32_unsupported_features_in_expr(right, findings, embedded_web_enabled);
        }
        ExpressionKind::UnaryNot(inner) | ExpressionKind::Postfix { base: inner, .. } => {
            collect_esp32_unsupported_features_in_expr(inner, findings, embedded_web_enabled);
        }
        ExpressionKind::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_esp32_unsupported_features_in_expr(condition, findings, embedded_web_enabled);
            collect_esp32_unsupported_features_in_expr(then_expr, findings, embedded_web_enabled);
            collect_esp32_unsupported_features_in_expr(else_expr, findings, embedded_web_enabled);
        }
        ExpressionKind::FunctionCall { base, args } => {
            if let Some(feature) = classify_esp32_unsupported_call(base, embedded_web_enabled) {
                findings.push(Esp32UnsupportedFeature {
                    line: expr.line,
                    feature: feature.0,
                    guidance: feature.1,
                });
            }
            collect_esp32_unsupported_features_in_expr(base, findings, embedded_web_enabled);
            for arg in args {
                collect_esp32_unsupported_features_in_expr(
                    &arg.value,
                    findings,
                    embedded_web_enabled,
                );
            }
        }
        ExpressionKind::ArrayAccess { base, index } => {
            collect_esp32_unsupported_features_in_expr(base, findings, embedded_web_enabled);
            collect_esp32_unsupported_features_in_expr(index, findings, embedded_web_enabled);
        }
        ExpressionKind::MemberAccess { base, .. }
        | ExpressionKind::SafeMemberAccess { base, .. } => {
            collect_esp32_unsupported_features_in_expr(base, findings, embedded_web_enabled);
        }
        ExpressionKind::Prefix { target, .. } => {
            collect_esp32_unsupported_features_in_target(target, findings, embedded_web_enabled);
        }
        ExpressionKind::Identifier(_) => {}
        ExpressionKind::Literal(literal) => match literal {
            Literal::String(parts) => {
                for part in parts {
                    if let StringPart::Expression(inner) = part {
                        collect_esp32_unsupported_features_in_expr(
                            inner,
                            findings,
                            embedded_web_enabled,
                        );
                    }
                }
            }
            Literal::Array(items) => {
                for item in items {
                    collect_esp32_unsupported_features_in_expr(
                        item,
                        findings,
                        embedded_web_enabled,
                    );
                }
            }
            Literal::Struct(entries) => {
                for (key, value) in entries {
                    collect_esp32_unsupported_features_in_expr(key, findings, embedded_web_enabled);
                    collect_esp32_unsupported_features_in_expr(
                        value,
                        findings,
                        embedded_web_enabled,
                    );
                }
            }
            Literal::Function { body, .. } => {
                collect_esp32_unsupported_features_in_function_body(
                    body,
                    findings,
                    embedded_web_enabled,
                );
            }
            Literal::Number(_) | Literal::Boolean(_) | Literal::Null => {}
        },
    }
}

fn collect_esp32_unsupported_features_in_target(
    target: &ast::AssignmentTarget,
    findings: &mut Vec<Esp32UnsupportedFeature>,
    embedded_web_enabled: bool,
) {
    match target {
        ast::AssignmentTarget::Identifier(_) => {}
        ast::AssignmentTarget::Member { base, .. } => {
            collect_esp32_unsupported_features_in_expr(base, findings, embedded_web_enabled);
        }
        ast::AssignmentTarget::Index { base, index } => {
            collect_esp32_unsupported_features_in_expr(base, findings, embedded_web_enabled);
            collect_esp32_unsupported_features_in_expr(index, findings, embedded_web_enabled);
        }
    }
}

fn classify_esp32_unsupported_call(
    base: &ast::Expression,
    embedded_web_enabled: bool,
) -> Option<(&'static str, &'static str)> {
    let chain = member_call_chain(base)?;
    if !embedded_web_enabled
        && chain.len() == 2
        && chain[0].eq_ignore_ascii_case("web")
        && chain[1].eq_ignore_ascii_case("server")
    {
        return Some((
            "`web.server()`",
            "ESP32 routed web support is behind `--esp32-web`. Rebuild with that flag to opt into the embedded web runtime.",
        ));
    }
    let method = chain.last()?.to_ascii_lowercase();

    match method.as_str() {
        "listen" => Some((
            "`app.listen()`",
            "Embedded HTTP transport is not implemented yet, so ESP32 builds cannot start the app server listener.",
        )),
        "setview" | "rendertemplate" => Some((
            "`event.setView()` / `event.renderTemplate()`",
            "ESP32 builds do not support filesystem-backed template rendering.",
        )),
        "buildstaticfiles" => Some((
            "`app.middleware.buildStaticFiles()`",
            "ESP32 builds do not support filesystem-backed static asset mounts.",
        )),
        "buildwebhook" | "webhook" => Some((
            "webhook registration",
            "Webhook helpers are native-server only right now and depend on the full HTTP server runtime.",
        )),
        "gethttpcookie" | "getcookie" | "sethttpcookie" | "setcookie" => Some((
            "cookie helpers",
            "Cookie helpers are not supported in the lean ESP32 app-server subset yet.",
        )),
        "getsession" | "getsessionvalue" | "paramsessionvalue" | "setsessionvalue"
        | "sessionexists" | "clearsession" | "invalidatesession" => Some((
            "session helpers",
            "Sessions are not supported in the lean ESP32 app-server subset yet.",
        )),
        _ => None,
    }
}

fn member_call_chain(expr: &ast::Expression) -> Option<Vec<&str>> {
    use ast::ExpressionKind;

    match &expr.kind {
        ExpressionKind::Identifier(name) => Some(vec![name.as_str()]),
        ExpressionKind::MemberAccess { base, member }
        | ExpressionKind::SafeMemberAccess { base, member } => {
            let mut chain = member_call_chain(base)?;
            chain.push(member.as_str());
            Some(chain)
        }
        _ => None,
    }
}

fn render_esp32_fusion_runner_source(registration_calls: &str) -> String {
    include_str!("esp32_fusion_runner_template.rs.txt")
        .replace("{{registration_calls}}", registration_calls)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_boxlang_bytecode(bytes: &[u8]) -> String {
    let res = (|| -> Result<String> {
        let mut chunk: Chunk = postcard::from_bytes(bytes)?;
        chunk.reconstruct_functions();
        let mut vm = vm::VM::new();
        let val = vm.interpret(chunk)?;
        Ok(val.to_string())
    })();

    match res {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_boxlang(source: &str) -> String {
    let res = (|| -> Result<String> {
        let ast = parser::parse(source, Some("wasm_input"))?;
        let mut compiler = compiler::Compiler::new("wasm_input");
        let chunk = compiler.compile(&ast, source)?;
        let mut vm = vm::VM::new();
        let val = vm.interpret(chunk)?;
        Ok(val.to_string())
    })();

    match res {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct BoxLangVM {
    vm: vm::VM,
    chunk: Option<Rc<RefCell<Chunk>>>,
}

#[cfg(target_arch = "wasm32")]
fn as_wasm_js_error(message: impl Into<String>) -> JsValue {
    Error::new(&message.into()).into()
}

#[cfg(target_arch = "wasm32")]
async fn yield_to_browser_host() -> Result<(), JsValue> {
    let promise = Promise::new(&mut |resolve: Function, reject: Function| {
        let win = match window() {
            Some(win) => win,
            None => {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("window is unavailable"));
                return;
            }
        };

        if let Err(err) = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0) {
            let _ = reject.call1(&JsValue::NULL, &err);
        }
    });

    let _ = JsFuture::from(promise).await?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BoxLangVM {
    #[wasm_bindgen(constructor)]
    pub fn new() -> BoxLangVM {
        BoxLangVM {
            vm: vm::VM::new(),
            chunk: None,
        }
    }

    pub fn load_bytecode(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let res = (|| -> Result<()> {
            let mut chunk: Chunk = postcard::from_bytes(bytes)?;
            chunk.reconstruct_functions();
            let chunk_rc = Rc::new(RefCell::new(chunk.clone()));
            self.chunk = Some(chunk_rc.clone());
            self.vm.interpret(chunk)?;
            Ok(())
        })();

        res.map_err(|e| js_sys::Error::new(&format!("Error: {}", e)).into())
    }

    pub fn vm_ptr(&self) -> usize {
        &self.vm as *const vm::VM as usize
    }

    pub fn pump(&mut self) -> Result<(), JsValue> {
        self.vm
            .pump_until_blocked()
            .map_err(|e| as_wasm_js_error(format!("Error: {}", e)))
    }

    pub async fn call(&mut self, name: &str, args: Array) -> Result<JsValue, JsValue> {
        let mut bx_args = Vec::new();
        for i in 0..args.length() {
            bx_args.push(self.vm.js_to_bx(args.get(i)));
        }

        let func = self
            .vm
            .get_global(name)
            .ok_or_else(|| as_wasm_js_error(format!("Function {} not found", name)))?;

        let future = self
            .vm
            .start_call_function_value(func, bx_args)
            .map_err(|e| as_wasm_js_error(format!("Error: {}", e)))?;

        loop {
            self.vm
                .pump_until_blocked()
                .map_err(|e| as_wasm_js_error(format!("Error: {}", e)))?;

            match self
                .vm
                .future_state(future)
                .map_err(|e| as_wasm_js_error(format!("Error: {}", e)))?
            {
                vm::HostFutureState::Pending => yield_to_browser_host().await?,
                vm::HostFutureState::Completed(value) => return Ok(self.vm.bx_to_js(&value)),
                vm::HostFutureState::Failed(error) => {
                    let msg = self.vm.format_error_value(error);
                    return Err(as_wasm_js_error(msg));
                }
            }
        }
    }
}

pub fn run() -> Result<()> {
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let _ = std::fs::write("panic_debug.txt", format!("{:#?}\n\n{backtrace}", info));
    }));
    // 1. Check for WASM custom section first
    #[cfg(target_arch = "wasm32")]
    if let Ok(chunk) = load_wasm_custom_section() {
        return run_chunk(chunk, &[]);
    }

    // 2. Check for embedded bytecode at end of binary (Native)
    if let Ok(embedded_chunk) = load_embedded_bytecode() {
        return run_chunk(embedded_chunk, &[]);
    }

    let args: Vec<String> = std_env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("esp32-doctor") {
        run_esp32_doctor()?;
        return Ok(());
    }
    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        print_usage();
        return Ok(());
    }

    if args.contains(&"--version".to_string()) || args.contains(&"-v".to_string()) {
        print_version();
        return Ok(());
    }

    if args.contains(&"--verbose".to_string()) {
        enable_logging();
    }

    let is_build = args.contains(&"--build".to_string());
    let mut is_flash = args.contains(&"--flash".to_string());
    let is_full_flash = args.contains(&"--full-flash".to_string());
    let is_watch = args.contains(&"--watch".to_string());
    let esp32_web = args.contains(&"--esp32-web".to_string());
    if is_full_flash || is_watch {
        is_flash = true;
    }
    let mut is_fast_deploy = args.contains(&"--fast-deploy".to_string());
    let chip = if let Some(idx) = args.iter().position(|a| a == "--chip") {
        args.get(idx + 1).map(|s| s.as_str())
    } else {
        None
    };
    let target = if let Some(idx) = args.iter().position(|a| a == "--target") {
        args.get(idx + 1).map(|s| s.as_str())
    } else {
        None
    };

    // For ESP32, --flash defaults to fast deploy unless --full-flash is present.
    if target == Some("esp32") && is_flash && !is_full_flash {
        is_fast_deploy = true;
    }

    let no_shaking = args.contains(&"--no-shaking".to_string());
    let no_std_lib = args.contains(&"--no-std-lib".to_string());
    let strip_source = args.contains(&"--strip-source".to_string());
    let is_serve = args.contains(&"--serve".to_string());
    let webroot = if let Some(idx) = args.iter().position(|a| a == "--webroot") {
        args.get(idx + 1)
            .cloned()
            .unwrap_or_else(|| ".".to_string())
    } else {
        ".".to_string()
    };

    if is_serve {
        #[cfg(feature = "server")]
        {
            let port = if let Some(idx) = args.iter().position(|a| a == "--port") {
                args.get(idx + 1)
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(8080)
            } else {
                8080
            };
            let host = if let Some(idx) = args.iter().position(|a| a == "--host") {
                args.get(idx + 1)
                    .cloned()
                    .unwrap_or_else(|| "127.0.0.1".to_string())
            } else {
                "127.0.0.1".to_string()
            };
            let server_args = matchbox_server::Args {
                port,
                host,
                webroot,
                config: None,
                app: None,
            };

            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                matchbox_server::run_server(server_args).await;
            });
            return Ok(());
        }
        #[cfg(not(feature = "server"))]
        {
            bail!("Server feature not enabled in this build.");
        }
    }

    let mut keep_symbols = Vec::new();
    if let Some(idx) = args.iter().position(|a| a == "--keep") {
        if let Some(val) = args.get(idx + 1) {
            keep_symbols = val.split(',').map(|s| s.trim().to_string()).collect();
        }
    }

    let output: Option<PathBuf> = if let Some(idx) = args.iter().position(|a| a == "--output") {
        args.get(idx + 1).map(|s| PathBuf::from(s))
    } else {
        None
    };

    if target == Some("wasi-http") {
        let out_path = output
            .clone()
            .unwrap_or_else(|| PathBuf::from(&webroot).with_extension("wasm"));
        produce_wasi_http_webroot_artifact(Path::new(&webroot), &out_path)?;
        return Ok(());
    }

    // Collect --module <path> flags.
    let extra_module_paths: Vec<PathBuf> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--module")
        .filter_map(|(i, _)| args.get(i + 1).map(|p| PathBuf::from(p)))
        .collect();

    let filename = args.iter().skip(1).find(|a| {
        !a.starts_with("--")
            && *a != "native"
            && *a != "wasm"
            && *a != "wasi"
            && *a != "wasi-http"
            && *a != "js"
            && *a != "esp32"
            && (args
                .iter()
                .position(|arg| arg == *a)
                .map(|pos| pos == 0 || args[pos - 1] != "--target")
                .unwrap_or(true))
            && (args
                .iter()
                .position(|arg| arg == *a)
                .map(|pos| pos == 0 || args[pos - 1] != "--keep")
                .unwrap_or(true))
            && (args
                .iter()
                .position(|arg| arg == *a)
                .map(|pos| pos == 0 || args[pos - 1] != "--output")
                .unwrap_or(true))
            && (args
                .iter()
                .position(|arg| arg == *a)
                .map(|pos| pos == 0 || args[pos - 1] != "--module")
                .unwrap_or(true))
            && (args
                .iter()
                .position(|arg| arg == *a)
                .map(|pos| pos == 0 || args[pos - 1] != "--chip")
                .unwrap_or(true))
    });

    match filename {
        Some(name) => {
            let path = Path::new(name);
            if path.is_dir() {
                process_directory(
                    path,
                    is_build,
                    target,
                    keep_symbols,
                    no_shaking,
                    no_std_lib,
                    strip_source,
                    output.as_deref(),
                    &extra_module_paths,
                    is_flash,
                    chip,
                    is_fast_deploy,
                    is_watch,
                    is_full_flash,
                    esp32_web,
                )?;
            } else {
                process_file(
                    path,
                    is_build,
                    target,
                    keep_symbols,
                    no_shaking,
                    no_std_lib,
                    strip_source,
                    output.as_deref(),
                    &extra_module_paths,
                    is_flash,
                    chip,
                    is_fast_deploy,
                    is_watch,
                    is_full_flash,
                    esp32_web,
                )?;
            }
        }
        None => {
            if is_build || target.is_some() {
                bail!("No filename provided for build/target");
            }
            run_repl()?;
        }
    }

    Ok(())
}

fn print_usage() {
    println!("Usage: matchbox [options] [file.bxs|file.bxb|directory]");
    println!("       matchbox esp32-doctor");
    println!("\nOptions:");
    println!("  -h, --help          Show this help message");
    println!("  -v, --version       Show version information");
    println!("  --verbose           Emit verbose build logging");
    println!("  --build             Compile to bytecode (.bxb)");
    println!("  --target <native>   Produce a standalone native binary");
    println!("  --target <wasi>     Produce a standalone WASI container binary");
    println!("  --target <wasi-http> Produce a WASI HTTP component from --webroot");
    println!("  --target <wasm>     Produce a standalone WASM binary (Web)");
    println!("  --target <js>       Produce a JavaScript module wrapper");
    println!("  --target <esp32>    Produce an ESP32 firmware binary");
    println!("  --esp32-web         Enable the embedded routed web runtime build flavor for ESP32");
    println!(
        "  --flash             Flash to device. Defaults to fast bytecode-only flash for ESP32."
    );
    println!("  --full-flash        Force a full firmware flash (includes VM/Runner)");
    println!(
        "  --fast-deploy       (Deprecated) Use --flash instead, which is now fast by default."
    );
    println!("  --chip <name>       The target ESP32 chip (e.g. esp32, esp32s3, esp32c3)");
    println!("  --keep <symbols>    Comma-separated list of BIFs to preserve");
    println!("  --no-shaking        Disable tree-shaking and include all prelude BIFs");
    println!("  --no-std-lib        Exclude the standard library (prelude) entirely");
    println!("  --output <path>     Set the output file path for compiled artifacts");
    println!("  --strip-source      Strip embedded source text from compiled output");
    println!(
        "                      Errors still report file:line; native binaries fall back to disk for snippets"
    );
    println!("  --serve             Start the built-in web server");
    println!("  --port <number>     Web server port (default: 8080)");
    println!("  --host <address>    Web server host (default: 127.0.0.1)");
    println!("  --webroot <path>    Web server root directory (default: .)");
    println!(
        "  --module <path>     Load a BoxLang module directory (may be specified multiple times)"
    );
    println!("\nCommands:");
    println!("  esp32-doctor        Check ESP32 build/flash prerequisites and print fixes");
    println!("\nIf no file is provided, matchbox starts in REPL mode.");
}

fn print_version() {
    let commit = env!("GIT_COMMIT");
    let date = env!("BUILD_DATE");
    let version = env!("CARGO_PKG_VERSION");
    println!("matchbox version {}", version);
    println!("commit: {}", commit);
    println!("built on: {}", date);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DoctorLevel {
    Ok,
    Warn,
    Fail,
}

impl DoctorLevel {
    fn label(self) -> &'static str {
        match self {
            DoctorLevel::Ok => "ok",
            DoctorLevel::Warn => "warning",
            DoctorLevel::Fail => "fail",
        }
    }
}

fn run_esp32_doctor() -> Result<()> {
    let mut worst = DoctorLevel::Ok;

    println!("MatchBox ESP32 doctor");
    println!();

    let idf_path = std_env::var("IDF_PATH").ok();
    match &idf_path {
        Some(path) => {
            let toolchain = Path::new(path).join("tools/cmake/toolchain-esp32s3.cmake");
            if toolchain.exists() {
                doctor_report(&mut worst, DoctorLevel::Ok, format!("IDF_PATH={path}"));
            } else {
                doctor_report(
                    &mut worst,
                    DoctorLevel::Fail,
                    format!(
                        "IDF_PATH is set to {path}, but {}/tools/cmake/toolchain-esp32s3.cmake is missing.",
                        path
                    ),
                );
            }
        }
        None => doctor_report(
            &mut worst,
            DoctorLevel::Fail,
            "IDF_PATH is not set. Run `source <esp-idf>/export.sh` before `matchbox --target esp32`.",
        ),
    }

    let idf_version = command_output("idf.py", &["--version"]);
    match idf_version {
        Some(version) => doctor_report(
            &mut worst,
            DoctorLevel::Ok,
            format!("idf.py --version => {version}"),
        ),
        None => doctor_report(
            &mut worst,
            DoctorLevel::Fail,
            "`idf.py` is not runnable in this shell. Activate ESP-IDF first with `source <esp-idf>/export.sh`.",
        ),
    }

    match std_env::var("RUSTUP_TOOLCHAIN") {
        Ok(toolchain) if toolchain == "esp" => {
            doctor_report(&mut worst, DoctorLevel::Ok, "RUSTUP_TOOLCHAIN=esp")
        }
        Ok(toolchain) => doctor_report(
            &mut worst,
            DoctorLevel::Warn,
            format!(
                "RUSTUP_TOOLCHAIN={toolchain}. For `matchbox --target esp32`, use `export RUSTUP_TOOLCHAIN=esp`."
            ),
        ),
        Err(_) => doctor_report(
            &mut worst,
            DoctorLevel::Warn,
            "RUSTUP_TOOLCHAIN is not set. This is fine for rebuilding MatchBox itself, but ESP32 builds should run with `export RUSTUP_TOOLCHAIN=esp`.",
        ),
    }

    for tool in ["cargo", "cmake", "ninja", "ldproxy", "espflash"] {
        match find_in_path(tool) {
            Some(path) => doctor_report(
                &mut worst,
                DoctorLevel::Ok,
                format!("{tool} => {}", path.display()),
            ),
            None => doctor_report(
                &mut worst,
                if tool == "ldproxy" || tool == "espflash" {
                    DoctorLevel::Fail
                } else {
                    DoctorLevel::Warn
                },
                format!("{tool} is not on PATH."),
            ),
        }
    }

    match std_env::var("LIBCLANG_PATH") {
        Ok(value) => doctor_report(
            &mut worst,
            DoctorLevel::Ok,
            format!("LIBCLANG_PATH={value}"),
        ),
        Err(_) => doctor_report(
            &mut worst,
            DoctorLevel::Warn,
            "LIBCLANG_PATH is not set. On Linux, `export LIBCLANG_PATH=/usr/lib64` may be required if bindgen picks the wrong libclang.",
        ),
    }

    match std_env::var("ESP_IDF_ESPUP_CLANG_SYMLINK") {
        Ok(value) if value == "ignore" => doctor_report(
            &mut worst,
            DoctorLevel::Ok,
            "ESP_IDF_ESPUP_CLANG_SYMLINK=ignore",
        ),
        Ok(value) => doctor_report(
            &mut worst,
            DoctorLevel::Warn,
            format!(
                "ESP_IDF_ESPUP_CLANG_SYMLINK={value}. If bindgen/header parsing fails, try `export ESP_IDF_ESPUP_CLANG_SYMLINK=ignore`."
            ),
        ),
        Err(_) => doctor_report(
            &mut worst,
            DoctorLevel::Warn,
            "ESP_IDF_ESPUP_CLANG_SYMLINK is not set. If bindgen/header parsing fails, try `export ESP_IDF_ESPUP_CLANG_SYMLINK=ignore`.",
        ),
    }

    let stubs_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("stubs");
    for (label, filename) in [
        ("ESP32 stub", "runner_stub_esp32.elf"),
        ("ESP32-S3 stub", "runner_stub_esp32s3.elf"),
        ("ESP32-C3 stub", "runner_stub_esp32c3.elf"),
    ] {
        let path = stubs_dir.join(filename);
        match fs::metadata(&path) {
            Ok(meta) if meta.len() > 0 => doctor_report(
                &mut worst,
                DoctorLevel::Ok,
                format!("{label} is present: {} bytes", meta.len()),
            ),
            Ok(_) => doctor_report(
                &mut worst,
                DoctorLevel::Warn,
                format!(
                    "{label} exists but is empty at {}. MatchBox will fall back to a local build.",
                    path.display()
                ),
            ),
            Err(_) => doctor_report(
                &mut worst,
                DoctorLevel::Warn,
                format!(
                    "{label} is missing at {}. MatchBox will fall back to a local build.",
                    path.display()
                ),
            ),
        }
    }

    if let Ok(cwd) = std_env::current_dir() {
        let cargo_config = cwd.join(".cargo/config.toml");
        if cargo_config.exists() {
            match fs::read_to_string(&cargo_config) {
                Ok(contents)
                    if contents.contains("xtensa-esp32")
                        || contents.contains("riscv32imc-esp-espidf")
                        || contents.contains("target = \"esp\"") =>
                {
                    doctor_report(
                        &mut worst,
                        DoctorLevel::Warn,
                        format!(
                            "{} forces an ESP target. Build the MatchBox CLI from a neutral directory or pass `--target x86_64-unknown-linux-gnu`.",
                            cargo_config.display()
                        ),
                    );
                }
                Ok(_) => doctor_report(
                    &mut worst,
                    DoctorLevel::Ok,
                    format!(
                        "{} found, but it does not force an ESP target.",
                        cargo_config.display()
                    ),
                ),
                Err(error) => doctor_report(
                    &mut worst,
                    DoctorLevel::Warn,
                    format!("Could not read {}: {}", cargo_config.display(), error),
                ),
            }
        }
    }

    report_serial_devices(&mut |level, message| doctor_report(&mut worst, level, message));

    println!();
    match worst {
        DoctorLevel::Ok => println!("ESP32 doctor found no blocking issues."),
        DoctorLevel::Warn => println!(
            "ESP32 doctor found warnings. Builds may still work, but the shell setup is not ideal."
        ),
        DoctorLevel::Fail => println!(
            "ESP32 doctor found blocking issues. Fix the failing items above before using `--target esp32`."
        ),
    }

    Ok(())
}

fn doctor_report(worst: &mut DoctorLevel, level: DoctorLevel, message: impl AsRef<str>) {
    if matches!(level, DoctorLevel::Fail)
        || (matches!(level, DoctorLevel::Warn) && matches!(*worst, DoctorLevel::Ok))
    {
        *worst = level;
    }
    println!("[{}] {}", level.label(), message.as_ref());
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    let path = std_env::var_os("PATH")?;
    for dir in std_env::split_paths(&path) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate_exe = dir.join(format!("{binary}.exe"));
            if candidate_exe.is_file() {
                return Some(candidate_exe);
            }
        }
    }
    None
}

fn command_output(binary: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(binary).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            None
        } else {
            Some(stderr)
        }
    } else {
        Some(stdout)
    }
}

fn report_serial_devices(report: &mut impl FnMut(DoctorLevel, String)) {
    let devices = serial_devices();
    if devices.is_empty() {
        report(
            DoctorLevel::Warn,
            "No common serial devices were found under /dev (ttyACM*, ttyUSB*, cu.usb*)."
                .to_string(),
        );
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let current_uid = command_output("id", &["-u"]).and_then(|s| s.parse::<u32>().ok());
        let groups = command_output("id", &["-G"])
            .map(|s| {
                s.split_whitespace()
                    .filter_map(|part| part.parse::<u32>().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for device in devices {
            match fs::metadata(&device) {
                Ok(meta) => {
                    let mode = meta.mode();
                    let uid = meta.uid();
                    let gid = meta.gid();
                    let has_access = current_uid
                        .map(|user| {
                            if user == uid {
                                mode & 0o600 == 0o600
                            } else if groups.contains(&gid) {
                                mode & 0o060 == 0o060
                            } else {
                                mode & 0o006 == 0o006
                            }
                        })
                        .unwrap_or(false);
                    if has_access {
                        report(
                            DoctorLevel::Ok,
                            format!(
                                "Serial device {} is readable/writable in this shell.",
                                device.display()
                            ),
                        );
                    } else {
                        report(
                            DoctorLevel::Warn,
                            format!(
                                "Serial device {} exists but this shell probably cannot open it. Add your user to the owning serial group or flash with `sudo espflash ...` as a fallback.",
                                device.display()
                            ),
                        );
                    }
                }
                Err(error) => report(
                    DoctorLevel::Warn,
                    format!(
                        "Could not inspect serial device {}: {}",
                        device.display(),
                        error
                    ),
                ),
            }
        }
    }

    #[cfg(not(unix))]
    for device in devices {
        report(
            DoctorLevel::Ok,
            format!("Detected serial device {}", device.display()),
        );
    }
}

fn serial_devices() -> Vec<PathBuf> {
    let mut devices = Vec::new();
    #[cfg(unix)]
    if let Ok(entries) = fs::read_dir("/dev") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.starts_with("ttyACM")
                    || name.starts_with("ttyUSB")
                    || name.starts_with("cu.usb")
                {
                    devices.push(path);
                }
            }
        }
    }
    devices.sort();
    devices
}

pub fn process_file(
    source_path: &Path,
    is_build: bool,
    orig_target: Option<&str>,
    keep_symbols: Vec<String>,
    no_shaking: bool,
    no_std_lib: bool,
    strip_source: bool,
    output: Option<&Path>,
    extra_module_paths: &[PathBuf],
    is_flash: bool,
    orig_chip: Option<&str>,
    is_fast_deploy: bool,
    is_watch: bool,
    is_full_flash: bool,
    esp32_web: bool,
) -> Result<()> {
    if source_path.extension().and_then(|s| s.to_str()) == Some("bxb") {
        let bytes = fs::read(source_path)?;
        let mut chunk: Chunk = postcard::from_bytes(&bytes)?;
        chunk.reconstruct_functions();
        run_chunk(chunk, &[])?;
    } else {
        let source = fs::read_to_string(source_path)?;
        let ext = source_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let ast = if ext == "bxm" {
            parser::parse_bxm(&source, source_path.to_str())
                .map_err(|e| anyhow::anyhow!("{} {}", "Parse Error:".red().bold(), e))?
        } else {
            parser::parse(&source, source_path.to_str())
                .map_err(|e| anyhow::anyhow!("{} {}", "Parse Error:".red().bold(), e))?
        };
        if orig_target == Some("esp32") {
            validate_esp32_target(&ast, esp32_web)?;
        }
        // Discover modules from matchbox.toml (in CWD) and any --module flags.
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| source_path.parent().unwrap_or(Path::new(".")).to_path_buf());
        let modules_info = modules::discover_modules(&cwd, extra_module_paths)?;
        let embedded_manifest = if orig_target == Some("esp32") {
            embedded::discover_embedded_app(&cwd)?
        } else {
            None
        };

        let module_mappings: Vec<(String, PathBuf)> = modules_info
            .iter()
            .map(|m| (m.name.clone(), m.path.clone()))
            .collect();
        let mut extra_preludes: Vec<String> = modules_info
            .iter()
            .flat_map(|m| m.bif_sources.iter().cloned())
            .collect();
        // Inject a getModuleSettings(name) function whose body is baked from the
        // settings collected by each module's configure() lifecycle call.
        if !modules_info.is_empty() {
            let settings_bxs = modules::generate_get_module_settings_bxs(&modules_info);
            extra_preludes.push(settings_bxs);
        }
        let mut chunk = matchbox_compiler::compile_with_treeshaking(
            source_path.to_str().unwrap_or("unknown"),
            &ast,
            &source,
            keep_symbols,
            no_shaking,
            no_std_lib,
            &module_mappings,
            &extra_preludes,
        )
        .map_err(|e| anyhow::anyhow!("Compiler Error: {}", e))?;

        chunk.reconstruct_functions();

        if strip_source {
            strip_sources(&mut chunk);
        }

        if is_build {
            let bytes = postcard::to_stdvec(&chunk)?;
            let out_path = output
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| source_path.with_extension("bxb"));
            fs::write(&out_path, bytes)?;
            println!("Compiled to {}", out_path.display());
        } else if let Some(t) = orig_target {
            let project_root = source_path.parent().unwrap_or(Path::new("."));
            let native_dir = project_root.join("native");

            let has_native_modules = modules_info.iter().any(|m| m.has_native);
            let should_use_fusion = ((native_dir.exists() && native_dir.is_dir())
                || has_native_modules)
                && !(t == "esp32" && esp32_web);
            if should_use_fusion {
                return produce_fusion_artifact(
                    &chunk,
                    source_path,
                    &native_dir,
                    t,
                    &ast,
                    output,
                    &modules_info,
                    embedded_manifest.as_ref(),
                    is_flash,
                    orig_chip,
                    esp32_web,
                );
            }

            match t {
                "wasi" => produce_wasi_binary(&chunk, source_path, output)?,
                "wasm" => produce_wasm_binary(&chunk, source_path, output)?,
                "js" => browser::packaging::produce_js_bundle(&chunk, source_path, &ast, output)?,
                "esp32" => {
                    let esp32_config = read_esp32_box_config(&cwd)?;
                    // In watch mode, force a full flash on the initial entry so the
                    // on-device runner always matches the current bytecode format.
                    let (fd, ff) = if is_watch {
                        (false, true)
                    } else {
                        (is_fast_deploy, is_full_flash)
                    };
                    produce_esp32_binary(
                        &chunk,
                        source_path,
                        output,
                        is_flash,
                        orig_chip,
                        fd,
                        ff,
                        esp32_web,
                        embedded_manifest.as_ref(),
                        esp32_config.as_ref(),
                    )?;
                }
                target_val => produce_native_binary(&chunk, source_path, target_val, output)?,
            }
        } else {
            // Register datasources from matchbox.toml only when actually running.
            #[cfg(all(not(target_arch = "wasm32"), feature = "bif-datasource"))]
            register_datasources_from_config(&cwd)?;

            run_chunk(chunk, &modules_info)?;
        }

        if is_watch {
            if orig_target != Some("esp32") {
                bail!("--watch is currently only supported for --target esp32");
            }
            let chip_str = orig_chip.map(|s| s.to_string());
            println!(
                "WATCH MODE ENABLED: Watching for changes in {}...",
                source_path.parent().unwrap().display()
            );
            return watch_mode(source_path, chip_str, is_full_flash, esp32_web);
        }
    }
    Ok(())
}

fn process_directory(
    path: &Path,
    is_build: bool,
    orig_target: Option<&str>,
    keep_symbols: Vec<String>,
    no_shaking: bool,
    no_std_lib: bool,
    strip_source: bool,
    output: Option<&Path>,
    extra_module_paths: &[PathBuf],
    is_flash: bool,
    orig_chip: Option<&str>,
    is_fast_deploy: bool,
    is_watch: bool,
    is_full_flash: bool,
    esp32_web: bool,
) -> Result<()> {
    let entry_points = ["index.bxs", "main.bxs", "Application.bx"];
    let mut entry_file = None;
    for ep in entry_points {
        let p = path.join(ep);
        if p.exists() {
            entry_file = Some(p);
            break;
        }
    }
    let entry_file = entry_file.context("No entry point found in directory")?;
    process_file(
        &entry_file,
        is_build,
        orig_target,
        keep_symbols,
        no_shaking,
        no_std_lib,
        strip_source,
        output,
        extra_module_paths,
        is_flash,
        orig_chip,
        is_fast_deploy,
        is_watch,
        is_full_flash,
        esp32_web,
    )
}

/// Recursively clear embedded source text from a chunk tree.
/// `filename` and per-opcode `lines` are preserved so errors still report `file:line`.
/// Native binaries automatically fall back to reading the source file from disk.
fn strip_sources(chunk: &mut Chunk) {
    chunk.source = String::new();
    for constant in chunk.constants.iter_mut() {
        match constant {
            types::Constant::CompiledFunction(f) => strip_sources(&mut f.chunk),
            types::Constant::Class(c) => {
                strip_sources(&mut c.constructor.chunk);
                for (_, m) in c.methods.iter_mut() {
                    strip_sources(&mut m.chunk);
                }
            }
            types::Constant::Interface(i) => {
                for (_, m) in i.methods.iter_mut() {
                    if let Some(f) = m {
                        strip_sources(&mut f.chunk);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "bif-datasource"))]
fn register_datasources_from_config(project_dir: &Path) -> Result<()> {
    use std::sync::Arc;
    let configs = modules::read_datasource_configs(project_dir)?;
    for (name, entry) in configs {
        let config = matchbox_vm::datasource::traits::DatasourceConfig {
            driver: entry.driver.clone(),
            host: entry.host,
            port: entry.port,
            database: entry.database,
            username: entry.username,
            password: entry.password,
            max_connections: entry.max_connections,
        };
        match entry.driver.to_lowercase().as_str() {
            "postgresql" | "postgres" => {
                let driver =
                    matchbox_vm::datasource::drivers::postgres::PostgresDriver::new(&config)
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to create datasource '{}': {}", name, e)
                        })?;
                matchbox_vm::datasource::registry::register(&name, Arc::new(driver));
            }
            other => {
                eprintln!(
                    "Warning: datasource '{}' has unknown driver '{}', skipping.",
                    name, other
                );
            }
        }
    }
    Ok(())
}

pub fn run_chunk(chunk: Chunk, modules: &[modules::ModuleInfo]) -> Result<()> {
    let args: Vec<String> = std_env::args().collect();

    let mut external_bifs = HashMap::new();
    let mut native_classes = HashMap::new();

    #[cfg(feature = "bif-tui")]
    {
        native_classes.extend(matchbox_tui::register_classes());
    }

    // In a real implementation with dynamic loading, we would load .so/.dll here.
    for m in modules {
        if m.name == "native-math" {
            // HACK for tests: since we can't easily dynamic-load from the same binary's crate
            // we just manually register what we know is in there.
            fn cube(
                _vm: &mut dyn types::BxVM,
                args: &[types::BxValue],
            ) -> std::result::Result<types::BxValue, String> {
                if args.len() != 1 {
                    return Err("cube requires 1 argument".to_string());
                }
                let n = args[0].as_number();
                Ok(types::BxValue::new_number(n * n * n))
            }
            external_bifs.insert("cube".to_string(), cube as types::BxNativeFunction);
        }
    }

    let mut vm = vm::VM::new_with_bifs(external_bifs, native_classes);
    vm.cli_args = args.clone();

    #[cfg(feature = "jit")]
    vm.enable_jit();

    vm.interpret(chunk)?;
    Ok(())
}

fn run_repl() -> Result<()> {
    println!("BoxLang REPL (Rust)");
    println!("Type 'exit' or 'quit' to exit.");

    let args: Vec<String> = std_env::args().collect();
    let mut vm = vm::VM::new_with_args(args);

    // Load full prelude for REPL

    let prelude_ast = parser::parse(matchbox_compiler::PRELUDE_SOURCE, Some("prelude.bxs"))?;
    let mut compiler = compiler::Compiler::new("prelude");
    let prelude_chunk = compiler.compile(&prelude_ast, matchbox_compiler::PRELUDE_SOURCE)?;
    vm.interpret(prelude_chunk)?;

    loop {
        print!("bx> ");
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            println!();
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        if input == "exit" || input == "quit" {
            break;
        }

        match parser::parse(input, Some("repl")) {
            Ok(ast) => {
                let mut compiler = compiler::Compiler::new("repl");
                compiler.is_repl = true;
                match compiler.compile(&ast, input) {
                    Ok(chunk) => match vm.interpret(chunk) {
                        Ok(val) => {
                            if val != types::BxValue::new_null() {
                                println!("=> {}", val);
                            }
                        }
                        Err(e) => println!("{} {}", "Error:".red().bold(), e),
                    },
                    Err(e) => println!("{} {}", "Compiler Error:".red().bold(), e),
                }
            }
            Err(e) => println!("{} {}", "Parse Error:".red().bold(), e),
        }
    }

    Ok(())
}

fn load_embedded_bytecode() -> Result<Chunk> {
    let self_path = std_env::current_exe().map_err(|_| anyhow::anyhow!("Not an executable"))?;
    let bytes = fs::read(self_path)?;
    if bytes.len() < 16 {
        bail!("Too small");
    }
    let footer_start = bytes.len() - 8;
    if &bytes[footer_start..] != MAGIC_FOOTER {
        bail!("No footer");
    }
    let len_start = bytes.len() - 16;
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&bytes[len_start..footer_start]);
    let len = u64::from_le_bytes(len_bytes) as usize;
    let chunk_start = len_start - len;
    let chunk_bytes = &bytes[chunk_start..len_start];
    let mut chunk: Chunk = postcard::from_bytes(chunk_bytes)?;
    chunk.reconstruct_functions();
    Ok(chunk)
}

#[cfg(target_arch = "wasm32")]
fn load_wasm_custom_section() -> Result<Chunk> {
    bail!("Custom section loading requires host support in this POC")
}

fn produce_native_binary(
    chunk: &Chunk,
    source_path: &Path,
    target: &str,
    output: Option<&Path>,
) -> Result<()> {
    let stub_key = if target == "native" { "host" } else { target };
    let native_bytes = stubs::get_stub(stub_key)?;
    let mut binary_bytes = native_bytes.to_vec();
    let chunk_bytes = postcard::to_stdvec(chunk)?;
    let chunk_len = chunk_bytes.len() as u64;
    binary_bytes.extend_from_slice(&chunk_bytes);
    binary_bytes.extend_from_slice(&chunk_len.to_le_bytes());
    binary_bytes.extend_from_slice(MAGIC_FOOTER);
    let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        if cfg!(windows) {
            source_path.with_extension("exe")
        } else {
            source_path.with_extension("")
        }
    });
    fs::write(&out_path, binary_bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&out_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&out_path, perms)?;
    }
    println!("Native binary produced: {}", out_path.display());
    Ok(())
}

fn produce_fusion_artifact(
    chunk: &Chunk,
    source_path: &Path,
    native_dir: &Path,
    target: &str,
    ast: &[ast::Statement],
    output: Option<&Path>,
    modules: &[modules::ModuleInfo],
    embedded_manifest: Option<&embedded::EmbeddedBuildManifest>,
    is_flash: bool,
    chip: Option<&str>,
    esp32_web: bool,
) -> Result<()> {
    println!(
        "Native Fusion detected! Target: {}. Building hybrid artifact...",
        target
    );

    let is_esp32 = target == "esp32";
    let chip = chip.unwrap_or("esp32");
    let esp_target = match chip {
        "esp32c3" | "esp32c6" | "esp32h2" => "riscv32imc-esp-espidf",
        "esp32s2" => "xtensa-esp32s2-espidf",
        "esp32s3" => "xtensa-esp32s3-espidf",
        _ => "xtensa-esp32-espidf",
    };

    let script_stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("fusion");
    let build_dir = std_env::temp_dir()
        .join("matchbox-fusion")
        .join(script_stem)
        .join(format!(
            "{}-{}",
            std::process::id(),
            FUSION_BUILD_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
    let mut embedded_route_table_path: Option<PathBuf> = None;
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir)
            .with_context(|| format!("Failed to clear fusion build dir {}", build_dir.display()))?;
    }
    fs::create_dir_all(&build_dir.join("src"))
        .with_context(|| format!("Failed to create fusion build dir {}", build_dir.display()))?;
    if let Some(manifest) = embedded_manifest {
        let manifest_path = embedded::write_embedded_manifest(&build_dir, manifest)?;
        let route_table_path = embedded::write_embedded_route_table(&build_dir, manifest)?;
        embedded_route_table_path = Some(route_table_path.clone());
        println!(
            "Embedded route manifest produced: {}",
            manifest_path.display()
        );
        println!(
            "Embedded route table produced: {}",
            route_table_path.display()
        );
    }

    let is_wasm = target == "wasm" || target == "js";
    let is_wasi = target == "wasi";

    // 1. Generate Cargo.toml
    let vm_path = concat!(env!("CARGO_MANIFEST_DIR"), "/crates/matchbox-vm").replace('\\', "/");

    let mut extra_dep_lines = String::new();
    for module in modules.iter().filter(|m| m.has_native) {
        let dep_path = module.path.join("matchbox");
        let dep_str = dep_path.to_str().unwrap_or("").replace('\\', "/");
        let dep_str = dep_str.strip_prefix("//?/").unwrap_or(&dep_str);
        let cargo_toml_path = dep_path.join("Cargo.toml");
        let crate_name = read_crate_name(&cargo_toml_path)?;
        let rust_name = crate_name.replace('-', "_");
        extra_dep_lines.push_str(&format!(
            "{} = {{ package = \"{}\", path = \"{}\" }}\n",
            rust_name, crate_name, dep_str,
        ));
    }

    let vm_dependency = if target == "js" {
        format!(
            "matchbox_vm = {{ path = \"{}\", features = [\"js\"] }}",
            vm_path
        )
    } else {
        format!("matchbox_vm = {{ path = \"{}\" }}", vm_path)
    };

    let wasm_extra_dependencies = if is_wasm {
        r#"
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
console_error_panic_hook = "0.1"
js-sys = "0.3"
web-sys = { version = "0.3", features = ["Window"] }
"#
    } else {
        ""
    };

    let mut cargo_toml = format!(
        r#"[package]
name = "fusion_build"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
{vm_dependency}
postcard = {{ version = "1.0", features = ["alloc", "use-std"] }}
bincode = "1.3.3"
anyhow = "1.0"
{wasm_extra_dependencies}
{extra_dep_lines}
"#
    );

    if is_esp32 {
        cargo_toml.push_str("esp-idf-svc = { version = \"0.52\", features = [\"binstart\", \"critical-section\", \"embassy-time-driver\"] }\n");
        cargo_toml.push_str("esp-idf-sys = \"0.37\"\n");
        cargo_toml.push_str("embedded-svc = \"0.29\"\n");
        cargo_toml
            .push_str("embassy-time = { version = \"0.5\", features = [\"generic-queue-8\"] }\n");
        cargo_toml.push_str("log = \"0.4\"\n");
        cargo_toml.push_str("serde_json = \"1.0\"\n");
        cargo_toml.push_str("url = \"2.5\"\n");
        cargo_toml.push_str("\n[features]\ndefault = []\nembedded-web = []\n");
        cargo_toml.push_str(
            "\n[build-dependencies]\nembuild = { version = \"0.33\", features = [\"espidf\"] }\n",
        );

        // Copy partitions and toolchain
        let runner_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/matchbox-esp32-runner");
        fs::copy(
            runner_root.join("partitions.csv"),
            build_dir.join("partitions.csv"),
        )?;
        fs::copy(
            runner_root.join("rust-toolchain.toml"),
            build_dir.join("rust-toolchain.toml"),
        )?;

        // Create .cargo/config.toml
        fs::create_dir_all(build_dir.join(".cargo"))?;
        fs::write(
            build_dir.join(".cargo/config.toml"),
            format!(
                r#"[build]
target = "{esp_target}"

[target.{esp_target}]
linker = "ldproxy"

[unstable]
build-std = ["std", "panic_abort"]
"#
            ),
        )?;

        // Create build.rs
        fs::write(
            build_dir.join("build.rs"),
            r#"use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dest_path = out_dir.join("bytecode.bxb");

    if let Ok(bytecode_path) = env::var("BOXLANG_BYTECODE_PATH") {
        fs::copy(bytecode_path, &dest_path).expect("Failed to copy bytecode");
    } else {
        fs::write(&dest_path, b"").expect("Failed to write dummy bytecode");
    }
    embuild::espidf::sysenv::output();
}
"#,
        )?;

        let mut sdkconfig_defaults = String::new();
        for module in modules.iter().filter(|m| m.has_native) {
            let sdkconfig_path = module.path.join("matchbox").join("sdkconfig.defaults");
            if sdkconfig_path.exists() {
                let contents = fs::read_to_string(&sdkconfig_path)?;
                if !sdkconfig_defaults.is_empty() && !sdkconfig_defaults.ends_with('\n') {
                    sdkconfig_defaults.push('\n');
                }
                sdkconfig_defaults.push_str(&contents);
                if !sdkconfig_defaults.ends_with('\n') {
                    sdkconfig_defaults.push('\n');
                }
            }
        }
        if !sdkconfig_defaults.is_empty() {
            fs::write(build_dir.join("sdkconfig.defaults"), sdkconfig_defaults)?;
        }
    }

    cargo_toml.push_str(
        r#"
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
"#,
    );

    if is_wasm {
        cargo_toml.push_str("\n[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n");
    }

    fs::write(build_dir.join("Cargo.toml"), cargo_toml)?;
    let workspace_lock = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    if workspace_lock.exists() {
        fs::copy(workspace_lock, build_dir.join("Cargo.lock"))?;
    }

    // 2. Prepare user modules
    let mut mod_decls = String::new();
    let mut registration_calls = String::new();

    if native_dir.exists() && native_dir.is_dir() {
        for entry in fs::read_dir(native_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let mod_name = path.file_stem().unwrap().to_str().unwrap();
                fs::copy(
                    &path,
                    build_dir.join("src").join(format!("{}.rs", mod_name)),
                )?;
                mod_decls.push_str(&format!("mod {};\n", mod_name));

                let content = fs::read_to_string(&path)?;
                if content.contains("pub fn register_bifs") {
                    registration_calls.push_str(&format!(
                        "    for (name, val) in {}::register_bifs() {{ bifs.insert(name, val); }}\n",
                        mod_name
                    ));
                }
                if content.contains("pub fn register_classes") {
                    registration_calls.push_str(&format!(
                        "    for (name, val) in {}::register_classes() {{ classes.insert(name, val); }}\n",
                        mod_name
                    ));
                }
            }
        }
    }

    for module in modules.iter().filter(|m| m.has_native) {
        let matchbox_dir = module.path.join("matchbox");
        let cargo_toml_path = matchbox_dir.join("Cargo.toml");
        let crate_name = read_crate_name(&cargo_toml_path)?;
        let rust_name = crate_name.replace('-', "_");

        let src_dir = matchbox_dir.join("src");
        let (mut has_bifs, mut has_classes) = (false, false);
        if src_dir.is_dir() {
            for entry in fs::read_dir(&src_dir)?.flatten() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                    let content = fs::read_to_string(&p).unwrap_or_default();
                    if content.contains("pub fn register_bifs") {
                        has_bifs = true;
                    }
                    if content.contains("pub fn register_classes") {
                        has_classes = true;
                    }
                }
            }
        }
        if has_bifs {
            registration_calls.push_str(&format!(
                "    for (name, val) in {}::register_bifs() {{ bifs.insert(name, val); }}\n",
                rust_name
            ));
        }
        if has_classes {
            registration_calls.push_str(&format!(
                "    for (name, val) in {}::register_classes() {{ classes.insert(name, val); }}\n",
                rust_name
            ));
        }
    }

    // 3. Generate main.rs / lib.rs
    let bytecode = postcard::to_stdvec(chunk)?;
    let mut code = String::new();

    if !is_esp32 && target != "js" {
        code.push_str(
            r#"
use matchbox_vm::{vm::VM, types::{BxValue, BxNativeFunction}, Chunk};
use std::collections::HashMap;
"#,
        );
    }

    code.push_str(&mod_decls);
    code.push_str("\n");

    if is_esp32 {
        code.push_str(&render_esp32_fusion_runner_source(&registration_calls));
    } else if target == "js" {
        code.push_str(&browser::runtime::render_fusion_web_host_source(
            &registration_calls,
            &bytecode,
        ));
    } else {
        code.push_str(&format!(
            r#"
fn run_interpreted() -> anyhow::Result<()> {{
    let mut bifs = HashMap::new();
    let mut classes = HashMap::new();
{}
    let bytecode: Vec<u8> = vec!{:?};
    let mut chunk: Chunk = postcard::from_bytes(&bytecode)?;
    chunk.reconstruct_functions();
    let mut vm = VM::new_with_bifs(bifs, classes);
    vm.interpret(chunk)?;
    Ok(())
}}
"#,
            registration_calls, bytecode
        ));
    }

    if is_wasm && target != "js" {
        code.push_str(r#"
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn run() { run_interpreted().unwrap(); }
#[wasm_bindgen]
pub struct BoxLangVM { vm: VM }
#[wasm_bindgen]
impl BoxLangVM {
    #[wasm_bindgen(constructor)]
    pub fn new() -> BoxLangVM { BoxLangVM { vm: VM::new_with_bifs(HashMap::new(), HashMap::new()) } }
    pub fn load_bytecode(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut chunk: Chunk = postcard::from_bytes(bytes).map_err(|e| e.to_string())?;
        chunk.reconstruct_functions();
        self.vm.interpret(chunk).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn vm_ptr(&self) -> usize { &self.vm as *const VM as usize }
    pub fn call(&mut self, name: &str, args: js_sys::Array) -> Result<JsValue, String> {
        let mut bx_args = Vec::new();
        for i in 0..args.length() { bx_args.push(self.vm.js_to_bx(args.get(i))); }
        match self.vm.call_function(name, bx_args) {
            Ok(val) => Ok(self.vm.bx_to_js(&val)),
            Err(e) => Err(e.to_string()),
        }
    }
}
"#);
        fs::write(build_dir.join("src").join("lib.rs"), code)?;
    } else if target == "js" {
        fs::write(build_dir.join("src").join("lib.rs"), code)?;
    } else if !is_esp32 {
        code.push_str("fn main() -> anyhow::Result<()> { run_interpreted() }\n");
        fs::write(build_dir.join("src").join("main.rs"), code)?;
    } else {
        fs::write(build_dir.join("src").join("main.rs"), code)?;
    }

    // 4. Build
    let mut cmd = if is_esp32 {
        let mut c = std::process::Command::new("rustup");
        c.arg("run")
            .arg("esp")
            .arg("cargo")
            .arg("build")
            .arg("--release");
        c.env("BOXLANG_BYTECODE_PATH", build_dir.join("bytecode.bxb")); // Dummy or real
        c.env_remove("RUSTC")
            .env_remove("CARGO")
            .env_remove("MAKEFLAGS")
            .env_remove("CARGO_MAKEFLAGS")
            .env_remove("CARGO_TARGET_DIR");
        c
    } else {
        let cargo_bin = std_env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let mut c = std::process::Command::new(cargo_bin);
        c.arg("build").arg("--release");
        c.env_remove("RUSTC")
            .env_remove("CARGO")
            .env_remove("MAKEFLAGS")
            .env_remove("CARGO_MAKEFLAGS")
            .env_remove("CARGO_TARGET_DIR");
        if build_dir.join("Cargo.lock").exists() {
            c.arg("--offline");
        }
        c
    };

    cmd.current_dir(&build_dir);
    if is_wasm {
        cmd.arg("--target").arg("wasm32-unknown-unknown");
    } else if is_wasi {
        cmd.arg("--target").arg("wasm32-wasip1");
    } else if is_esp32 {
        let bytecode_path = build_dir.join("bytecode.bxb");
        fs::write(&bytecode_path, bytecode)?;
        cmd.env("BOXLANG_BYTECODE_PATH", bytecode_path.to_str().unwrap());
        if let Some(route_table_path) = embedded_route_table_path.as_ref() {
            cmd.env(
                "MATCHBOX_EMBEDDED_ROUTE_TABLE",
                route_table_path.to_str().unwrap(),
            );
        }
        let sdkconfig_defaults_path = build_dir.join("sdkconfig.defaults");
        if sdkconfig_defaults_path.exists() {
            cmd.env(
                "ESP_IDF_SDKCONFIG_DEFAULTS",
                sdkconfig_defaults_path.to_str().unwrap(),
            );
        }
        if esp32_web {
            cmd.arg("--features").arg("embedded-web");
        }
    }

    let status = cmd
        .status()
        .with_context(|| format!("Failed to start cargo build in {}", build_dir.display()))?;
    if !status.success() {
        bail!("Failed to compile native fusion binary");
    }

    // 5. Handle Artifact
    if is_esp32 {
        let artifact = build_dir
            .join("target")
            .join(esp_target)
            .join("release")
            .join("fusion_build");
        let out_path = output
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| source_path.with_extension("elf"));
        fs::copy(&artifact, &out_path).with_context(|| {
            format!(
                "Failed to copy native fusion artifact {} to {}",
                artifact.display(),
                out_path.display()
            )
        })?;
        println!("ESP32 Fusion binary produced: {}", out_path.display());
        if is_flash {
            let mut flash_cmd = std::process::Command::new("espflash");
            flash_cmd
                .arg("flash")
                .arg("--chip")
                .arg(chip)
                .arg(&out_path);
            flash_cmd.status()?;
        }
    } else if target == "native" {
        let exe_name = if cfg!(windows) {
            "fusion_build.exe"
        } else {
            "fusion_build"
        };
        let artifact = build_dir.join("target").join("release").join(exe_name);
        let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            if cfg!(windows) {
                source_path.with_extension("exe")
            } else {
                source_path.with_extension("")
            }
        });
        fs::copy(&artifact, &out_path).with_context(|| {
            format!(
                "Failed to copy native fusion artifact {} to {}",
                artifact.display(),
                out_path.display()
            )
        })?;
        println!("Native Fusion binary produced: {}", out_path.display());
    } else if target == "wasi" || target == "wasm" {
        let t_folder = if target == "wasi" {
            "wasm32-wasip1"
        } else {
            "wasm32-unknown-unknown"
        };
        let artifact = build_dir
            .join("target")
            .join(t_folder)
            .join("release")
            .join("fusion_build.wasm");
        let out_path = output
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| source_path.with_extension("wasm"));
        fs::copy(&artifact, &out_path).with_context(|| {
            format!(
                "Failed to copy fusion wasm artifact {} to {}",
                artifact.display(),
                out_path.display()
            )
        })?;
        println!(
            "{} Fusion binary produced: {}",
            target.to_uppercase(),
            out_path.display()
        );
    } else if target == "js" {
        let artifact = build_dir
            .join("target")
            .join("wasm32-unknown-unknown")
            .join("release")
            .join("fusion_build.wasm");
        browser::packaging::produce_fusion_js_bundle(&artifact, source_path, ast, output)?;
    }
    let _ = fs::remove_dir_all(&build_dir);
    Ok(())
}

/// Extract the `[package] name` value from a `Cargo.toml` file.
fn read_crate_name(cargo_toml_path: &Path) -> Result<String> {
    let text = fs::read_to_string(cargo_toml_path)?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("name") {
            if let Some(eq) = line.find('=') {
                let value = line[eq + 1..]
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'');
                if !value.is_empty() {
                    return Ok(value.to_string());
                }
            }
        }
    }
    bail!("Could not find `name` in {}", cargo_toml_path.display())
}

fn produce_wasi_binary(chunk: &Chunk, source_path: &Path, output: Option<&Path>) -> Result<()> {
    let mut wasm_bytes = stubs::get_stub("wasi")?.to_vec();

    let chunk_bytes = postcard::to_stdvec(chunk)?;

    // The runner stub contains a 1 MB sentinel region whose layout is:
    //   [0..8]  = magic "BXLG\x00\x00\x00\x01"
    //   [8..12] = bytecode length (u32 LE)
    //   [12..]  = bytecode data
    // We locate it by magic and patch in-place — no cargo invocation needed.
    const SENTINEL: [u8; 8] = [b'B', b'X', b'L', b'G', 0, 0, 0, 1];
    const EMBED_CAPACITY: usize = 1024 * 1024;
    const EMBED_DATA_CAPACITY: usize = EMBED_CAPACITY - 12;

    let sentinel_pos = wasm_bytes
        .windows(SENTINEL.len())
        .position(|w| w == SENTINEL)
        .context(
            "WASI stub is missing the BOXLANG_EMBED sentinel. \
                  Rebuild the runner stub: \
                  cargo build --release --target wasm32-wasip1 -p matchbox_runner",
        )?;

    if chunk_bytes.len() > EMBED_DATA_CAPACITY {
        bail!(
            "Bytecode ({} bytes) exceeds the WASI embed capacity ({} bytes). \
             Increase EMBED_CAPACITY in crates/matchbox-runner/src/main.rs and rebuild the stub.",
            chunk_bytes.len(),
            EMBED_DATA_CAPACITY
        );
    }

    let len_offset = sentinel_pos + 8;
    let data_offset = sentinel_pos + 12;

    let len_bytes = (chunk_bytes.len() as u32).to_le_bytes();
    wasm_bytes[len_offset..len_offset + 4].copy_from_slice(&len_bytes);
    wasm_bytes[data_offset..data_offset + chunk_bytes.len()].copy_from_slice(&chunk_bytes);

    let out_path = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| source_path.with_extension("wasm"));
    fs::write(&out_path, wasm_bytes)?;

    println!("WASI container binary produced: {}", out_path.display());
    Ok(())
}

#[cfg(feature = "server")]
#[derive(serde::Serialize)]
struct WasiHttpPayload {
    config: matchbox_server::webroot_core::WebrootConfig,
    assets: matchbox_server::webroot_core::EmbeddedAssetPackage,
}

#[cfg(feature = "server")]
pub fn produce_wasi_http_webroot_artifact(webroot: &Path, output: &Path) -> Result<()> {
    let package = matchbox_server::prepare_wasi_http_webroot(webroot, None)?;
    for warning in &package.warnings {
        eprintln!("warning: {warning}");
    }

    let payload = WasiHttpPayload {
        config: package.config,
        assets: package.assets,
    };
    let payload_bytes = bincode::serialize(&payload)?;
    if payload_bytes.len() > WASI_HTTP_EMBED_DATA_CAPACITY {
        bail!(
            "WASI HTTP webroot payload ({} bytes) exceeds the embed capacity ({} bytes).",
            payload_bytes.len(),
            WASI_HTTP_EMBED_DATA_CAPACITY
        );
    }

    let mut wasm_bytes = read_or_build_wasi_http_runner()?;
    patch_wasi_http_runner_payload(&mut wasm_bytes, &payload_bytes)?;
    fs::write(output, wasm_bytes)?;
    println!("WASI HTTP component produced: {}", output.display());
    Ok(())
}

#[cfg(not(feature = "server"))]
pub fn produce_wasi_http_webroot_artifact(_webroot: &Path, _output: &Path) -> Result<()> {
    bail!("WASI HTTP target requires the server feature.");
}

fn read_or_build_wasi_http_runner() -> Result<Vec<u8>> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let runner_path = workspace_root
        .join("target")
        .join("wasm32-wasip2")
        .join("release")
        .join("matchbox_wasi_http_runner.wasm");

    if !runner_path.exists() {
        let status = Command::new("cargo")
            .current_dir(&workspace_root)
            .args([
                "build",
                "--release",
                "--target",
                "wasm32-wasip2",
                "-p",
                "matchbox_wasi_http_runner",
            ])
            .status()
            .context("failed to invoke cargo to build the WASI HTTP runner")?;
        if !status.success() {
            bail!(
                "failed to build WASI HTTP runner. Install the target with `rustup target add wasm32-wasip2` and retry."
            );
        }
    }

    fs::read(&runner_path).with_context(|| {
        format!(
            "failed to read WASI HTTP runner at {}",
            runner_path.display()
        )
    })
}

fn patch_wasi_http_runner_payload(wasm_bytes: &mut [u8], payload_bytes: &[u8]) -> Result<()> {
    let sentinel_pos = wasm_bytes
        .windows(WASI_HTTP_EMBED_MAGIC.len())
        .enumerate()
        .filter(|(_, window)| *window == WASI_HTTP_EMBED_MAGIC)
        .find_map(|(pos, _)| {
            let data_offset = pos + 12;
            wasm_bytes
                .get(data_offset)
                .copied()
                .filter(|byte| *byte == 0xA5)
                .map(|_| pos)
        })
        .context("WASI HTTP runner is missing the embedded webroot sentinel")?;
    let len_offset = sentinel_pos + 8;
    let data_offset = sentinel_pos + 12;
    let end = data_offset + payload_bytes.len();
    if end > wasm_bytes.len() {
        bail!("WASI HTTP runner embedded payload region is too small");
    }

    wasm_bytes[len_offset..len_offset + 4]
        .copy_from_slice(&(payload_bytes.len() as u32).to_le_bytes());
    wasm_bytes[data_offset..end].copy_from_slice(payload_bytes);
    Ok(())
}

fn produce_wasm_binary(chunk: &Chunk, source_path: &Path, output: Option<&Path>) -> Result<()> {
    let wasm_bytes = stubs::get_stub("wasi")?.to_vec();
    let chunk_bytes = postcard::to_stdvec(chunk)?;

    let mut out_bytes = wasm_bytes;
    use wasm_encoder::Encode;
    let custom_section = wasm_encoder::CustomSection {
        name: "boxlang_bytecode".into(),
        data: (&chunk_bytes).into(),
    };
    let mut section_bytes = Vec::new();
    custom_section.encode(&mut section_bytes);
    out_bytes.extend_from_slice(&section_bytes);

    let out_path = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| source_path.with_extension("wasm"));
    fs::write(&out_path, out_bytes)?;

    println!("WASM binary produced: {}", out_path.display());
    Ok(())
}

fn produce_esp32_binary(
    chunk: &Chunk,
    source_path: &Path,
    output: Option<&Path>,
    is_flash: bool,
    chip: Option<&str>,
    is_fast_deploy: bool,
    is_full_flash: bool,
    esp32_web: bool,
    embedded_manifest: Option<&embedded::EmbeddedBuildManifest>,
    esp32_config: Option<&Esp32BoxConfig>,
) -> Result<()> {
    fn write_embedded_esp32_partitions_csv() -> Result<PathBuf> {
        let path = std_env::temp_dir().join("matchbox_esp32_partitions.csv");
        fs::write(&path, ESP32_PARTITIONS_CSV)?;
        Ok(path)
    }

    fn write_storage_payload(chip: &str, payload: &[u8], description: &str) -> Result<()> {
        let mut data = (payload.len() as u32).to_le_bytes().to_vec();
        data.extend_from_slice(payload);
        let sector_size = 4096;
        let padded_len = (data.len() + sector_size - 1) / sector_size * sector_size;
        data.resize(padded_len, 0xFF);

        let temp_bin = std_env::temp_dir().join(format!("matchbox_{}.bin", description));
        fs::write(&temp_bin, &data)?;

        let mut flash_cmd = std::process::Command::new("espflash");
        flash_cmd
            .arg("write-bin")
            .arg("--chip")
            .arg(chip)
            .arg("--after")
            .arg("no-reset")
            .arg(ESP32_STORAGE_OFFSET)
            .arg(&temp_bin);

        let status = flash_cmd.status()?;
        if !status.success() {
            bail!("Failed to write {} to ESP32 storage partition", description);
        }

        let mut reset_cmd = std::process::Command::new("espflash");
        reset_cmd.arg("reset").arg("--chip").arg(chip);
        let _ = reset_cmd.status();

        Ok(())
    }

    let chip = chip.unwrap_or("esp32");
    let bytecode_bytes = postcard::to_stdvec(chunk)?;

    // ESP32 targets:
    let target = match chip {
        "esp32c3" | "esp32c6" | "esp32h2" => "riscv32imc-esp-espidf",
        "esp32s2" => "xtensa-esp32s2-espidf",
        "esp32s3" => "xtensa-esp32s3-espidf",
        _ => "xtensa-esp32-espidf",
    };

    let temp_dir = std_env::temp_dir().join("matchbox_esp32_build");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)?;
    }
    fs::create_dir_all(&temp_dir)?;

    let embedded_route_table_path = if esp32_web {
        embedded_manifest
            .map(|manifest| embedded::write_embedded_route_table(&temp_dir, manifest))
            .transpose()?
    } else {
        None
    };

    if is_fast_deploy && !esp32_web {
        println!(
            "FAST DEPLOY: Sending {} bytes of bytecode to ESP32 'storage' partition...",
            bytecode_bytes.len()
        );
        write_storage_payload(chip, &bytecode_bytes, "fast_deploy")?;
        println!("Fast deploy successful!");
        return Ok(());
    }

    if is_fast_deploy && esp32_web {
        if let Some(route_table_path) = embedded_route_table_path.as_ref() {
            let route_bytes = fs::read(route_table_path)?;
            println!(
                "FAST DEPLOY: Sending {} bytes of embedded app artifact to ESP32 'storage' partition...",
                route_bytes.len()
            );
            write_storage_payload(chip, &route_bytes, "embedded_route_table")?;
            println!("Fast deploy successful!");
            return Ok(());
        }
        bail!("Embedded web fast deploy requires an app/ directory with embedded routes");
    }

    // Attempt to use a pre-built stub if available
    if !esp32_web {
        if let Ok(stub_bytes) = stubs::get_stub(target) {
            println!("Using pre-built ESP32 stub for target '{}'...", target);
            let out_path = output
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| source_path.with_extension("elf"));

            // The ELF stub expects bytecode to be appended with a footer, or embedded via a specific mechanism.
            // For now, we use the same "append to end" logic used for native binaries,
            // but the matchbox-esp32-runner needs to know how to find it if load_from_flash fails.
            let mut binary_bytes = stub_bytes.to_vec();
            let chunk_bytes = postcard::to_stdvec(chunk)?;
            let chunk_len = chunk_bytes.len() as u64;
            binary_bytes.extend_from_slice(&chunk_bytes);
            binary_bytes.extend_from_slice(&chunk_len.to_le_bytes());
            binary_bytes.extend_from_slice(MAGIC_FOOTER);

            fs::write(&out_path, binary_bytes)?;

            if is_flash {
                println!("Flashing stub and then bytecode...");
                let partition_csv = write_embedded_esp32_partitions_csv()?;

                // 1. Flash the stub firmware with the partition table
                let mut flash_cmd = std::process::Command::new("espflash");
                flash_cmd
                    .arg("flash")
                    .arg("--chip")
                    .arg(chip)
                    .arg("--partition-table")
                    .arg(partition_csv)
                    .arg(&out_path);
                let status = flash_cmd.status()?;
                if !status.success() {
                    bail!("Failed to flash stub.");
                }

                // 2. Automatically perform fast-deploy for the bytecode
                return produce_esp32_binary(
                    chunk,
                    source_path,
                    output,
                    false,
                    Some(chip),
                    true,
                    is_full_flash,
                    esp32_web,
                    embedded_manifest,
                    esp32_config,
                );
            }

            println!("ESP32 firmware produced (stub): {}", out_path.display());
            println!(
                "NOTE: To run this, you must also flash the bytecode to the 'storage' partition."
            );
            return Ok(());
        } else {
            println!(
                "Note: Pre-built stub for '{}' is unavailable. Falling back to local build...",
                target
            );
        }
    }

    println!(
        "Building custom ESP32 firmware for chip: {} (no stub found)...",
        chip
    );
    println!(
        "Using activated ESP-IDF environment (`ESP_IDF_TOOLS_INSTALL_DIR=fromenv`) for ESP32 runner build."
    );

    let bytecode_path = temp_dir.join("bytecode.bxb");
    fs::write(&bytecode_path, bytecode_bytes)?;

    let runner_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join("matchbox-esp32-runner");

    let cargo_bin = std_env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = std::process::Command::new(cargo_bin);

    cmd.arg("build")
        .arg("--release")
        .arg("--target")
        .arg(target)
        .current_dir(&runner_path)
        .env("BOXLANG_BYTECODE_PATH", bytecode_path.to_str().unwrap())
        .env("ESP_IDF_TOOLS_INSTALL_DIR", "fromenv")
        .env("MCU", chip);

    if let Some(config) = esp32_config {
        if let Some(board) = config.board.as_deref() {
            cmd.env("MATCHBOX_ESP32_BOARD", board);
        }
        if let Some(web_port) = config.web_port {
            cmd.env("MATCHBOX_ESP32_WEB_PORT", web_port.to_string());
        }
        if let Some(wifi) = config.wifi.as_ref() {
            if let Some(ssid) = wifi.ssid.as_deref() {
                cmd.env("MATCHBOX_ESP32_WIFI_SSID", ssid);
            }
            if let Some(password) = wifi.password.as_deref() {
                cmd.env("MATCHBOX_ESP32_WIFI_PASSWORD", password);
            }
            if let Some(hostname) = wifi.hostname.as_deref() {
                cmd.env("MATCHBOX_ESP32_WIFI_HOSTNAME", hostname);
            }
        }
    }

    if let Some(route_table_path) = embedded_route_table_path.as_ref() {
        cmd.env(
            "MATCHBOX_EMBEDDED_ROUTE_TABLE",
            route_table_path.to_str().unwrap(),
        );
    }

    if esp32_web {
        cmd.arg("--features").arg("embedded-web");
    }
    let psram_enabled = esp32_config
        .and_then(|config| config.psram)
        .unwrap_or(false)
        || std_env::var_os("MATCHBOX_ESP32_PSRAM").is_some();
    if psram_enabled {
        cmd.arg("--features").arg("psram");
        match esp32_config.and_then(|config| config.board.as_deref()) {
            Some("xiao-esp32s3-sense") => {
                let sdkconfig_path = runner_path.join("sdkconfig.xiao-esp32s3-sense");
                cmd.env("ESP_IDF_SDKCONFIG", sdkconfig_path.to_str().unwrap());
                cmd.env("SDKCONFIG", sdkconfig_path.to_str().unwrap());
            }
            _ => {
                let sdkconfig_defaults = runner_path.join("sdkconfig.defaults.psram");
                cmd.env(
                    "ESP_IDF_SDKCONFIG_DEFAULTS",
                    sdkconfig_defaults.to_str().unwrap(),
                );
                cmd.env("SDKCONFIG_DEFAULTS", sdkconfig_defaults.to_str().unwrap());
            }
        }
    }

    let status = cmd.status()?;
    if !status.success() {
        bail!(
            "Failed to compile ESP32 runner. Ensure the ESP32 Rust toolchain is installed, \
             a real ESP-IDF environment has been activated (for example `source <esp-idf>/export.sh`), \
             and `RUSTUP_TOOLCHAIN=esp` is set before invoking MatchBox."
        );
    }

    let elf_path = runner_path
        .join("target")
        .join(target)
        .join("release")
        .join("matchbox-esp32-runner");
    let out_path = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| source_path.with_extension("elf"));

    fs::copy(&elf_path, &out_path)?;
    println!("ESP32 firmware produced: {}", out_path.display());

    if is_flash {
        println!("Flashing to device...");
        let partition_csv = write_embedded_esp32_partitions_csv()?;

        let mut flash_cmd = std::process::Command::new("espflash");
        flash_cmd
            .arg("flash")
            .arg("--chip")
            .arg(chip)
            .arg("--partition-table")
            .arg(partition_csv)
            .arg(&elf_path);

        let flash_status = flash_cmd.status()?;
        if !flash_status.success() {
            bail!("Failed to flash to ESP32 device.");
        }
        println!("Flash successful!");

        if esp32_web {
            if let Some(route_table_path) = embedded_route_table_path.as_ref() {
                let route_bytes = fs::read(route_table_path)?;
                println!("Writing embedded app artifact to 'storage' partition...");
                write_storage_payload(chip, &route_bytes, "embedded_route_table")?;
                println!("Embedded app artifact deploy complete.");
            }
        }

        if is_full_flash && !esp32_web {
            println!("Full flash complete. Sending fresh bytecode to the 'storage' partition...");
            return produce_esp32_binary(
                chunk,
                source_path,
                output,
                false,
                Some(chip),
                true,
                is_full_flash,
                esp32_web,
                embedded_manifest,
                esp32_config,
            );
        } else if is_full_flash && esp32_web {
            println!("Full flash complete for embedded web firmware.");
        }
    }

    Ok(())
}

/// Poll until the serial port path exists or the timeout expires.
fn wait_for_port(port: &str, timeout: std::time::Duration) -> bool {
    use std::time::Instant;
    let start = Instant::now();
    while start.elapsed() < timeout {
        if Path::new(port).exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

fn watch_mode(
    source_path: &Path,
    chip: Option<String>,
    is_full_flash: bool,
    esp32_web: bool,
) -> Result<()> {
    use notify::{RecursiveMode, Watcher, event::EventKind};
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Stdio};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })?;

    let source_path_abs = fs::canonicalize(source_path)?;
    let watch_dir = source_path_abs
        .parent()
        .context("Failed to get source directory")?;
    watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;

    let monitor_child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

    // Handle Ctrl+C to kill the child process before exiting
    let monitor_ctrlc = Arc::clone(&monitor_child);
    ctrlc::set_handler(move || {
        println!("\nWATCH MODE: Shutting down...");
        if let Ok(mut child_opt) = monitor_ctrlc.lock() {
            if let Some(mut child) = child_opt.take() {
                #[cfg(unix)]
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                #[cfg(not(unix))]
                {
                    child.kill().ok();
                }
                let _ = child.wait();
            }
        }
        std::process::exit(0);
    })
    .expect("Error setting Ctrl+C handler");

    // Detect the serial port once using espflash's board-info (quick probe)
    let serial_port = {
        let output = std::process::Command::new("espflash")
            .arg("board-info")
            .arg("--before")
            .arg("no-reset-no-sync")
            .output();
        let mut port = None;
        if let Ok(out) = &output {
            let combined = String::from_utf8_lossy(&out.stderr).to_string()
                + &String::from_utf8_lossy(&out.stdout);
            for line in combined.lines() {
                if line.contains("Serial port:") {
                    if let Some(p) = line.split("Serial port:").nth(1) {
                        port = Some(p.trim().trim_matches('\'').to_string());
                    }
                }
            }
        }
        port.unwrap_or_else(|| {
            for p in &[
                "/dev/ttyACM0",
                "/dev/ttyUSB0",
                "/dev/ttyACM1",
                "/dev/ttyUSB1",
            ] {
                if Path::new(p).exists() {
                    return p.to_string();
                }
            }
            "/dev/ttyACM0".to_string()
        })
    };
    println!("WATCH MODE: Using serial port: {}", serial_port);

    // Read serial output directly via stty + cat.  espflash monitor always
    // enters the bootloader handshake which breaks USB-JTAG (chip reset
    // disconnects USB).  Raw cat avoids that entirely.
    // Read serial output directly via stty + cat.  espflash monitor always
    // enters the bootloader handshake which breaks USB-JTAG (chip reset
    // disconnects USB).  Raw cat avoids that entirely.
    let start_monitor = |port: &str| -> Option<Child> {
        // Configure serial port for raw mode at 115200 baud
        let _ = std::process::Command::new("stty")
            .arg("-F")
            .arg(port)
            .arg("115200")
            .arg("raw")
            .arg("-echo")
            .status();

        let mut cmd = std::process::Command::new("cat");
        cmd.arg(port);

        // Put the child in its own process group for clean kill
        #[cfg(unix)]
        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        match cmd.spawn() {
            Ok(child) => Some(child),
            Err(e) => {
                eprintln!("Failed to start serial monitor: {}", e);
                None
            }
        }
    };

    println!("WATCH MODE: Initial monitor starting...");
    wait_for_port(&serial_port, Duration::from_secs(5));
    if let Ok(mut child_opt) = monitor_child.lock() {
        *child_opt = start_monitor(&serial_port);
    }

    loop {
        if let Ok(event) = rx.recv() {
            if let EventKind::Modify(_) = event.kind {
                if event.paths.iter().any(|p| {
                    fs::canonicalize(p)
                        .map(|cp| cp == source_path_abs)
                        .unwrap_or(false)
                }) {
                    // Debounce: drain the channel and wait for silence
                    std::thread::sleep(Duration::from_millis(200));
                    while let Ok(_) = rx.try_recv() {}

                    println!("\nChange detected! Killing monitor...");

                    // 1. Kill existing monitor and wait for it to fully exit
                    if let Ok(mut child_opt) = monitor_child.lock() {
                        if let Some(mut child) = child_opt.take() {
                            #[cfg(unix)]
                            unsafe {
                                libc::kill(-(child.id() as i32), libc::SIGKILL);
                            }
                            #[cfg(not(unix))]
                            {
                                child.kill().ok();
                            }
                            let _ = child.wait();
                        }
                    }

                    println!("Re-deploying...");

                    // 2. Recompile and fast-deploy
                    let mut flash_success = false;
                    for attempt in 1..=3 {
                        if !wait_for_port(&serial_port, Duration::from_secs(5)) {
                            println!(
                                "Attempt {}: port {} not available, retrying...",
                                attempt, serial_port
                            );
                            continue;
                        }

                        let res = (|| -> Result<()> {
                            let source_text = fs::read_to_string(source_path)?;
                            let ast =
                                parser::parse(&source_text, source_path.to_str()).map_err(|e| {
                                    anyhow::anyhow!("{} {}", "Parse Error:".red().bold(), e)
                                })?;
                            let cwd = std::env::current_dir().unwrap_or_else(|_| {
                                source_path.parent().unwrap_or(Path::new(".")).to_path_buf()
                            });
                            let embedded_manifest = if esp32_web {
                                embedded::discover_embedded_app(&cwd)?
                            } else {
                                None
                            };
                            let esp32_config = read_esp32_box_config(&cwd)?;
                            let chunk = matchbox_compiler::compile_with_treeshaking(
                                source_path.to_str().unwrap_or("unknown"),
                                &ast,
                                &source_text,
                                Vec::new(), // keep_symbols
                                false,      // no_shaking
                                false,      // no_std_lib
                                &[],        // module_mappings
                                &[],        // extra_preludes
                            )
                            .map_err(|e| anyhow::anyhow!("Compiler Error: {}", e))?;
                            produce_esp32_binary(
                                &chunk,
                                source_path,
                                None,
                                true,
                                chip.as_deref(),
                                true,
                                is_full_flash,
                                esp32_web,
                                embedded_manifest.as_ref(),
                                esp32_config.as_ref(),
                            )?;
                            Ok(())
                        })();

                        if let Err(e) = res {
                            println!("Redeploy attempt {} failed: {}. Retrying...", attempt, e);
                            std::thread::sleep(Duration::from_millis(1000));
                        } else {
                            println!("Live update successful! Restarting monitor...");
                            flash_success = true;
                            break;
                        }
                    }

                    if !flash_success {
                        eprintln!("CRITICAL: Redeploy failed after 3 attempts.");
                    }

                    // 3. Restart monitor — wait for port after flash reset
                    wait_for_port(&serial_port, Duration::from_secs(5));
                    if let Ok(mut child_opt) = monitor_child.lock() {
                        *child_opt = start_monitor(&serial_port);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esp32_validator_allows_route_and_middleware_registration() {
        let source = r#"
            app = web.server();
            app.use( function( event, rc, prc, next ) {
                prc.started = true;
                next.run();
            } );
            app.get( "/health", function( event, rc, prc ) {
                event.renderJson( { "ok": true } );
            } );
        "#;
        let ast = parser::parse(source, Some("test")).unwrap();
        assert!(validate_esp32_target(&ast, true).is_ok());
    }

    #[test]
    fn esp32_validator_allows_inline_html_and_json_handlers() {
        let source = r#"
            app = web.server();
            app.get( "/", function( event, rc, prc ) {
                event.renderHtml( "<h1>ready</h1>" );
            } );
            app.post( "/print", function( event, rc, prc ) {
                event.renderJson( { "ok": true, "path": event.getCurrentRoute() } );
            } );
        "#;
        let ast = parser::parse(source, Some("test")).unwrap();
        assert!(validate_esp32_target(&ast, true).is_ok());
    }

    #[test]
    fn esp32_validator_requires_opt_in_for_web_server() {
        let source = r#"
            app = web.server();
            app.get( "/health", function( event, rc, prc ) {
                event.renderJson( { "ok": true } );
            } );
        "#;
        let ast = parser::parse(source, Some("test")).unwrap();
        let err = validate_esp32_target(&ast, false).unwrap_err().to_string();
        assert!(err.contains("web.server()"));
        assert!(err.contains("--esp32-web"));
    }

    #[test]
    fn esp32_validator_rejects_listen() {
        let source = r#"
            app = web.server();
            app.listen( 8080 );
        "#;
        let ast = parser::parse(source, Some("test")).unwrap();
        let err = validate_esp32_target(&ast, true).unwrap_err().to_string();
        assert!(err.contains("line 3"));
        assert!(err.contains("app.listen()"));
        assert!(err.contains("Embedded HTTP transport is not implemented yet"));
    }

    #[test]
    fn esp32_validator_rejects_static_assets_templates_and_webhooks() {
        let source = r#"
            app = web.server();
            app.use( app.middleware.buildStaticFiles( "/assets", "public" ) );
            app.webhook(
                app.buildWebhook()
                    .path( "/hooks/example" )
                    .secret( "topsecret" )
                    .signatureHeader( "x-signature" ),
                function( event, rc, prc ) {
                    event.setView( "views/home.bxm" );
                }
            );
        "#;
        let ast = parser::parse(source, Some("test")).unwrap();
        let err = validate_esp32_target(&ast, true).unwrap_err().to_string();
        assert!(err.contains("buildStaticFiles()"));
        assert!(err.contains("webhook registration"));
        assert!(err.contains("event.setView()"));
    }

    #[test]
    fn esp32_validator_rejects_cookie_and_session_helpers() {
        let source = r#"
            function demo( event ) {
                event.setHTTPCookie( "theme", "light" );
                event.setSessionValue( "userId", 42 );
            }
        "#;
        let ast = parser::parse(source, Some("test")).unwrap();
        let err = validate_esp32_target(&ast, true).unwrap_err().to_string();
        assert!(err.contains("cookie helpers"));
        assert!(err.contains("session helpers"));
    }

    #[cfg(feature = "server")]
    #[test]
    fn wasi_http_webroot_artifact_embeds_prepared_webroot() {
        let has_wasip2_target = std::process::Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|targets| targets.lines().any(|target| target == "wasm32-wasip2"))
            .unwrap_or(false);
        if !has_wasip2_target {
            eprintln!(
                "skipping wasi_http_webroot_artifact_embeds_prepared_webroot: wasm32-wasip2 target is not installed"
            );
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().join("webroot");
        std::fs::create_dir(&webroot).unwrap();
        std::fs::write(
            webroot.join("index.bxm"),
            "<bx:output>Hello WASI HTTP</bx:output>",
        )
        .unwrap();
        let output = temp.path().join("app.wasm");

        produce_wasi_http_webroot_artifact(&webroot, &output).unwrap();

        let wasm = std::fs::read(&output).unwrap();
        assert!(wasm.starts_with(b"\0asm"));
        let sentinel = wasm
            .windows(WASI_HTTP_EMBED_MAGIC.len())
            .position(|window| window == WASI_HTTP_EMBED_MAGIC)
            .expect("runner sentinel should remain present");
        let len_offset = sentinel + 8;
        let payload_len = u32::from_le_bytes(wasm[len_offset..len_offset + 4].try_into().unwrap());
        assert!(payload_len > 0);
    }
}
