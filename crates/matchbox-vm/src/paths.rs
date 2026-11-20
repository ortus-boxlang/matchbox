//! BoxLang-compatible path expansion, mirroring `FileSystemUtil.expandPath` of the JVM runtime.
//!
//! The rules, in order:
//! 1. A leading `//` loses one slash and every `\` becomes `/`.
//! 2. A relative path (no leading `/`) is taken from the directory of the calling template, when there is one.
//!    Without a template it is taken from the root mapping, which defaults to the working directory.
//! 3. The longest mapping whose name starts the path (case-insensitive) wins, and the rest of the path is
//!    appended to the mapping's folder. Mapping names always end in `/`, so `/mytest` matches `/mytest/` and
//!    `/mytest/x`, but `/mytestfoo` does not.
//! 4. An absolute path is kept as it is when it exists, or when its first segment exists on disk.
//! 5. An `onMissingMapping` handler may supply the answer.
//! 6. Anything else is taken from the root mapping.
//!
//! Nothing here touches the VM. Hosts supply the mappings, the template and the missing-mapping handler.

use crate::types::{BxVM, BxValue};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// A virtual path prefix and the folder it stands for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathMapping {
    /// Always starts and ends with `/`, with `/` as the only separator.
    pub name: String,
    pub path: PathBuf,
    /// An external mapping may be reached from outside the application (see `externalOnly`).
    pub external: bool,
}

impl PathMapping {
    pub fn new(name: &str, path: impl Into<PathBuf>, external: bool) -> Self {
        Self { name: clean_name(name), path: path.into(), external }
    }
}

/// `foo\bar` and `/foo/bar` both become `/foo/bar/`; the root stays `/`.
pub fn clean_name(name: &str) -> String {
    let mut name = name.trim().replace('\\', "/");
    if !name.starts_with('/') {
        name.insert(0, '/');
    }
    if !name.ends_with('/') {
        name.push('/');
    }
    name
}

/// `expandPath( path )` for the code running in `vm`: its calling file, the VM's mappings and its
/// `onMissingMapping` interceptors. A trailing separator on `path` is kept on the result.
pub fn expand_for_vm(vm: &mut dyn BxVM, path: &str) -> Result<String, String> {
    let has_trailing = path.ends_with('/') || path.ends_with('\\');
    let mappings = vm.path_mappings();
    let template = vm.current_chunk().map(|chunk| PathBuf::from(chunk.borrow().filename()));
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let options = ExpandOptions { template: template.as_deref(), ..ExpandOptions::default() };
    let resolved = expand_path(&mappings, &cwd, path, &options, &mut |missing: &str| {
        // BoxLang's onMissingMapping: a handler puts the answer in `resolvedFilePath`.
        let data = vm.struct_new();
        let text = vm.string_new(missing.to_string());
        vm.struct_set(data, "path", BxValue::new_ptr(text));
        vm.struct_set(data, "resolvedFilePath", BxValue::new_null());
        vm.announce("onMissingMapping", BxValue::new_ptr(data)).ok()?;
        let answer = vm.struct_get(data, "resolvedFilePath");
        (!answer.is_null()).then(|| PathBuf::from(vm.to_string(answer)))
    });
    let mut result = resolved.to_string_lossy().into_owned();
    if has_trailing && !result.ends_with(std::path::MAIN_SEPARATOR) {
        result.push(std::path::MAIN_SEPARATOR);
    }
    Ok(result)
}

/// Options for [`expand_path`].
#[derive(Default)]
pub struct ExpandOptions<'a> {
    /// The file whose code is calling. Relative paths start in its folder.
    pub template: Option<&'a Path>,
    /// Only consider mappings flagged external.
    pub external_only: bool,
    /// Treat an absolute path as relative.
    pub force_relative: bool,
}

