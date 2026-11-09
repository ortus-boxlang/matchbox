//! BoxLang mappings: virtual paths such as `/models` that point to an absolute directory.
//!
//! Same model as BoxLang on the JVM (`ortus.boxlang.runtime.util.Mapping`): a name with a leading
//! and trailing slash, an absolute path and an `external` flag. A mapping value is either a string
//! path or a struct `{ path, external }`; `external` defaults to `true`.
//!
//! The mappings that apply to a compilation are set once per thread by the front end
//! ([`set_mappings`]) before the compiler is created; every compiler created on that thread
//! (including the ones that load class files) resolves dotted class paths through them.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    /// `/models/`: lowercase is not forced, matching is case insensitive.
    pub name: String,
    pub path: PathBuf,
    pub external: bool,
}

impl Mapping {
    /// A mapping with its name normalized (leading and trailing slash).
    pub fn new(name: &str, path: impl Into<PathBuf>, external: bool) -> Self {
        Mapping {
            name: clean_name(name),
            path: path.into(),
            external,
        }
    }
}

/// `models`, `/models` and `/models/` are the same mapping: `/models/`.
pub fn clean_name(name: &str) -> String {
    let trimmed = name.trim().replace('\\', "/");
    let inner = trimmed.trim_matches('/');
    if inner.is_empty() {
        "/".to_string()
    } else {
        format!("/{inner}/")
    }
}

/// Builds a mapping from the value found in `this.mappings` / `boxlang.json`: a string path or a
/// struct with a required `path` and an optional `external` (default `true`).
/// Relative paths are resolved against `base`.
pub fn from_json(name: &str, value: &serde_json::Value, base: &Path) -> Result<Mapping, String> {
    let (path, external) = match value {
        serde_json::Value::String(path) => (path.clone(), true),
        serde_json::Value::Object(object) => {
            let path = object
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| format!("Mapping [{name}]: a struct mapping needs a path"))?
                .to_string();
            let external = object
                .get("external")
                .map(|e| match e {
                    serde_json::Value::Bool(b) => *b,
                    serde_json::Value::String(s) => !s.eq_ignore_ascii_case("false"),
                    _ => true,
                })
                .unwrap_or(true);
            (path, external)
        }
        _ => return Err(format!("Mapping [{name}]: expected a path or a struct")),
    };
    let path = PathBuf::from(path);
    // `normalize` is lexical: an unmatched `..` would be dropped from a relative base, so make it absolute first
    let base = if base.is_absolute() {
        base.to_path_buf()
    } else {
        std::env::current_dir().map(|cwd| cwd.join(base)).unwrap_or_else(|_| base.to_path_buf())
    };
    let absolute = if path.is_absolute() { path } else { base.join(path) };
    Ok(Mapping::new(name, normalize(&absolute), external))
}

/// Lexical `..` / `.` cleanup (no file system access, the directory may not exist yet).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

thread_local! {
    static MAPPINGS: RefCell<Vec<Mapping>> = const { RefCell::new(Vec::new()) };
}

/// Sets the mappings used by compilers created on this thread. Later entries override earlier
/// ones with the same name (application mappings are added after the global ones).
pub fn set_mappings(mappings: Vec<Mapping>) {
    let mut merged: Vec<Mapping> = Vec::new();
    for mapping in mappings {
        merged.retain(|m| !m.name.eq_ignore_ascii_case(&mapping.name));
        merged.push(mapping);
    }
    MAPPINGS.with(|m| *m.borrow_mut() = merged);
}

/// The mappings active on this thread (see [`set_mappings`]).
pub fn current_mappings() -> Vec<Mapping> {
    MAPPINGS.with(|m| m.borrow().clone())
}

/// Candidate base paths (no extension) for a dotted class path such as `models.User`, in mapping
/// order of specificity (longest mapping name first). `/` maps the whole class path.
pub fn candidates(mappings: &[Mapping], dotted_path: &str) -> Vec<PathBuf> {
    let slash = format!("/{}", dotted_path.replace('.', "/"));
    let slash_lower = slash.to_lowercase();
    let mut matching: Vec<&Mapping> = mappings
        .iter()
        .filter(|m| {
            let name = m.name.to_lowercase();
            // "/models/" matches "/models/user" but not "/modelsx/user"
            slash_lower.starts_with(&name) && slash.len() > name.len() - 1
        })
        .collect();
    matching.sort_by_key(|m| std::cmp::Reverse(m.name.len()));
    matching
        .into_iter()
        .map(|m| {
            let rest = &slash[m.name.len()..];
            m.path.join(rest)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_get_slashes() {
        assert_eq!(clean_name("models"), "/models/");
        assert_eq!(clean_name("/models"), "/models/");
        assert_eq!(clean_name("/models/"), "/models/");
        assert_eq!(clean_name("/"), "/");
        assert_eq!(clean_name("/a/b"), "/a/b/");
    }

    #[test]
    fn string_and_struct_values() {
        let base = Path::new("/proj");
        let m = from_json("/app", &serde_json::json!("./app"), base).unwrap();
        assert_eq!((m.name.as_str(), m.path.as_path(), m.external), ("/app/", Path::new("/proj/app"), true));
        let m = from_json("internal", &serde_json::json!({"path": "/abs/in", "external": false}), base).unwrap();
        assert_eq!((m.name.as_str(), m.path.as_path(), m.external), ("/internal/", Path::new("/abs/in"), false));
        assert!(from_json("x", &serde_json::json!({"external": true}), base).is_err());
    }

    #[test]
    fn a_relative_base_keeps_its_parent_steps() {
        let cwd = std::env::current_dir().unwrap();
        let m = from_json("x", &serde_json::json!("../../shared"), Path::new("project")).unwrap();
        assert_eq!(m.path, normalize(&cwd.join("project").join("../../shared")));
    }

    #[test]
    fn candidates_match_by_prefix_and_prefer_the_longest() {
        let maps = vec![
            Mapping::new("/", "/root", true),
            Mapping::new("/models", "/m", true),
            Mapping::new("/models/deep", "/d", true),
        ];
        assert_eq!(candidates(&maps, "models.User")[0], Path::new("/m/User"));
        assert_eq!(candidates(&maps, "models.deep.Thing")[0], Path::new("/d/Thing"));
        assert_eq!(candidates(&maps, "other.Thing")[0], Path::new("/root/other/Thing"));
        // a prefix must end at a path boundary
        assert!(candidates(&[Mapping::new("/models", "/m", true)], "modelsx.User").is_empty());
    }

    #[test]
    fn later_mappings_override_earlier_ones_with_the_same_name() {
        set_mappings(vec![Mapping::new("/a", "/one", true), Mapping::new("a", "/two", true)]);
        assert_eq!(current_mappings(), vec![Mapping::new("/a", "/two", true)]);
        set_mappings(vec![]);
    }
}
