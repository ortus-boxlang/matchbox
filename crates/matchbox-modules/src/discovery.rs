use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// A resolved, validated module ready for use in compilation.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// The module name (e.g. `"bx-strings"`). Becomes the `bxModules.{name}` namespace.
    pub name: String,
    pub dependencies: Vec<String>,
    pub parent_module: Option<String>,
    pub nested_modules: Vec<String>,
    /// Absolute physical path to the module root directory.
    pub path: PathBuf,
    /// Contents of each `bifs/*.bxs` (or `.bx`) file in this module, injected as extra
    /// prelude sources during tree-shaking.
    pub bif_sources: Vec<String>,
    /// Whether this module contains a `matchbox/Cargo.toml` for native Rust compilation.
    pub has_native: bool,
    /// BoxLang class BIFs, registered using their invoke() method at runtime.
    pub bif_classes: Vec<(String, String)>,
    /// Application settings overrides; configure() runs only in the application VM.
    pub settings: serde_json::Value,
}

// ─────────────────────────────────────────────────────────────────────────────
// matchbox.toml deserialization
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    modules: HashMap<String, ManifestEntry>,
    #[allow(dead_code)]
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
#[allow(dead_code)]
pub struct DatasourceEntry {
    pub driver: String,
    #[serde(default = "default_ds_host")]
    pub host: String,
    #[serde(default = "default_ds_port")]
    pub port: u16,
    pub database: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
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
/// 3. Project-local `boxlang_modules/` directory scanning
/// 4. `extra_module_paths` collected from `--module <path>` CLI flags
/// 5. `BOXLANG_HOME/modules/` supplies packages not installed/selected locally
///
/// Returns an empty `Vec` when neither source provides any modules, so callers can always
/// call this unconditionally.
pub fn discover_modules(
    project_dir: &Path,
    extra_module_paths: &[PathBuf],
) -> Result<Vec<ModuleInfo>> {
    let home = std::env::var_os("BOXLANG_HOME").map(PathBuf::from);
    discover_modules_with_home(project_dir, extra_module_paths, home.as_deref())
}

/// Explicit home selection avoids changing process-wide environment in embedded hosts.
pub fn discover_modules_with_home(
    project_dir: &Path,
    extra_module_paths: &[PathBuf],
    home: Option<&Path>,
) -> Result<Vec<ModuleInfo>> {
    discover_modules_with_config(project_dir, extra_module_paths, home, None)
}

/// Modules and all discovered installation/package paths that HTTP hosts must keep private.
pub struct DiscoveredModules {
    pub modules: Vec<ModuleInfo>,
    pub private_paths: Vec<PathBuf>,
}

pub fn discover_modules_with_config(
    project_dir: &Path,
    extra_module_paths: &[PathBuf],
    home: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<Vec<ModuleInfo>> {
    Ok(discover_module_installations(project_dir, extra_module_paths, home, config_path)?.modules)
}

/// Resolve configuration once so package selection and HTTP privacy use the same snapshot.
pub fn discover_module_installations(
    project_dir: &Path,
    extra_module_paths: &[PathBuf],
    home: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<DiscoveredModules> {
    let mut private_paths = Vec::new();
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

            let mod_path = project_dir.join("boxlang_modules").join(&name);
            if mod_path.exists() {
                entries.push((name, mod_path));
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

    // 3. Scan the project installation directory for packages not explicitly configured.
    scan_module_directory(
        &project_dir.join("boxlang_modules"),
        &mut entries,
        &mut private_paths,
    )?;

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

    // The runtime home is lower priority than project packages and explicit selections.
    if let Some(home) = home {
        scan_module_directory(&home.join("modules"), &mut entries, &mut private_paths)?;
    }

    let file = config_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| project_dir.join("boxlang.json"));
    let config: serde_json::Value = if config_path.is_some() || file.exists() {
        serde_json::from_str(&std::fs::read_to_string(&file)?)
            .with_context(|| format!("Invalid {}", file.display()))?
    } else {
        serde_json::json!({})
    };
    if config_path.is_some()
        && !config.as_object().is_some_and(|object| {
            object
                .keys()
                .all(|key| matches!(key.as_str(), "modules" | "modulesDirectory"))
        })
    {
        bail!(
            "Native runtime configuration currently supports only modules enabled/settings and modulesDirectory; other runtime configuration remains unsupported"
        );
    }

    if let Some(directories) = config.get("modulesDirectory") {
        for directory in directories
            .as_array()
            .context("modulesDirectory must be an array of strings")?
        {
            let directory = directory
                .as_str()
                .context("modulesDirectory entries must be strings")?;
            if directory.trim().is_empty() {
                continue;
            }
            let directory = project_dir.join(directory);
            scan_module_directory(&directory, &mut entries, &mut private_paths)?;
        }
    }

    if let Some(entries) = config.get("modules") {
        let entries = entries
            .as_object()
            .context("modules configuration must be an object")?;
        for (name, entry) in entries {
            let entry = entry
                .as_object()
                .with_context(|| format!("Module {name} configuration must be an object"))?;
            if entry
                .keys()
                .any(|key| !matches!(key.as_str(), "enabled" | "settings"))
            {
                bail!("Module {name} supports only enabled/settings configuration");
            }
            if let Some(enabled) = entry.get("enabled")
                && !enabled.is_null()
                && !enabled.is_boolean()
            {
                bail!("Module {name} enabled must be boolean");
            }
            if let Some(settings) = entry.get("settings")
                && !settings.is_null()
                && !settings.is_object()
            {
                bail!("Module {name} settings must be an object");
            }
        }
    }
    // Validate and resolve each entry.
    let mut modules: Vec<ModuleInfo> = Vec::new();
    let mut pending: std::collections::VecDeque<_> = entries
        .into_iter()
        .map(|(name, path)| {
            (
                name,
                path,
                None::<String>,
                Vec::<PathBuf>::new(),
                serde_json::json!({}),
            )
        })
        .collect();
    while let Some((mut name, path, parent_module, mut ancestors, parent_overrides)) =
        pending.pop_front()
    {
        let path = path.canonicalize().with_context(|| {
            format!("Module '{}': path does not exist: {}", name, path.display())
        })?;

        if ancestors.contains(&path) {
            bail!("Circular nested module path: {}", path.display());
        }
        ancestors.push(path.clone());
        if !private_paths.contains(&path) {
            private_paths.push(path.clone());
        }
        let descriptor = path.join("ModuleConfig.bx");
        if !descriptor.exists() {
            bail!(
                "Module '{}' at '{}' is missing ModuleConfig.bx",
                name,
                path.display()
            );
        }

        let package_path = path.join("box.json");
        let package: serde_json::Value = if package_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(package_path)?)?
        } else {
            serde_json::json!({})
        };
        if let Some(declared) = package["boxlang"]["moduleName"].as_str() {
            name = declared.to_owned();
        }
        if parent_module.is_some()
            && modules
                .iter()
                .any(|module| module.name.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        let mut overrides = parent_overrides
            .as_object()
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(&name))
            })
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| serde_json::json!({}));
        let mut global = config["modules"][&name].clone();
        if let Some(entries) = global.as_object_mut() {
            entries.retain(|_, value| !value.is_null());
        }
        merge_configuration(&mut overrides, &global);
        if overrides["enabled"] == false {
            continue;
        }
        let source = std::fs::read_to_string(&descriptor)?;
        let metadata = descriptor_metadata(&source)?;
        if !overrides["enabled"].as_bool().unwrap_or(metadata.enabled) {
            continue;
        }
        let mut children = Vec::new();
        scan_module_directory(&path.join("modules"), &mut children, &mut private_paths)?;
        for (child, child_path) in children {
            pending.push_back((
                child,
                child_path,
                Some(name.clone()),
                ancestors.clone(),
                metadata.modules.clone(),
            ));
        }
        // Collect bifs/*.bxs sources (sorted for determinism).
        let bifs_dir = path.join("bifs");
        let mut bif_sources = Vec::new();
        let mut bif_classes = Vec::new();
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
                let file = entry.path();
                let ast = matchbox_compiler::parser::parse(&src, file.to_str())?;
                if ast.iter().any(|s| {
                    matches!(
                        s.kind,
                        matchbox_compiler::ast::StatementKind::ClassDecl { .. }
                    )
                }) {
                    bif_classes.push((
                        file.file_stem().unwrap().to_string_lossy().into_owned(),
                        src,
                    ));
                } else {
                    bif_sources.push(src);
                }
            }
        }

        // Published artifacts take precedence; never silently compile a mismatched target.
        let has_artifacts = package["matchbox"]["artifacts"]
            .as_object()
            .is_some_and(|a| !a.is_empty());
        let has_native = !has_artifacts && path.join("matchbox").join("Cargo.toml").exists();

        let settings = overrides["settings"]
            .as_object()
            .map(|m| serde_json::Value::Object(m.clone()))
            .unwrap_or_else(|| serde_json::json!({}));

        modules.push(ModuleInfo {
            name,
            dependencies: metadata.dependencies,
            parent_module,
            nested_modules: Vec::new(),
            path,
            bif_sources,
            bif_classes,
            has_native,
            settings,
        });
    }

    for index in 0..modules.len() {
        modules[index].nested_modules = modules
            .iter()
            .filter(|child| child.parent_module.as_deref() == Some(modules[index].name.as_str()))
            .map(|child| child.name.clone())
            .collect();
    }
    Ok(DiscoveredModules {
        modules: order_modules(modules)?,
        private_paths,
    })
}

