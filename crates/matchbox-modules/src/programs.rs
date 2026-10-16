//! Compile module descriptors without executing them; resolve prebuilt native artifacts.
use crate::discovery::ModuleInfo;
use anyhow::{Context, Result, bail};
use matchbox_compiler::{ast::*, parser, tokenizer::{lex, TokenKind}};
use matchbox_vm::{
    Chunk,
    bundle::{Artifact, Bif, BifMember, Component, Module, ModuleClass, SharedLibrary, TARGET},
};
use serde::Deserialize;
use std::{collections::HashMap, fs, path::Path};

#[derive(Deserialize)]
struct Package {
    #[serde(default)]
    matchbox: Native,
}
#[derive(Default, Deserialize)]
struct Native {
    #[serde(default)]
    artifacts: HashMap<String, ArtifactLocation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactLocation {
    path: Option<String>,
    url: Option<String>,
    filename: Option<String>,
    sha256: String,
    abi: u32,
    #[serde(default)]
    libraries: Vec<LibraryLocation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LibraryLocation {
    path: Option<String>,
    url: Option<String>,
    filename: Option<String>,
    sha256: String,
}

pub fn prepare(modules: &[ModuleInfo], target: &str) -> Result<Vec<Module>> {
    prepare_modules(modules, target, true)
}
/// Installed runtime packages must contain their native bytes, never fetch them at startup.
pub fn prepare_installed(modules: &[ModuleInfo], target: &str) -> Result<Vec<Module>> {
    if modules.iter().any(|module| module.has_native) {
        bail!("Source-only native modules cannot load at runtime; install a package with prebuilt artifacts");
    }
    prepare_modules(modules, target, false)
}
fn prepare_modules(modules: &[ModuleInfo], target: &str, allow_remote: bool) -> Result<Vec<Module>> {
    modules
        .iter()
        .enumerate()
        .map(|(index, module)| {
            let source = fs::read_to_string(module.path.join("ModuleConfig.bx"))?;
            let class = format!("__mb_config_{index}");
            let instance = format!("__mb_module_{index}");
            let mut ast = parser::parse(&source, Some("ModuleConfig.bx"))?;
            rename_class(&mut ast, &class)?;
            // Both named and anonymous descriptor classes are supported.
            ast.extend(parser::parse(
                &format!("{instance} = new {class}();"),
                None,
            )?);
            let descriptor = compile(
                &ast,
                &source,
                &module.path.join("ModuleConfig.bx").to_string_lossy(),
                modules,
            )?;
            let mut bifs_ast = Vec::new();
            let mut bif_registrations = Vec::new();
            for (bif_index, (name, source)) in module.bif_classes.iter().enumerate() {
                let class = format!("__mb_bif_{index}_{bif_index}");
                let instance = format!("__mb_bif_instance_{index}_{bif_index}");
                let mut ast = parser::parse(source, Some(name))?;
                let function = format!("__mb_bif_function_{index}_{bif_index}");
                let (aliases, members) = bif_annotations(&ast, name)?;
                bif_registrations.push(Bif {
                    instance: instance.clone(),
                    function: function.clone(),
                    aliases,
                    members,
                });
                rename_class(&mut ast, &class)?;
                ast.iter()
                    .find_map(|s| {
                        let StatementKind::ClassDecl { members, .. } = &s.kind else {
                            return None;
                        };
                        members.iter().find_map(|m| {
                            let ClassMember::Statement(s) = m else {
                                return None;
                            };
                            let StatementKind::FunctionDecl { name, .. } = &s.kind else {
                                return None;
                            };
                            name.eq_ignore_ascii_case("invoke").then_some(())
                        })
                    })
                    .with_context(|| format!("Module {} BIF {name} needs invoke()", module.name))?;
                ast.extend(parser::parse(
                    &format!("{instance} = new {class}(); {function} = {instance}.invoke;"),
                    None,
                )?);
                bifs_ast.extend(ast);
            }
            for source in &module.bif_sources {
                bifs_ast.extend(parser::parse(source, None)?);
            }
            let bifs = compile(
                &bifs_ast,
                "",
                &module.path.join("bifs/module-bifs.bx").to_string_lossy(),
                modules,
            )?;
            let mut classes = Vec::new();
            let mut components = Vec::new();
            for (class_index, file) in class_files(&module.path)?.into_iter().enumerate() {
                let relative = file.strip_prefix(&module.path)?;
                let name = relative.with_extension("").to_string_lossy().replace(['/', '\\'], ".");
                let global = format!("__mb_class_{index}_{class_index}");
                let source = fs::read_to_string(&file)?;
                if file.extension().is_some_and(|extension| extension == "bxs") {
                    // ponytail: conservative lexer filter; ambiguous scripts still parse.
                    // Use parser-level declaration discovery if further exclusions are needed.
                    let lexed = lex(&source);
                    let tokens = lexed.tokens();
                    let has_class = tokens.iter().enumerate().any(|(i, token)| {
                        if token.kind != TokenKind::Class { return false; }
                        let previous = i.checked_sub(1).map(|p| tokens[p].kind);
                        let member = matches!(previous, Some(TokenKind::Dot | TokenKind::QuestionDot | TokenKind::ColonColon));
                        let key = matches!(previous, Some(TokenKind::LeftBrace | TokenKind::LeftBracket | TokenKind::Comma))
                            && tokens.get(i + 1).is_some_and(|next| next.kind == TokenKind::Colon);
                        !member && !key
                    });
                    if !has_class { continue; }
                }
                let mut ast = parser::parse(&source, file.to_str())?;
                if !ast.iter().any(|s| matches!(s.kind, StatementKind::ClassDecl { .. })) { continue; }
                // Loading a class must not execute surrounding script/application statements.
                ast.retain(|s| matches!(s.kind, StatementKind::Import { .. } | StatementKind::ClassDecl { .. } | StatementKind::InterfaceDecl { .. }));
                if let Some(component) = name.strip_prefix("components.").filter(|name| !name.contains('.') && file.extension().is_some_and(|extension| extension == "bx")) {
                    for feature in ["ignoreEnableOutputOnly", "autoEvaluateBodyExpressions"] {
                        if annotation_bool(&ast, feature)? { bail!("Module component {feature} is not yet supported"); }
                    }
                    components.push(Component {
                        class: global.clone(),
                        aliases: annotation_aliases(&ast, component, "BoxComponent")?,
                        allows_body: annotation_bool(&ast, "AllowsBody")?,
                        requires_body: annotation_bool(&ast, "RequiresBody")?,
                    });
                }
                rename_class(&mut ast, &global)?;
                classes.push(ModuleClass {
                    path: relative.to_string_lossy().into_owned(), global,
                    chunk: compile(&ast, &source, &file.to_string_lossy(), modules)?,
                });
            }
            let artifact = read_artifact(&module.path, target, allow_remote)?;
            Ok(Module {
                name: module.name.clone(),
                path: module.path.to_string_lossy().into_owned(),
                dependencies: module.dependencies.clone(),
                parent_module: module.parent_module.clone(),
                nested_modules: module.nested_modules.clone(),
                instance,
                descriptor,
                bifs,
                bif_registrations,
                classes,
                components,
                settings: module.settings.clone(),
                artifact,
            })
        })
        .collect()
}
fn class_files(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            // BIFs are compiled separately; module-root build outputs are not source packages.
            let excluded_root = directory == root
                && matches!(entry.file_name().to_str(), Some("bifs" | "build" | "dist" | "target" | "modules"));
            if excluded_root || entry.file_name() == ".git" { continue; }
            let kind = entry.file_type()?;
            if kind.is_dir() { visit(root, &path, files)?; }
            else if path.extension().is_some_and(|extension| extension == "bx" || extension == "bxs") && path != root.join("ModuleConfig.bx") {
                if !path.canonicalize()?.starts_with(root) { bail!("Module class path escapes module package: {}", path.display()); }
                files.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort();
    Ok(files)
}
fn annotation_value(expression: &Expression) -> Result<serde_json::Value> {
    use serde_json::Value;
    Ok(match &expression.kind {
        ExpressionKind::Identifier(name) => Value::String(name.clone()),
        ExpressionKind::Literal(Literal::String(parts)) => Value::String(
            parts
                .iter()
                .map(|part| match part {
                    StringPart::Text(text) => Ok(text.as_str()),
                    _ => bail!("BIF annotations must be literals"),
                })
                .collect::<Result<String>>()?,
        ),
        ExpressionKind::Literal(Literal::Array(values)) => {
            Value::Array(values.iter().map(annotation_value).collect::<Result<_>>()?)
        }
        ExpressionKind::Literal(Literal::Struct(values)) => Value::Object(
            values
                .iter()
                .map(|(key, value)| {
                    let key = annotation_value(key)?;
                    Ok((
                        key.as_str()
                            .context("Annotation keys must be strings")?
                            .to_lowercase(),
                        annotation_value(value)?,
                    ))
                })
                .collect::<Result<_>>()?,
        ),
        ExpressionKind::Literal(Literal::Boolean(value)) => Value::Bool(*value),
        ExpressionKind::Literal(Literal::Number(value)) => serde_json::json!(value),
        ExpressionKind::Literal(Literal::Null) => Value::Null,
        _ => bail!("BIF annotations must be literals"),
    })
}
fn class_annotation(ast: &[Statement], key: &str) -> Result<serde_json::Value> {
    let attributes = ast.iter().find_map(|statement| match &statement.kind {
        StatementKind::ClassDecl { attributes, .. } => Some(attributes), _ => None,
    }).context("Module registration needs a class")?;
    let Some(attribute) = attributes.iter().rev().find(|a| a.name.eq_ignore_ascii_case(key)) else { return Ok(serde_json::Value::Null); };
    if attribute.args.len() > 1 {
        return Ok(serde_json::Value::Array(attribute.args.iter().map(|arg| annotation_value(&arg.value)).collect::<Result<_>>()?));
    }
    attribute.args.first().map(|arg| annotation_value(&arg.value)).unwrap_or_else(|| Ok(serde_json::Value::String(String::new())))
}
fn annotation_bool(ast: &[Statement], key: &str) -> Result<bool> {
    match class_annotation(ast, key)? {
        serde_json::Value::Null => Ok(false),
        serde_json::Value::Bool(value) => Ok(value),
        serde_json::Value::String(value) if value.is_empty() => Ok(false),
        serde_json::Value::String(value) if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") => Ok(value.eq_ignore_ascii_case("true")),
        _ => bail!("{key} must be a boolean"),
    }
}
fn annotation_aliases(ast: &[Statement], name: &str, key: &str) -> Result<Vec<String>> {
    let mut aliases = match class_annotation(ast, key)? {
        serde_json::Value::String(alias) if !alias.trim().is_empty() => vec![alias],
        serde_json::Value::Array(aliases) => {
            let mut result = aliases
                .iter()
                .map(|alias| {
                    alias
                        .as_str()
                        .map(str::to_owned)
                        .with_context(|| format!("{key} aliases must be strings"))
                })
                .collect::<Result<Vec<_>>>()?;
            result.push(name.into());
            result
        }
        _ => vec![name.into()],
    };
    if aliases.iter().any(|name| name.trim().is_empty()) {
        bail!("{key} aliases cannot be empty");
    }
    let mut seen = std::collections::HashSet::new();
    aliases.retain(|name| seen.insert(name.to_lowercase()));
    Ok(aliases)
}
fn bif_annotations(ast: &[Statement], name: &str) -> Result<(Vec<String>, Vec<BifMember>)> {
    let aliases = annotation_aliases(ast, name, "BoxBIF")?;
    let mut members = Vec::new();
    let mut add_member = |type_name: &str, record: &serde_json::Value| -> Result<()> {
        let type_name = type_name.to_lowercase();
        let default_name = name.to_lowercase().replace(&type_name, "");
        let member = record
            .get("name")
            .map(|name| name.as_str().context("BoxMember name must be a string"))
            .transpose()?
            .unwrap_or(&default_name)
            .to_owned();
        let object_argument = record
            .get("objectargument")
            .map(|argument| {
                argument
                    .as_str()
                    .context("BoxMember objectArgument must be a string")
            })
            .transpose()?
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        members.push(BifMember {
            name: member,
            type_name,
            object_argument,
        });
        Ok(())
    };
    match class_annotation(ast, "boxMember")? {
        serde_json::Value::String(type_name) if type_name.trim().is_empty() => {
            bail!("BoxMember requires a type")
        }
        serde_json::Value::String(type_name) => add_member(&type_name, &serde_json::json!({}))?,
        serde_json::Value::Object(types) => {
            for (type_name, record) in types {
                if !record.is_object() {
                    bail!("BoxMember value must be a struct");
                }
                add_member(&type_name, &record)?;
            }
        }
        _ => (),
    }
    Ok((aliases, members))
}
fn rename_class(ast: &mut [Statement], name: &str) -> Result<()> {
    let mut found = false;
    for statement in ast {
        if let StatementKind::ClassDecl { name: old, .. } = &mut statement.kind {
            if found {
                bail!("Module descriptor/BIF must contain one class");
            }
            *old = name.into();
            found = true;
        }
    }
    if !found {
        bail!("Module descriptor/BIF must contain a class");
    }
    Ok(())
}
fn compile(
    ast: &[Statement],
    source: &str,
    filename: &str,
    modules: &[ModuleInfo],
) -> Result<Chunk> {
    matchbox_compiler::compile_with_treeshaking(
        filename,
        ast,
        source,
        vec![],
        false,
        false,
        !modules.is_empty(),
        &[],
    )
}
fn read_artifact(root: &Path, target: &str, allow_remote: bool) -> Result<Option<Artifact>> {
    let package = root.join("box.json");
    if !package.exists() {
        return Ok(None);
    }
    let package: Package = serde_json::from_str(&fs::read_to_string(package)?)?;
    if package.matchbox.artifacts.is_empty() {
        return Ok(None);
    }
    let location = package
        .matchbox
        .artifacts
        .get(target)
        .with_context(|| format!("No native artifact published for {target}"))?;
    if location.abi != 1 {
        bail!("Unsupported native artifact ABI {}", location.abi);
    }
    let remote_filename = if target.contains("windows") {
        "plugin.dll"
    } else if target.contains("apple") {
        "plugin.dylib"
    } else {
        "plugin.so"
    };
    let filename = location
        .filename
        .as_deref()
        .or_else(|| location.url.as_ref().map(|_| remote_filename));
    let plugin = read_library(
        root,
        location.path.as_deref(),
        location.url.as_deref(),
        filename,
        &location.sha256,
        allow_remote,
    )?;
    let libraries = location
        .libraries
        .iter()
        .map(|library| {
            read_library(
                root,
                library.path.as_deref(),
                library.url.as_deref(),
                library.filename.as_deref(),
                &library.sha256,
                allow_remote,
            )
        })
        .collect::<Result<_>>()?;
    let artifact = Artifact {
        target: target.into(),
        abi: location.abi,
        filename: plugin.filename,
        sha256: plugin.sha256,
        bytes: plugin.bytes,
        libraries,
    };
    artifact.validate_files()?;
    Ok(Some(artifact))
}
fn read_library(
    root: &Path,
    path: Option<&str>,
    url: Option<&str>,
    filename: Option<&str>,
    sha256: &str,
    allow_remote: bool,
) -> Result<SharedLibrary> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        bail!("Artifact SHA-256 must be 64 lowercase hexadecimal characters");
    }
    if let Some(filename) = filename {
        matchbox_vm::bundle::validate_filename(filename)?;
    }
    let (filename, bytes) = match (path, url) {
        (Some(path), None) => {
            let packaged = root.join(path);
            // Keep a package symlink's name, not the resolved versioned filename.
            let filename = filename.map(str::to_owned).unwrap_or(
                packaged
                    .file_name()
                    .context("Artifact has no filename")?
                    .to_string_lossy()
                    .into_owned(),
            );
            matchbox_vm::bundle::validate_filename(&filename)?;
            let path = packaged.canonicalize()?;
            if !path.starts_with(root.canonicalize()?) {
                bail!("Native artifact path escapes module package");
            }
            (filename, fs::read(path)?)
        }
        (None, Some(_)) if !allow_remote => bail!("Runtime module artifacts must be packaged locally; remote downloads are not allowed at server startup"),
        (None, Some(url)) => (
            filename
                .context("Remote dependent libraries require a filename")?
                .to_owned(),
            matchbox_vm::plugins::download(url).map_err(anyhow::Error::msg)?,
        ),
        _ => bail!("Native artifact must specify exactly one of path or url"),
    };
    if sha256 != matchbox_vm::bundle::hash(&bytes) {
        bail!("Published native artifact hash does not match its bytes: {filename}");
    }
    Ok(SharedLibrary {
        filename,
        sha256: sha256.into(),
        bytes,
    })
}
pub fn output_target(target: Option<&str>) -> &str {
    match target {
        None | Some("native") => TARGET,
        Some(target) => target,
    }
}
