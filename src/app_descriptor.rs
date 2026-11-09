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