fn scan_module_directory(
    directory: &Path,
    entries: &mut Vec<(String, PathBuf)>,
    private_paths: &mut Vec<PathBuf>,
) -> Result<()> {
    let metadata = match std::fs::metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Reading module directory {}", directory.display()));
        }
    };
    if !metadata.is_dir() {
        eprintln!(
            "Warning: module directory {} is not a directory; ignoring",
            directory.display()
        );
        return Ok(());
    }
    let canonical = directory.canonicalize()?;
    if !private_paths.contains(&canonical) {
        private_paths.push(canonical);
    }
    let mut paths = std::fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    for path in paths {
        if !path.is_dir() || !path.join("ModuleConfig.bx").is_file() {
            continue;
        }
        let canonical = path.canonicalize()?;
        if !private_paths.contains(&canonical) {
            private_paths.push(canonical);
        }
        let name = path
            .file_name()
            .context("Module directory has no name")?
            .to_string_lossy()
            .into_owned();
        if !entries
            .iter()
            .any(|(selected, _)| selected.eq_ignore_ascii_case(&name))
        {
            entries.push((name, path));
        }
    }
    Ok(())
}

/// Read `[datasources.<name>]` sections from `matchbox.toml` in `project_dir`.
///
/// Returns a map of `datasource_name → DatasourceEntry`. Returns an empty map
/// when no matchbox.toml is found or when the file contains no `[datasources]`
/// section.
#[allow(dead_code)]
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

