use anyhow::{Context, Result};
use matchbox_compiler::{
    ast::{Statement, StatementKind},
    compile_with_treeshaking, parser,
};
use matchbox_embedded::{
    EmbeddedAppDefinition, EmbeddedRoute, EmbeddedSourceKind, route_from_app_file,
    validate_embedded_app,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddedBuildManifest {
    pub app_root: PathBuf,
    pub application_path: Option<PathBuf>,
    pub app: EmbeddedAppDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddedRouteTable {
    pub application: Option<EmbeddedApplicationEntry>,
    pub routes: Vec<EmbeddedRouteTableEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddedApplicationEntry {
    pub source_path: String,
    pub bytecode: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddedRouteTableEntry {
    pub method: String,
    pub path: String,
    pub source_kind: String,
    pub source_path: String,
    pub bytecode: Vec<u8>,
}

pub fn discover_embedded_app(project_root: &Path) -> Result<Option<EmbeddedBuildManifest>> {
    let app_root = project_root.join("app");
    let application_path = discover_application_file(project_root);

    if !app_root.exists() || !app_root.is_dir() {
        if application_path.is_none() {
            return Ok(None);
        }
        let app = EmbeddedAppDefinition::default();
        return Ok(Some(EmbeddedBuildManifest {
            app_root,
            application_path,
            app,
        }));
    }

    let mut files = Vec::new();
    collect_embedded_files(&app_root, &mut files)?;

    let mut app = EmbeddedAppDefinition::default();
    for file in files {
        let route = route_from_app_file(&app_root, &file)?;
        app.routes.push(route);
    }

    if app.routes.is_empty() && application_path.is_none() {
        return Ok(None);
    }

    validate_embedded_app(&app)?;

    Ok(Some(EmbeddedBuildManifest {
        app_root,
        application_path,
        app,
    }))
}

pub fn write_embedded_manifest(
    build_dir: &Path,
    manifest: &EmbeddedBuildManifest,
) -> Result<PathBuf> {
    let manifest_path = build_dir.join("embedded-app-manifest.json");
    let json = serde_json::to_vec_pretty(manifest)?;
    fs::write(&manifest_path, json)
        .with_context(|| format!("Failed to write {}", manifest_path.display()))?;
    Ok(manifest_path)
}

pub fn build_embedded_route_table(manifest: &EmbeddedBuildManifest) -> Result<EmbeddedRouteTable> {
    let application = manifest
        .application_path
        .as_ref()
        .map(|path| application_to_table_entry(path))
        .transpose()?;
    let mut routes = Vec::with_capacity(manifest.app.routes.len());
    for route in &manifest.app.routes {
        routes.push(route_to_table_entry(route)?);
    }
    Ok(EmbeddedRouteTable {
        application,
        routes,
    })
}

pub fn write_embedded_route_table(
    build_dir: &Path,
    manifest: &EmbeddedBuildManifest,
) -> Result<PathBuf> {
    let route_table = build_embedded_route_table(manifest)?;
    let path = build_dir.join("embedded-route-table.json");
    let bytes = postcard::to_stdvec(&route_table)?;
    fs::write(&path, bytes).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(path)
}

fn collect_embedded_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("Failed to read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_embedded_files(&path, files)?;
            continue;
        }

        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
        {
            Some(ext) if ext == "bxm" || ext == "bxs" => files.push(path),
            _ => {}
        }
    }

    files.sort();
    Ok(())
}

fn discover_application_file(project_root: &Path) -> Option<PathBuf> {
    let root_application = project_root.join("Application.bx");
    if root_application.is_file() {
        return Some(root_application);
    }

    let starter_application = project_root
        .join("src")
        .join("main")
        .join("bx")
        .join("Application.bx");
    if starter_application.is_file() {
        return Some(starter_application);
    }

    None
}

fn application_to_table_entry(path: &Path) -> Result<EmbeddedApplicationEntry> {
    let bytecode = compile_source_file(path)
        .with_context(|| format!("Failed to compile application {}", path.display()))?;

    Ok(EmbeddedApplicationEntry {
        source_path: path.display().to_string(),
        bytecode,
    })
}

fn route_to_table_entry(route: &EmbeddedRoute) -> Result<EmbeddedRouteTableEntry> {
    let bytecode =
        compile_source_file_with_kind(&route.source_path, Some(route.source_kind.clone()))?;

    Ok(EmbeddedRouteTableEntry {
        method: route.method.clone(),
        path: route.path.clone(),
        source_kind: match route.source_kind {
            EmbeddedSourceKind::Template => "template".to_string(),
            EmbeddedSourceKind::Script => "script".to_string(),
        },
        source_path: route.source_path.display().to_string(),
        bytecode,
    })
}

fn compile_source_file(path: &Path) -> Result<Vec<u8>> {
    compile_source_file_with_kind(path, None)
}

fn compile_source_file_with_kind(
    path: &Path,
    source_kind: Option<EmbeddedSourceKind>,
) -> Result<Vec<u8>> {
    let source =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let mut ast = match source_kind {
        Some(EmbeddedSourceKind::Template) => {
            parser::parse_bxm(&source, Some(&path.to_string_lossy()))
                .with_context(|| format!("Failed to parse template {}", path.display()))?
        }
        Some(EmbeddedSourceKind::Script) | None => {
            parser::parse(&source, Some(&path.to_string_lossy()))
                .with_context(|| format!("Failed to parse script {}", path.display()))?
        }
    };

    infer_class_name_from_filename(&mut ast, path);
    let mut chunk = compile_with_treeshaking(
        &path.display().to_string(),
        &ast,
        &source,
        vec![],
        false,
        false,
        false,
        &[],
    )
    .with_context(|| format!("Failed to compile {}", path.display()))?;
    chunk.reconstruct_functions();
    postcard::to_stdvec(&chunk).with_context(|| format!("Failed to serialize {}", path.display()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn discovers_embedded_routes_from_app_directory() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("matchbox-embedded-discovery-{}", nonce));
        let app_dir = root.join("app").join("printer");
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(root.join("app").join("index.bxm"), "<h1>Home</h1>").unwrap();
        fs::write(app_dir.join("[id].bxm"), "<h1>#url.id#</h1>").unwrap();
        fs::write(
            root.join("app").join("print.post.bxs"),
            "writeOutput( 'ok' );",
        )
        .unwrap();

        let manifest = discover_embedded_app(&root).unwrap().unwrap();
        let routes: Vec<(String, String)> = manifest
            .app
            .routes
            .iter()
            .map(|route| (route.method.clone(), route.path.clone()))
            .collect();

        assert!(routes.contains(&("GET".to_string(), "/".to_string())));
        assert!(routes.contains(&("GET".to_string(), "/printer/:id".to_string())));
        assert!(routes.contains(&("POST".to_string(), "/print".to_string())));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn builds_route_table_entries_from_manifest() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("matchbox-embedded-route-table-{}", nonce));
        let app_dir = root.join("app");
        fs::create_dir_all(&app_dir).unwrap();
        let source_path = app_dir.join("print.post.bxs");
        fs::write(&source_path, "writeOutput( 'ok' );").unwrap();

        let manifest = EmbeddedBuildManifest {
            app_root: app_dir.clone(),
            application_path: None,
            app: EmbeddedAppDefinition {
                listen: Default::default(),
                routes: vec![EmbeddedRoute {
                    method: "POST".to_string(),
                    path: "/print".to_string(),
                    source_path: source_path.clone(),
                    source_kind: EmbeddedSourceKind::Script,
                }],
            },
        };

        let table = build_embedded_route_table(&manifest).unwrap();
        assert_eq!(table.routes.len(), 1);
        assert_eq!(table.routes[0].method, "POST");
        assert_eq!(table.routes[0].path, "/print");
        assert_eq!(table.routes[0].source_kind, "script");
        assert!(!table.routes[0].bytecode.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn includes_application_lifecycle_bytecode_when_application_bx_exists() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("matchbox-embedded-application-{}", nonce));
        let app_dir = root.join("app");
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(
            root.join("Application.bx"),
            r#"
                class {
                    function onApplicationStart() {
                        println( "started" );
                    }
                }
            "#,
        )
        .unwrap();
        fs::write(app_dir.join("index.bxm"), "<h1>Home</h1>").unwrap();

        let manifest = discover_embedded_app(&root).unwrap().unwrap();
        let table = build_embedded_route_table(&manifest).unwrap();

        assert!(table.application.is_some());
        assert_eq!(table.routes.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discovers_application_lifecycle_without_app_routes() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("matchbox-embedded-application-only-{}", nonce));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Application.bx"),
            r#"
                class {
                    function onApplicationStart() {
                        println( "configured" );
                    }
                }
            "#,
        )
        .unwrap();

        let manifest = discover_embedded_app(&root).unwrap().unwrap();
        assert!(manifest.application_path.is_some());
        assert!(manifest.app.routes.is_empty());

        let table = build_embedded_route_table(&manifest).unwrap();
        assert!(table.application.is_some());
        assert!(table.routes.is_empty());

        let _ = fs::remove_dir_all(&root);
    }
}
