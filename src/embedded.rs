use anyhow::{Context, Result};
use matchbox_compiler::{compile_with_treeshaking, parser};
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
    pub app: EmbeddedAppDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddedRouteTable {
    pub routes: Vec<EmbeddedRouteTableEntry>,
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
    if !app_root.exists() || !app_root.is_dir() {
        return Ok(None);
    }

    let mut files = Vec::new();
    collect_embedded_files(&app_root, &mut files)?;

    let mut app = EmbeddedAppDefinition::default();
    for file in files {
        let route = route_from_app_file(&app_root, &file)?;
        app.routes.push(route);
    }

    if app.routes.is_empty() {
        return Ok(None);
    }

    validate_embedded_app(&app)?;

    Ok(Some(EmbeddedBuildManifest { app_root, app }))
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
    let mut routes = Vec::with_capacity(manifest.app.routes.len());
    for route in &manifest.app.routes {
        routes.push(route_to_table_entry(route)?);
    }
    Ok(EmbeddedRouteTable { routes })
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

fn route_to_table_entry(route: &EmbeddedRoute) -> Result<EmbeddedRouteTableEntry> {
    let source = fs::read_to_string(&route.source_path)
        .with_context(|| format!("Failed to read {}", route.source_path.display()))?;
    let ast = match route.source_kind {
        EmbeddedSourceKind::Template => {
            parser::parse_bxm(&source, Some(&route.source_path.to_string_lossy())).with_context(
                || format!("Failed to parse template {}", route.source_path.display()),
            )?
        }
        EmbeddedSourceKind::Script => {
            parser::parse(&source, Some(&route.source_path.to_string_lossy())).with_context(
                || format!("Failed to parse script {}", route.source_path.display()),
            )?
        }
    };

    let mut chunk = compile_with_treeshaking(
        &route.source_path.display().to_string(),
        &ast,
        &source,
        vec![],
        false,
        false,
        &[],
        &[],
    )
    .with_context(|| format!("Failed to compile {}", route.source_path.display()))?;
    chunk.reconstruct_functions();
    let bytecode = postcard::to_stdvec(&chunk)
        .with_context(|| format!("Failed to serialize {}", route.source_path.display()))?;

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
}