struct DescriptorMetadata {
    enabled: bool,
    dependencies: Vec<String>,
    modules: serde_json::Value,
}
fn merge_configuration(target: &mut serde_json::Value, source: &serde_json::Value) {
    if let Some(source) = source.as_object() {
        if !target.is_object() {
            *target = serde_json::json!({});
        }
        let target = target.as_object_mut().unwrap();
        for (key, value) in source {
            if value.is_object() {
                merge_configuration(
                    target
                        .entry(key.clone())
                        .or_insert_with(|| serde_json::json!({})),
                    value,
                );
            } else {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}
fn literal_configuration(
    expression: &matchbox_compiler::ast::Expression,
    key: bool,
) -> Result<serde_json::Value> {
    use matchbox_compiler::ast::*;
    Ok(match &expression.kind {
        ExpressionKind::Identifier(name) if key => serde_json::Value::String(name.to_lowercase()),
        ExpressionKind::Literal(Literal::String(parts)) => serde_json::Value::String(
            parts
                .iter()
                .map(|part| match part {
                    StringPart::Text(text) => Ok(text.as_str()),
                    _ => bail!("Nested module overrides must be literals"),
                })
                .collect::<Result<String>>()?,
        ),
        ExpressionKind::Literal(Literal::Boolean(value)) => serde_json::Value::Bool(*value),
        ExpressionKind::Literal(Literal::Number(value)) => serde_json::json!(value),
        ExpressionKind::Literal(Literal::Null) => serde_json::Value::Null,
        ExpressionKind::Literal(Literal::Array(items)) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| literal_configuration(item, false))
                .collect::<Result<_>>()?,
        ),
        ExpressionKind::Literal(Literal::Struct(items)) => serde_json::Value::Object(
            items
                .iter()
                .map(|(key, value)| {
                    Ok((
                        literal_configuration(key, true)?
                            .as_str()
                            .context("Nested override keys must be strings")?
                            .to_lowercase(),
                        literal_configuration(value, false)?,
                    ))
                })
                .collect::<Result<_>>()?,
        ),
        _ => bail!(
            "Nested module overrides must be literals; dynamic descriptor configuration is unsupported"
        ),
    })
}
fn descriptor_metadata(source: &str) -> Result<DescriptorMetadata> {
    use matchbox_compiler::ast::*;
    fn string(expression: &Expression) -> Option<String> {
        match &expression.kind {
            ExpressionKind::Literal(Literal::String(parts))
                if parts.iter().all(|p| matches!(p, StringPart::Text(_))) =>
            {
                Some(
                    parts
                        .iter()
                        .filter_map(|p| {
                            if let StringPart::Text(s) = p {
                                Some(s.as_str())
                            } else {
                                None
                            }
                        })
                        .collect(),
                )
            }
            _ => None,
        }
    }
    let mut result = DescriptorMetadata {
        enabled: true,
        dependencies: vec![],
        modules: serde_json::json!({}),
    };
    for statement in matchbox_compiler::parser::parse(source, Some("ModuleConfig.bx"))? {
        let StatementKind::ClassDecl { members, .. } = statement.kind else {
            continue;
        };
        for member in members {
            let ClassMember::Statement(Statement {
                kind: StatementKind::Expression(expression),
                ..
            }) = member
            else {
                continue;
            };
            let ExpressionKind::Assignment {
                target: AssignmentTarget::Member { base, member },
                value,
            } = expression.kind
            else {
                continue;
            };
            if !matches!(base.kind, ExpressionKind::Identifier(ref name) if name.eq_ignore_ascii_case("this"))
            {
                continue;
            }
            match member.to_lowercase().as_str() {
                "enabled" => {
                    let ExpressionKind::Literal(Literal::Boolean(enabled)) = value.kind else {
                        bail!("Module enabled metadata must be a boolean literal");
                    };
                    result.enabled = enabled;
                }
                "modules" => {
                    result.modules = literal_configuration(&value, false)?;
                    let entries = result
                        .modules
                        .as_object()
                        .context("Nested module overrides must be a struct")?;
                    for (name, entry) in entries {
                        let entry = entry.as_object().with_context(|| {
                            format!("Nested module {name} overrides must be a struct")
                        })?;
                        if entry
                            .keys()
                            .any(|key| !matches!(key.as_str(), "enabled" | "settings"))
                            || entry
                                .get("enabled")
                                .is_some_and(|value| !value.is_boolean())
                            || entry
                                .get("settings")
                                .is_some_and(|value| !value.is_object())
                        {
                            bail!(
                                "Nested module {name} supports only boolean enabled and struct settings"
                            );
                        }
                    }
                }
                "dependencies" => {
                    let ExpressionKind::Literal(Literal::Array(values)) = value.kind else {
                        bail!("Module dependencies must be an array literal");
                    };
                    result.dependencies = values
                        .iter()
                        .map(|v| string(v).context("Module dependencies must contain names"))
                        .collect::<Result<_>>()?;
                }
                _ => (),
            }
        }
    }
    Ok(result)
}
fn order_modules(modules: Vec<ModuleInfo>) -> Result<Vec<ModuleInfo>> {
    let mut names = HashMap::new();
    for (index, module) in modules.iter().enumerate() {
        if names.insert(module.name.to_lowercase(), index).is_some() {
            bail!("Duplicate module name: {}", module.name);
        }
    }
    fn visit(
        index: usize,
        modules: &[ModuleInfo],
        names: &HashMap<String, usize>,
        states: &mut [u8],
        order: &mut Vec<usize>,
    ) -> Result<()> {
        if states[index] == 2 {
            return Ok(());
        }
        if states[index] == 1 {
            bail!(
                "Circular module dependency involving {}",
                modules[index].name
            );
        }
        states[index] = 1;
        for dependency in modules[index]
            .nested_modules
            .iter()
            .chain(&modules[index].dependencies)
        {
            let next = *names.get(&dependency.to_lowercase()).with_context(|| {
                format!(
                    "Module {} requires missing module {dependency}",
                    modules[index].name
                )
            })?;
            visit(next, modules, names, states, order)?;
        }
        states[index] = 2;
        order.push(index);
        Ok(())
    }
    let mut states = vec![0; modules.len()];
    let mut order = Vec::new();
    for index in 0..modules.len() {
        visit(index, &modules, &names, &mut states, &mut order)?;
    }
    Ok(order
        .into_iter()
        .map(|index| modules[index].clone())
        .collect())
}