/// Expands `path` to an absolute path.
///
/// `root` is the folder of the `/` mapping unless `mappings` defines one. `on_missing` is asked for a path when
/// nothing else matched; it returns `None` to fall through to the root mapping.
pub fn expand_path(
    mappings: &[PathMapping],
    root: &Path,
    path: &str,
    options: &ExpandOptions<'_>,
    on_missing: &mut dyn FnMut(&str) -> Option<PathBuf>,
) -> PathBuf {
    // ColdBox appends too many slashes to view paths, so the JVM tolerates one extra.
    let mut path = path.strip_prefix("//").map_or_else(|| path.to_string(), |rest| format!("/{rest}"));
    path = path.replace('\\', "/");
    let original = path.clone();
    let absolute = !options.force_relative && Path::new(&original).is_absolute();

    if !absolute && !path.starts_with('/') {
        if let Some(template) = options.template.filter(|t| is_usable_template(t)) {
            let directory = template.parent().unwrap_or_else(|| Path::new("."));
            return normalize(&absolutize(&directory.join(&path)));
        }
        path.insert(0, '/');
    }

    // Longest mapping first, as the JVM keeps its mappings sorted.
    let mut candidates: Vec<&PathMapping> = mappings.iter().filter(|m| m.name != "/").collect();
    candidates.sort_by_key(|m| std::cmp::Reverse(m.name.len()));
    let lowered = path.to_ascii_lowercase();
    let matched = candidates.into_iter().find(|mapping| {
        if options.external_only && !mapping.external {
            return false;
        }
        let name = mapping.name.to_ascii_lowercase();
        lowered.starts_with(&name) || (name.len() == lowered.len() + 1 && format!("{lowered}/") == name)
    });
    if let Some(mapping) = matched {
        let rest = path.get(mapping.name.len().min(path.len())..).unwrap_or("");
        return normalize(&absolutize(&mapping.path.join(rest.trim_start_matches('/'))));
    }

    // "C:/foo" is absolute as written. "/foo" has to exist, or sit under a folder that does, before it is
    // believed to be a real absolute path rather than one relative to the root mapping.
    if absolute && original != "/" {
        if cfg!(windows) {
            return PathBuf::from(&original);
        }
        let exists = Path::new(&original).exists();
        let parent_is_root = Path::new(&original).parent().is_some_and(|p| p == Path::new("/"));
        if original.starts_with('/') && !exists && !parent_is_root {
            if let Some(first) = original.trim_start_matches('/').split('/').next() {
                if Path::new(&format!("/{first}")).exists() {
                    return PathBuf::from(&original);
                }
            }
        } else if exists {
            return PathBuf::from(&original);
        }
    }

    if let Some(resolved) = on_missing(&path) {
        return resolved;
    }

    let root_folder = mappings.iter().find(|m| m.name == "/").map_or(root, |m| m.path.as_path());
    normalize(&absolutize(&root_folder.join(path.trim_start_matches('/'))))
}

