//! Resources: files that travel with a program (data, templates, pictures), read with
//! `resourceRead( path )`, `resourceReadBinary( path )`, `resourceExists( path )` and
//! `resourceList( [dir] )`.
//!
//! The folders are a `resources/` folder next to the program plus each `--resources DIR`.
//! A file's path is its path inside its folder, with `/` separators (`catalog/apps.json`).
//! Run from source, the folders are read from disk; a native build packs every file into the
//! binary (`bundle::Application::resources`) and the runner serves them from memory, so a
//! program behaves the same both ways. Two folders with the same path is an error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

/// The files of a program, by resource path.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Resources {
    pub files: BTreeMap<String, Vec<u8>>,
}

enum Store {
    /// running from source: these folders, in order
    Folders(Vec<PathBuf>),
    /// a native binary: the files packed into it
    Packed(Arc<Resources>),
}

static STORE: RwLock<Option<Store>> = RwLock::new(None);

/// Serve resources from these folders (running from source, and what a native build packs).
pub fn use_folders(folders: Vec<PathBuf>) {
    *STORE.write().unwrap() = Some(Store::Folders(folders));
}

/// Serve the files packed into a native binary.
pub fn use_packed(resources: Resources) {
    *STORE.write().unwrap() = Some(Store::Packed(Arc::new(resources)));
}

/// The folders for a program: `resources/` next to it (when it exists), then the `--resources` ones.
pub fn folders_for(program: Option<&Path>, extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = program.and_then(|p| p.parent()) {
        let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
        let default = dir.join("resources");
        if default.is_dir() {
            out.push(default);
        }
    }
    for folder in extra {
        if !out.iter().any(|f| same_dir(f, folder)) {
            out.push(folder.clone());
        }
    }
    out
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// `catalog/apps.json`, `/catalog/apps.json` and `./catalog\apps.json` are one path; `..` is refused.
pub fn normalize(path: &str) -> Result<String, String> {
    let mut parts = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(format!("Resource path {path} may not contain ..")),
            p => parts.push(p),
        }
    }
    Ok(parts.join("/"))
}

/// Every file under the configured folders, read from disk (what a native build packs).
pub fn collect() -> Result<Resources, String> {
    let folders = match STORE.read().unwrap().as_ref() {
        Some(Store::Folders(f)) => f.clone(),
        Some(Store::Packed(r)) => return Ok((**r).clone()),
        None => Vec::new(),
    };
    let mut files = BTreeMap::new();
    let mut from: BTreeMap<String, PathBuf> = BTreeMap::new();
    for folder in &folders {
        if !folder.is_dir() {
            return Err(format!("Resource folder {} does not exist", folder.display()));
        }
        let mut found = Vec::new();
        walk(folder, folder, &mut found)?;
        for (rel, file) in found {
            if let Some(first) = from.get(&rel) {
                return Err(format!("Resource {rel} is in two folders: {} and {}", first.display(), folder.display()));
            }
            let bytes = std::fs::read(&file).map_err(|e| format!("Cannot read resource {}: {e}", file.display()))?;
            files.insert(rel.clone(), bytes);
            from.insert(rel, folder.clone());
        }
    }
    Ok(Resources { files })
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("Cannot list {}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out)?;
        } else if path.is_file() {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
            out.push((rel, path));
        }
    }
    Ok(())
}

/// The bytes of one resource.
pub fn read(path: &str) -> Result<Vec<u8>, String> {
    let rel = normalize(path)?;
    let store = STORE.read().unwrap();
    match store.as_ref() {
        Some(Store::Packed(r)) => r.files.get(&rel).cloned().ok_or_else(|| not_found(&rel, "it was not packed into this program")),
        Some(Store::Folders(folders)) => {
            let hits: Vec<PathBuf> = folders.iter().map(|f| f.join(&rel)).filter(|p| p.is_file()).collect();
            match hits.len() {
                0 => Err(not_found(&rel, &format!("looked in {}", describe(folders)))),
                1 => std::fs::read(&hits[0]).map_err(|e| format!("Cannot read resource {rel}: {e}")),
                _ => Err(format!("Resource {rel} is in two folders: {} and {}", hits[0].display(), hits[1].display())),
            }
        }
        None => Err(not_found(&rel, "this program has no resource folders")),
    }
}

fn not_found(rel: &str, why: &str) -> String {
    format!("Resource {rel} not found ({why})")
}

fn describe(folders: &[PathBuf]) -> String {
    if folders.is_empty() {
        return "no resource folders: add resources/ next to the program or pass --resources DIR".into();
    }
    folders.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")
}

pub fn exists(path: &str) -> bool {
    let Ok(rel) = normalize(path) else { return false };
    match STORE.read().unwrap().as_ref() {
        Some(Store::Packed(r)) => r.files.contains_key(&rel),
        Some(Store::Folders(folders)) => folders.iter().any(|f| f.join(&rel).is_file()),
        None => false,
    }
}

/// The resource paths under `dir` ("" for all), sorted.
pub fn list(dir: &str) -> Result<Vec<String>, String> {
    let prefix = normalize(dir)?;
    let folders = match STORE.read().unwrap().as_ref() {
        Some(Store::Packed(r)) => return Ok(filter(r.files.keys().cloned().collect(), &prefix)),
        Some(Store::Folders(f)) => f.clone(),
        None => return Ok(Vec::new()),
    };
    let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();
    for folder in folders.iter().filter(|f| f.is_dir()) {
        let mut found = Vec::new();
        walk(folder, folder, &mut found)?;
        for (rel, _) in found {
            if let Some(first) = seen.insert(rel.clone(), folder.clone()) {
                return Err(format!("Resource {rel} is in two folders: {} and {}", first.display(), folder.display()));
            }
        }
    }
    Ok(filter(seen.into_keys().collect(), &prefix))
}

fn filter(all: Vec<String>, prefix: &str) -> Vec<String> {
    if prefix.is_empty() {
        return all;
    }
    let with_slash = format!("{prefix}/");
    all.into_iter().filter(|p| p.starts_with(&with_slash)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_normalized_and_dot_dot_is_refused() {
        assert_eq!(normalize("/a/./b\\c.json").unwrap(), "a/b/c.json");
        assert!(normalize("a/../b").is_err());
    }
}
