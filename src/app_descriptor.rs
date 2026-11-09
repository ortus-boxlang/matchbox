//! Mappings for a compilation, from the places BoxLang allows (see docs/mappings.md):
//! `mappings` in `boxlang.json` (global) and `this.mappings` in an `Application.bx` next to the
//! program (application, overrides the global ones). Evaluated when the program is compiled, so a
//! native binary carries the classes found through them.

use anyhow::{Context, Result};
use matchbox_compiler::{
    ast,
    mappings::{self, Mapping},
    parser,
};
use std::{fs, path::Path};

/// Global mappings first, application mappings after them (later entries win).
pub fn collect(source_path: &Path) -> Result<Vec<Mapping>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let mut all = Vec::new();
    let config = cwd.join("boxlang.json");
    if config.is_file() {
        all.extend(from_boxlang_json(&config, &cwd)?);
    }
    let dir = source_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| cwd.clone());
    let application = dir.join("Application.bx");
    if application.is_file() && application != source_path {
        all.extend(from_application(&application)?);
    }
    Ok(all)
}

/// `"mappings": { "/models": "${user-dir}/models", "/internal": { "path": "...", "external": false } }`
pub fn from_boxlang_json(file: &Path, user_dir: &Path) -> Result<Vec<Mapping>> {
    let text = fs::read_to_string(file).with_context(|| format!("Cannot read {}", file.display()))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("Invalid {}", file.display()))?;
    let Some(object) = json.get("mappings").and_then(|m| m.as_object()) else {
        return Ok(Vec::new());
    };
    let user_dir_text = user_dir.to_string_lossy();
    let mut found = Vec::new();
    for (name, value) in object {
        let value = expand_placeholders(value, &user_dir_text);
        found.push(mappings::from_json(name, &value, user_dir).map_err(anyhow::Error::msg)?);
    }
    Ok(found)
}

