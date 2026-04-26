use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// A resolved, validated module ready for use in compilation.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// The module name (e.g. `"bx-strings"`). Becomes the `bxModules.{name}` namespace.
    pub name: String,
    /// Absolute physical path to the module root directory.
    pub path: PathBuf,
    /// Contents of each `bifs/*.bxs` (or `.bx`) file in this module, injected as extra
    /// prelude sources during tree-shaking.
    pub bif_sources: Vec<String>,
    /// Whether this module contains a `matchbox/Cargo.toml` for native Rust compilation.
    pub has_native: bool,
    /// Settings collected by executing `ModuleConfig.bx configure()` at compile time.
    /// An empty JSON object if the module has no settings or lifecycle execution failed.
    pub settings: serde_json::Value,
}

// ─────────────────────────────────────────────────────────────────────────────
// matchbox.toml deserialization
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    modules: HashMap<String, ManifestEntry>,
    #[serde(default)]
    datasources: HashMap<String, DatasourceEntry>,
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    path: String,
}

#[derive(Debug, Deserialize)]
struct BoxJson {
    #[serde(default)]
    dependencies: HashMap<String, String>,
    #[serde(default)]
    #[serde(rename = "devDependencies")]
    dev_dependencies: HashMap<String, String>,
}

/// A datasource configuration entry from `matchbox.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct DatasourceEntry {
    pub driver: String,
    #[serde(default = "default_ds_host")]
    pub host: String,
    #[serde(default = "default_ds_port")]
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    #[serde(rename = "maxConnections", default = "default_max_connections")]
    pub max_connections: u32,
}