/// A template named `unknown`, or none at all, is ad-hoc source with nowhere to be relative to.
fn is_usable_template(template: &Path) -> bool {
    let text = template.to_string_lossy();
    !text.is_empty() && text != "unknown"
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

/// Resolves `.` and `..` without reading the file system, as Java's `Path.normalize()` does.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<PathBuf> {
        None
    }

    fn expand(mappings: &[PathMapping], path: &str, options: &ExpandOptions<'_>) -> PathBuf {
        expand_path(mappings, Path::new("/work"), path, options, &mut none)
    }

    fn mappings() -> Vec<PathMapping> {
        vec![
            PathMapping::new("/mytest", "/data/io", true),
            PathMapping::new("/mytest/deep", "/data/deep", true),
            PathMapping::new("/private", "/data/private", false),
        ]
    }

    #[test]
    fn mapping_names_are_cleaned() {
        assert_eq!(clean_name("mytest"), "/mytest/");
        assert_eq!(clean_name("\\a\\b"), "/a/b/");
        assert_eq!(clean_name("/"), "/");
    }

    #[test]
    fn a_mapping_prefix_resolves_into_its_folder() {
        let options = ExpandOptions::default();
        assert_eq!(expand(&mappings(), "/mytest/x.txt", &options), PathBuf::from("/data/io/x.txt"));
        assert_eq!(expand(&mappings(), "/MYTEST/x.txt", &options), PathBuf::from("/data/io/x.txt"));
        assert_eq!(expand(&mappings(), "/mytest", &options), PathBuf::from("/data/io"));
        assert_eq!(expand(&mappings(), "/mytest/", &options), PathBuf::from("/data/io"));
    }

    #[test]
    fn the_longest_mapping_wins() {
        assert_eq!(expand(&mappings(), "/mytest/deep/a", &ExpandOptions::default()), PathBuf::from("/data/deep/a"));
    }

    #[test]
    fn a_name_that_only_starts_with_a_mapping_name_is_not_in_the_mapping() {
        assert_eq!(expand(&mappings(), "/mytestfoo/bar", &ExpandOptions::default()), PathBuf::from("/work/mytestfoo/bar"));
    }

    #[test]
    fn external_only_skips_internal_mappings() {
        let options = ExpandOptions { external_only: true, ..Default::default() };
        assert_eq!(expand(&mappings(), "/private/a", &options), PathBuf::from("/work/private/a"));
        assert_eq!(expand(&mappings(), "/mytest/a", &options), PathBuf::from("/data/io/a"));
    }

    #[test]
    fn without_a_template_relative_paths_start_at_the_root() {
        let options = ExpandOptions::default();
        assert_eq!(expand(&[], "a/b.txt", &options), PathBuf::from("/work/a/b.txt"));
        assert_eq!(expand(&[], "/a/b.txt", &options), PathBuf::from("/work/a/b.txt"));
        assert_eq!(expand(&[], "//a/b.txt", &options), PathBuf::from("/work/a/b.txt"));
        assert_eq!(expand(&[], "a\\b.txt", &options), PathBuf::from("/work/a/b.txt"));
    }

    #[test]
    fn a_root_mapping_replaces_the_default_root() {
        let mappings = vec![PathMapping::new("/", "/site", true)];
        assert_eq!(expand(&mappings, "/a.txt", &ExpandOptions::default()), PathBuf::from("/site/a.txt"));
    }

    #[test]
    fn relative_paths_start_in_the_templates_folder() {
        let template = Path::new("/app/views/page.bxs");
        let options = ExpandOptions { template: Some(template), ..Default::default() };
        assert_eq!(expand(&[], "data/x.toml", &options), PathBuf::from("/app/views/data/x.toml"));
        assert_eq!(expand(&[], "../x.toml", &options), PathBuf::from("/app/x.toml"));
        assert_eq!(expand(&[], ".", &options), PathBuf::from("/app/views"));
        // A rooted path ignores the template.
        assert_eq!(expand(&[], "/x.toml", &options), PathBuf::from("/work/x.toml"));
    }

    #[test]
    fn an_unknown_template_counts_as_none() {
        let options = ExpandOptions { template: Some(Path::new("unknown")), ..Default::default() };
        assert_eq!(expand(&[], "a.txt", &options), PathBuf::from("/work/a.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn an_absolute_path_that_exists_is_kept() {
        assert_eq!(expand(&[], "/tmp", &ExpandOptions::default()), PathBuf::from("/tmp"));
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_file_under_an_existing_folder_is_kept() {
        assert_eq!(expand(&[], "/tmp/no/such/file.txt", &ExpandOptions::default()), PathBuf::from("/tmp/no/such/file.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_top_level_name_is_taken_from_the_root() {
        assert_eq!(expand(&[], "/totallyFake", &ExpandOptions::default()), PathBuf::from("/work/totallyFake"));
        assert_eq!(expand(&[], "/totallyFake/a/b", &ExpandOptions::default()), PathBuf::from("/work/totallyFake/a/b"));
    }

    #[cfg(unix)]
    #[test]
    fn force_relative_never_keeps_an_absolute_path() {
        let options = ExpandOptions { force_relative: true, ..Default::default() };
        assert_eq!(expand(&[], "/tmp", &options), PathBuf::from("/work/tmp"));
    }

    #[test]
    fn the_missing_mapping_handler_can_answer() {
        let mut asked = Vec::new();
        let result = expand_path(&[], Path::new("/work"), "/nowhere/x", &ExpandOptions::default(), &mut |p| {
            asked.push(p.to_string());
            Some(PathBuf::from("/handled/x"))
        });
        assert_eq!(result, PathBuf::from("/handled/x"));
        assert_eq!(asked, vec!["/nowhere/x".to_string()]);
    }

    #[test]
    fn the_missing_mapping_handler_is_not_asked_when_a_mapping_matches() {
        let result = expand_path(&mappings(), Path::new("/work"), "/mytest/a", &ExpandOptions::default(), &mut |_| {
            panic!("must not be asked")
        });
        assert_eq!(result, PathBuf::from("/data/io/a"));
    }

    #[test]
    fn normalize_resolves_dots_without_the_file_system() {
        assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize(Path::new("a/../../b")), PathBuf::from("../b"));
    }
}