fn expand_placeholders(value: &serde_json::Value, user_dir: &str) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(s.replace("${user-dir}", user_dir)),
        serde_json::Value::Object(o) => serde_json::Value::Object(
            o.iter()
                .map(|(k, v)| (k.clone(), expand_placeholders(v, user_dir)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Runs the pseudo-constructor of `Application.bx` (without `init()`, like BoxLang) and reads
/// `this.mappings`. Relative paths resolve against the directory of the Application.bx.
pub fn from_application(file: &Path) -> Result<Vec<Mapping>> {
    let source = fs::read_to_string(file)?;
    let mut statements = parser::parse(&source, file.to_str())
        .map_err(|e| anyhow::anyhow!("Parse Error in {}: {e}", file.display()))?;
    for statement in statements.iter_mut() {
        if let ast::StatementKind::ClassDecl { name, .. } = &mut statement.kind {
            if name.is_empty() {
                *name = "Application".to_string();
            }
            let class_name = name.clone();
            let tail = format!(
                "function __bxReadMappings( app ) {{ try {{ return serializeJSON( app.mappings ); }} catch ( any e ) {{ return \"{{}}\"; }} }}\n__bxMappings = __bxReadMappings( new {class_name}() );\n"
            );
            let mut extra = parser::parse(&tail, file.to_str())
                .map_err(|e| anyhow::anyhow!("Parse Error: {e}"))?;
            statements.append(&mut extra);
            return evaluate(file, &statements, &source);
        }
    }
    Ok(Vec::new())
}

fn evaluate(file: &Path, statements: &[ast::Statement], source: &str) -> Result<Vec<Mapping>> {
    use matchbox_vm::types::BxVM;
    let chunk = matchbox_compiler::compile_with_treeshaking(
        file.to_str().unwrap_or("Application.bx"),
        statements,
        source,
        Vec::new(),
        false,
        false,
        false,
        &[],
    )?;
    let mut vm = matchbox_vm::vm::VM::new_with_args(Vec::new());
    vm.interpret(chunk)
        .with_context(|| format!("Error while reading mappings from {}", file.display()))?;
    // The program ends by storing the JSON in a top-level variable (a VM global).
    let value = BxVM::get_global_value(&vm, "__bxMappings")
        .or_else(|| BxVM::get_global_value(&vm, "__bxmappings"));
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let text = BxVM::to_string(&mut vm, value);
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let base = file.parent().unwrap_or(Path::new("."));
    let mut found = Vec::new();
    if let Some(object) = json.as_object() {
        for (name, value) in object {
            found.push(mappings::from_json(name, value, base).map_err(anyhow::Error::msg)?);
        }
    }
    Ok(found)
}

/// Runs the program inside the lifecycle of the `Application.bx` next to it, like `boxlang` does for
/// scripts and classes (docs: Application.bx, "Multi-Runtime Support"):
///
/// `onApplicationStart` once (only when `this.name` is set; `false` stops the run), `onRequestStart(
/// targetPage )` (`false` stops the run), the program, `onError( exception, "" )` for an uncaught
/// error (`true` means handled, otherwise the error propagates), `onRequestEnd()` always, and
/// `onApplicationEnd( application )` at the end. `onRequest` is not used for scripts. Session
/// hooks never fire in the CLI. The class is created without calling `init()`.
///
/// There is no `application` scope in MatchBox yet, so a plain struct named `application` is
/// created for the run. Declarations (`import`, `class`, `function`) stay outside the guarded
/// block so they are available to the hooks.
pub fn wrap_with_application(
    program: Vec<ast::Statement>,
    source_path: &Path,
) -> Result<Vec<ast::Statement>> {
    let dir = match source_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => std::env::current_dir()?,
    };
    let application_file = dir.join("Application.bx");
    if !application_file.is_file() || application_file == source_path {
        return Ok(program);
    }
    let source = fs::read_to_string(&application_file)?;
    let mut application = parser::parse(&source, application_file.to_str())
        .map_err(|e| anyhow::anyhow!("Parse Error in {}: {e}", application_file.display()))?;
    let mut has_class = false;
    for statement in application.iter_mut() {
        if let ast::StatementKind::ClassDecl { name, .. } = &mut statement.kind {
            if name.is_empty() {
                *name = "Application".to_string();
            }
            has_class = true;
            // Globals are case insensitive and the program gets an `application` struct, so the
            // class cannot keep the name `Application`.
            *name = "__BxApplication".to_string();
            break;
        }
    }
    if !has_class {
        return Ok(program);
    }

    let absolute = if source_path.is_absolute() {
        source_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(source_path)
    };
    let target = absolute.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let lifecycle = format!(
        r#"application = {{}};
__bxApp = __bxAppNew( "__BxApplication" );
__bxAppNamed = false;
try {{ __bxAppNamed = len( __bxApp.name ) > 0; }} catch ( any e ) {{ }}
__bxAppProceed = true;
if ( __bxAppNamed ) {{ __bxAppProceed = __bxAppHook( __bxApp, "onApplicationStart", [], true ); }}
try {{
    if ( __bxAppProceed && __bxAppHook( __bxApp, "onRequestStart", [ "{target}" ], true ) ) {{
        __bxBody();
    }}
}} catch ( any __bxError ) {{
    if ( !__bxAppHook( __bxApp, "onError", [ __bxError, "" ], false ) ) {{ rethrow; }}
}} finally {{
    __bxAppHook( __bxApp, "onRequestEnd", [], true );
}}
if ( __bxAppNamed ) {{ __bxAppHook( __bxApp, "onApplicationEnd", [ application ], true ); }}
"#
    );
    let mut generated = parser::parse(&lifecycle, application_file.to_str())
        .map_err(|e| anyhow::anyhow!("Parse Error: {e}"))?;

    // Declarations stay outside the guarded block, everything else runs inside it.
    let (declarations, body): (Vec<_>, Vec<_>) = program.into_iter().partition(|s| {
        matches!(
            s.kind,
            ast::StatementKind::Import { .. }
                | ast::StatementKind::ClassDecl { .. }
                | ast::StatementKind::InterfaceDecl { .. }
                | ast::StatementKind::FunctionDecl { .. }
        )
    });
    let mut body = Some(body);
    for statement in generated.iter_mut() {
        if splice_body(statement, &mut body) {
            break;
        }
    }
    if body.is_some() {
        anyhow::bail!("Internal error: lifecycle placeholder not found");
    }

    let mut all = application;
    all.extend(declarations);
    all.extend(generated);
    Ok(all)
}

/// Replaces the `__bxBody();` placeholder statement with the program body.
fn splice_body(statement: &mut ast::Statement, body: &mut Option<Vec<ast::Statement>>) -> bool {
    fn in_list(list: &mut Vec<ast::Statement>, body: &mut Option<Vec<ast::Statement>>) -> bool {
        if let Some(at) = list.iter().position(is_placeholder) {
            if let Some(statements) = body.take() {
                list.splice(at..=at, statements);
            }
            return true;
        }
        list.iter_mut().any(|s| splice_body(s, body))
    }
    match &mut statement.kind {
        ast::StatementKind::If { then_branch, else_branch, .. } => {
            in_list(then_branch, body) || else_branch.as_mut().is_some_and(|e| in_list(e, body))
        }
        ast::StatementKind::TryCatch { try_branch, .. } => in_list(try_branch, body),
        _ => false,
    }
}

fn is_placeholder(statement: &ast::Statement) -> bool {
    matches!(&statement.kind, ast::StatementKind::Expression(expr) if format!("{expr:?}").contains("__bxBody"))
}