fn default_ds_host() -> String {
    "localhost".to_string()
}
fn default_ds_port() -> u16 {
    5432
}
fn default_max_connections() -> u32 {
    10
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Discover all modules for the current compilation.
///
/// Sources (in priority order — CLI `--module` flags override manifest entries with the same
/// directory name):
/// 1. `box.json` dependencies (following CommandBox conventions)
/// 2. `matchbox.toml` in `project_dir`
/// 3. `modules/` and `boxlang_modules/` directory scanning
/// 4. `extra_module_paths` collected from `--module <path>` CLI flags
///
/// Returns an empty `Vec` when neither source provides any modules, so callers can always
/// call this unconditionally.
pub fn discover_modules(
    project_dir: &Path,
    extra_module_paths: &[PathBuf],
) -> Result<Vec<ModuleInfo>> {
    // Collect (name, raw_path) pairs.  Later entries for the same name replace earlier ones.
    let mut entries: Vec<(String, PathBuf)> = Vec::new();

    // 1. Read box.json if present (CommandBox convention)
    let box_json_path = project_dir.join("box.json");
    if box_json_path.exists() {
        let text = std::fs::read_to_string(&box_json_path)
            .with_context(|| format!("Failed to read {}", box_json_path.display()))?;
        let box_json: BoxJson = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse {}", box_json_path.display()))?;

        // Merge dependencies and devDependencies
        let mut all_deps = box_json.dependencies;
        all_deps.extend(box_json.dev_dependencies);

        for (name, value) in all_deps {
            // If value looks like a relative path (starts with . or contains /)
            if value.starts_with('.') || value.contains('/') || value.contains('\\') {
                let raw = Path::new(&value);
                let path = if raw.is_absolute() {
                    raw.to_path_buf()
                } else {
                    project_dir.join(raw)
                };
                if path.exists() {
                    entries.push((name, path));
                    continue;
                }
            }

            // Otherwise, look in modules/ or boxlang_modules/
            let mod_path = project_dir.join("modules").join(&name);
            if mod_path.exists() {
                entries.push((name, mod_path));
            } else {
                let bx_mod_path = project_dir.join("boxlang_modules").join(&name);
                if bx_mod_path.exists() {
                    entries.push((name, bx_mod_path));
                }
            }
        }
    }

    // 2. Read matchbox.toml if present.
    let manifest_path = project_dir.join("matchbox.toml");
    if manifest_path.exists() {
        let text = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let manifest: Manifest = toml::from_str(&text)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        for (name, entry) in manifest.modules {
            let raw = Path::new(&entry.path);
            let path = if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                project_dir.join(raw)
            };
            entries.retain(|(n, _)| n != &name);
            entries.push((name, path));
        }
    }

    // 3. Scan modules/ and boxlang_modules/ for any folders with ModuleConfig.bx not already added
    for dir_name in &["modules", "boxlang_modules"] {
        let dir_path = project_dir.join(dir_name);
        if dir_path.is_dir() {
            if let Ok(dir_entries) = std::fs::read_dir(dir_path) {
                for entry in dir_entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && path.join("ModuleConfig.bx").exists() {
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        if !entries.iter().any(|(n, _)| n == &name) {
                            entries.push((name, path));
                        }
                    }
                }
            }
        }
    }

    // 4. --module CLI overrides: derive name from the directory name, replace manifest entry
    //    with the same name so CLI always wins.
    for raw in extra_module_paths {
        let path = if raw.is_absolute() {
            raw.clone()
        } else {
            project_dir.join(raw)
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        entries.retain(|(n, _)| n != &name);
        entries.push((name, path));
    }

    // 3. Validate and resolve each entry.
    let mut modules = Vec::new();
    for (name, path) in entries {
        let path = path.canonicalize().with_context(|| {
            format!("Module '{}': path does not exist: {}", name, path.display())
        })?;

        let descriptor = path.join("ModuleConfig.bx");
        if !descriptor.exists() {
            bail!(
                "Module '{}' at '{}' is missing ModuleConfig.bx",
                name,
                path.display()
            );
        }

        // Collect bifs/*.bxs sources (sorted for determinism).
        let bifs_dir = path.join("bifs");
        let mut bif_sources = Vec::new();
        if bifs_dir.is_dir() {
            let mut bif_files: Vec<_> = std::fs::read_dir(&bifs_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let entry_path = e.path();
                    let ext = entry_path.extension().and_then(|x| x.to_str());
                    matches!(ext, Some("bxs") | Some("bx"))
                })
                .collect();
            bif_files.sort_by_key(|e| e.path());
            for entry in bif_files {
                let src = std::fs::read_to_string(entry.path()).with_context(|| {
                    format!(
                        "Module '{}': failed to read {}",
                        name,
                        entry.path().display()
                    )
                })?;
                bif_sources.push(src);
            }
        }

        let has_native = path.join("matchbox").join("Cargo.toml").exists();

        // Execute configure() + onLoad() in an isolated VM to collect module settings.
        let settings = execute_module_lifecycle(&name, &path);

        modules.push(ModuleInfo {
            name,
            path,
            bif_sources,
            has_native,
            settings,
        });
    }

    Ok(modules)
}

/// Read `[datasources.<name>]` sections from `matchbox.toml` in `project_dir`.
///
/// Returns a map of `datasource_name → DatasourceEntry`. Returns an empty map
/// when no matchbox.toml is found or when the file contains no `[datasources]`
/// section.
pub fn read_datasource_configs(project_dir: &Path) -> Result<HashMap<String, DatasourceEntry>> {
    let manifest_path = project_dir.join("matchbox.toml");
    if !manifest_path.exists() {
        return Ok(HashMap::new());
    }
    let text = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let manifest: Manifest = toml::from_str(&text)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
    Ok(manifest.datasources)
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2 — Module lifecycle execution
// ─────────────────────────────────────────────────────────────────────────────

/// Execute a module's `ModuleConfig.bx` lifecycle in an isolated VM.
///
/// The file must define a class named `ModuleConfig` with a `configure()` method
/// that sets `this.settings` to a struct, and an optional `onLoad()` method.
/// Errors are emitted as warnings; an empty JSON object is returned on failure.
pub fn execute_module_lifecycle(name: &str, path: &Path) -> serde_json::Value {
    let empty = serde_json::Value::Object(serde_json::Map::new());
    let descriptor = path.join("ModuleConfig.bx");

    let source = match std::fs::read_to_string(&descriptor) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "Warning: Module '{}': could not read ModuleConfig.bx: {}",
                name, e
            );
            return empty;
        }
    };

    // Build a wrapper that instantiates ModuleConfig, runs onLoad(), then calls
    // configure() and returns its result (the settings struct).
    let wrapper =
        format!("{source}\nmc = new ModuleConfig()\nmc.onLoad()\nreturn mc.configure()\n");

    let ast = match matchbox_compiler::parser::parse(&wrapper, Some(&descriptor.to_string_lossy()))
    {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "Warning: Module '{}': failed to parse ModuleConfig.bx: {}",
                name, e
            );
            return empty;
        }
    };

    let mut chunk = match matchbox_compiler::compile_with_treeshaking(
        &descriptor.to_string_lossy(),
        &ast,
        &wrapper,
        vec![],
        false,
        false,
        &[],
        &[],
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Warning: Module '{}': failed to compile ModuleConfig.bx: {}",
                name, e
            );
            return empty;
        }
    };

    chunk.reconstruct_functions();

    let mut vm = matchbox_vm::vm::VM::new();
    let result = match vm.interpret(chunk) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "Warning: Module '{}': ModuleConfig.bx lifecycle error: {}",
                name, e
            );
            return empty;
        }
    };

    let json = vm.bx_to_json(&result);
    if matches!(json, serde_json::Value::Object(_)) {
        json
    } else {
        empty
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// getModuleSettings BIF code generation
// ─────────────────────────────────────────────────────────────────────────────

/// Generate a BoxLang source snippet defining `getModuleSettings(name)` that returns
/// the compile-time-baked settings struct for the requested module name.
///
/// Injected as an extra prelude so the function is tree-shaken like any built-in.
pub fn generate_get_module_settings_bxs(modules: &[ModuleInfo]) -> String {
    // Strategy: per-module helper functions (struct built at top-level of each),
    // plus a default helper returning {}, dispatched via chained ternary ?:
    // to avoid if-block scoping issues (assignments inside Matchbox if-blocks
    // do not persist to outer scope).
    //
    // IMPORTANT: parameter names must be ALL LOWERCASE. The compiler stores
    // locals via `param.name.clone()` but resolve_local() lowercases the lookup
    // name, causing a case-mismatch that makes mixed-case params invisible.
    let mut bxs = String::new();
    let mut helpers: Vec<(String, String)> = Vec::new(); // (module_name, helper_fn_name)

    for m in modules {
        if let serde_json::Value::Object(ref map) = m.settings {
            if !map.is_empty() {
                let safe = m.name.replace('-', "_");
                let helper = format!("getModuleSettings_{safe}");
                bxs.push_str(&format!("function {}() {{\n", helper));
                bxs.push_str("    s = {}\n");
                for (k, v) in map {
                    bxs.push_str(&format!("    s.{} = {}\n", k, json_value_to_bxs(v)));
                }
                bxs.push_str("    return s\n");
                bxs.push_str("}\n");
                helpers.push((m.name.clone(), helper));
            }
        }
    }

    // Default (no-match) helper — avoids bare `{}` in ternary else position
    bxs.push_str("function getModuleSettings_default() {\n");
    bxs.push_str("    d = {}\n");
    bxs.push_str("    return d\n");
    bxs.push_str("}\n");

    // Build a chained ternary dispatched on "mn" (lowercase param — see note above).
    bxs.push_str("function getModuleSettings(mn) {\n");
    let mut expr = String::from("getModuleSettings_default()");
    for (mod_name, helper) in helpers.iter().rev() {
        expr = format!("(mn == \"{}\") ? {}() : {}", mod_name, helper, expr);
    }
    bxs.push_str(&format!("    result = {}\n", expr));
    bxs.push_str("    return result\n");
    bxs.push_str("}\n");
    bxs
}

fn json_value_to_bxs(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        _ => "{}".to_string(), // nested objects/arrays: simplified placeholder
    }
}
