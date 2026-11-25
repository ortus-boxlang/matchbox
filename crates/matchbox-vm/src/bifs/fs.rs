//! File system BIFs: directories, files, paths and simple file objects.
//!
//! Each public function here is a native BIF with the [`BxNativeFunction`]
//! signature, registered by name in [`super::register_all`] behind the
//! `bif-io` feature. Arguments arrive in BoxLang parameter order; named
//! arguments are mapped to that order by the VM before the call.
//!
//! [`BxNativeFunction`]: crate::types::BxNativeFunction

#[cfg(feature = "bif-io")]
use super::file_handle::{file_object_id, Charset, FileObject, Mode};
#[cfg(feature = "bif-io")]
use crate::types::{BxVM, BxValue};
#[cfg(feature = "bif-io")]
use chrono::{DateTime, Utc};
#[cfg(feature = "bif-io")]
use std::fs;
#[cfg(all(feature = "bif-io", unix))]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(feature = "bif-io")]
use std::path::{Path, PathBuf};
#[cfg(feature = "bif-io")]
use uuid::Uuid;

#[cfg(all(feature = "bif-io", not(target_arch = "wasm32")))]
use crate::datasource::{
    BxQuery,
    traits::{QueryColumn, QueryColumnType, QueryResult, SqlValue},
};
#[cfg(feature = "bif-io")]
use std::{cell::RefCell, rc::Rc};

#[cfg(feature = "bif-io")]
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
#[cfg(feature = "bif-io")]
use walkdir::WalkDir;

/// Converts a path to text and normalizes it with [`normalize_path_text`].
#[cfg(feature = "bif-io")]
fn normalize_path_string(path: &Path) -> String {
    normalize_path_text(path.to_string_lossy().as_ref())
}

/// Removes the Windows verbatim prefix from a path string.
///
/// On Windows, `\\?\UNC\server\share` becomes `\\server\share` and
/// `\\?\C:\dir` becomes `C:\dir`. On other platforms the text is returned
/// unchanged.
#[cfg(feature = "bif-io")]
fn normalize_path_text(path: &str) -> String {
    #[cfg(windows)]
    {
        if let Some(stripped) = path.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{}", stripped);
        }
        if let Some(stripped) = path.strip_prefix(r"\\?\") {
            return stripped.to_string();
        }
    }

    path.to_string()
}

/// Implements `directoryExists( path )`.
///
/// Returns `true` when `path` exists and is a directory.
///
/// # Errors
/// Returns an error when no path argument is given.
#[cfg(feature = "bif-io")]
pub fn directory_exists(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("directoryExists() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);
    Ok(BxValue::new_bool(path.exists() && path.is_dir()))
}

/// Implements `directoryCreate( path, createPath = true )`.
///
/// With `createPath` true, missing parent directories are created as well, and
/// an existing directory is not an error. With `createPath` false, only the last
/// directory is created. Returns `true`.
///
/// # Errors
/// Returns an error when no path argument is given, or when the directory
/// cannot be created.
#[cfg(feature = "bif-io")]
pub fn directory_create(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("directoryCreate() expects at least 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);

    let recurse = if args.len() > 1 {
        args[1].as_bool()
    } else {
        true
    };

    if recurse {
        fs::create_dir_all(path).map_err(|e| e.to_string())?;
    } else {
        fs::create_dir(path).map_err(|e| e.to_string())?;
    }

    Ok(BxValue::new_bool(true))
}

/// Implements `directoryDelete( path, recurse = false )`.
///
/// With `recurse` true, the directory and everything in it are removed.
/// Otherwise the directory must be empty. Returns `true`.
///
/// # Errors
/// Returns an error when no path argument is given, or when the directory
/// cannot be removed (missing, not empty without `recurse`, or no permission).
#[cfg(feature = "bif-io")]
pub fn directory_delete(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("directoryDelete() expects at least 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);

    let recurse = if args.len() > 1 {
        args[1].as_bool()
    } else {
        false
    };

    if recurse {
        fs::remove_dir_all(path).map_err(|e| e.to_string())?;
    } else {
        fs::remove_dir(path).map_err(|e| e.to_string())?;
    }

    Ok(BxValue::new_bool(true))
}

/// Implements `directoryList( path, recurse, listInfo, filter, sort, type, followLinks )`.
///
/// Lists the entries of `path`, not including `path` itself. A missing `path`
/// gives an empty listing.
///
/// - `recurse` (default `false`) also lists the contents of subdirectories.
/// - `listInfo` (default `"path"`) selects the result: `"path"` gives an array
///   of full paths, `"name"` an array of entry names, and `"query"` a query
///   built by [`directory_entries_to_query`].
/// - `filter` is a glob string or a closure, applied by [`EntryFilter`].
/// - `sort` (default `"name"`) is applied by [`sort_directory_entries`].
/// - `type` (default `"all"`) keeps only `"file"` or `"dir"` entries when given.
///   Both follow symbolic links, so a link to a directory counts as `"dir"`.
/// - `followLinks` (default `false`) makes a recursive listing walk into
///   linked directories. A link given as `path` itself is always resolved, and
///   entries under it are listed under `path`.
///
/// Entries that cannot be read, including link loops, are skipped.
///
/// # Errors
/// Returns an error when no path argument is given, when the filter is not a
/// valid glob, when a filter closure fails, or when `listInfo` is `"query"` on
/// `wasm32`, where queries are not available.
#[cfg(feature = "bif-io")]
pub fn directory_list(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("directoryList() expects at least 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let recurse = optional_bool(args.get(1), false);
    let list_info = args
        .get(2)
        .filter(|value| !value.is_null())
        .map(|value| vm.to_string(*value).to_lowercase())
        .unwrap_or_else(|| "path".to_string());
    let filter = EntryFilter::from_argument(vm, args.get(3))?;
    let sort = args
        .get(4)
        .filter(|value| !value.is_null())
        .map(|value| vm.to_string(*value))
        .unwrap_or_else(|| "name".to_string());
    let item_type = args
        .get(5)
        .filter(|value| !value.is_null())
        .map(|value| vm.to_string(*value).to_lowercase())
        .unwrap_or_else(|| "all".to_string());
    let follow_links = optional_bool(args.get(6), false);

    let mut entries = list_entries(vm, Path::new(&path_str), recurse, follow_links, &filter, |path| {
        match item_type.as_str() {
            "file" => path.is_file(),
            "dir" | "directory" => path.is_dir(),
            _ => true,
        }
    })?;

    sort_directory_entries(&mut entries, &sort);

    if list_info == "query" {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let query = directory_entries_to_query(&entries);
            let id = vm.native_object_new(Rc::new(RefCell::new(BxQuery::from_result(query))));
            return Ok(BxValue::new_ptr(id));
        }
        #[cfg(target_arch = "wasm32")]
        {
            return Err("directoryList(query) is not available in this build".to_string());
        }
    }

    let array_id = vm.array_new();
    for path in entries {
        let value = if list_info == "name" {
            file_name(&path).to_string()
        } else {
            path.to_string_lossy().to_string()
        };
        let s_id = vm.string_new(value);
        vm.array_push(array_id, BxValue::new_ptr(s_id));
    }
    Ok(BxValue::new_ptr(array_id))
}

/// Walks `root` and returns the entries that pass `keep` and then `filter`.
///
/// `root` itself is not included. Without `recurse` only its direct entries
/// are listed. Symbolic links inside `root` are only walked into when
/// `follow_links` is true; a link given as `root` is always resolved. Entries
/// that cannot be read, including link loops, are skipped.
///
/// # Errors
/// Returns an error when a filter closure fails.
#[cfg(feature = "bif-io")]
fn list_entries(
    vm: &mut dyn BxVM,
    root: &Path,
    recurse: bool,
    follow_links: bool,
    filter: &EntryFilter,
    keep: impl Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>, String> {
    let mut walker = WalkDir::new(root).min_depth(1).follow_links(follow_links);
    if !recurse {
        walker = walker.max_depth(1);
    }
    let mut entries = Vec::new();
    for path in walker.into_iter().filter_map(|entry| entry.ok()).map(|entry| entry.into_path()) {
        if keep(&path) && filter.matches(vm, &path)? {
            entries.push(path);
        }
    }
    Ok(entries)
}

/// The `filter` argument of `directoryList()` and `directoryCopy()`.
#[cfg(feature = "bif-io")]
enum EntryFilter {
    /// Keep every entry.
    All,
    /// Keep entries whose name matches any of the globs.
    Globs(GlobSet),
    /// Keep entries for which the closure, called with the full path, returns
    /// a truthy value.
    Closure(BxValue),
}

#[cfg(feature = "bif-io")]
impl EntryFilter {
    /// Builds a filter from a BIF argument, following BoxLang's rules.
    ///
    /// A function becomes [`EntryFilter::Closure`]. A string is split on `|`
    /// into globs in Java syntax (`*`, `?`, `**`, `[abc]`, `[!abc]`, `{a,b}` and
    /// `\` escapes), matched case-sensitively against the entry name. As in
    /// BoxLang, a string of one character or fewer, such as `""` or `"*"`, is
    /// no filter; a blank string is no filter too. A missing or null argument
    /// is no filter.
    ///
    /// # Errors
    /// Returns an error when a glob is not valid.
    fn from_argument(vm: &mut dyn BxVM, value: Option<&BxValue>) -> Result<Self, String> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(Self::All);
        };
        if vm.value_matches_type_name(*value, "function") {
            return Ok(Self::Closure(*value));
        }
        let pattern = vm.to_string(*value);
        if pattern.chars().count() <= 1 || pattern.trim().is_empty() {
            return Ok(Self::All);
        }
        let mut builder = GlobSetBuilder::new();
        for part in pattern.split('|').filter(|part| !part.is_empty()) {
            let glob = GlobBuilder::new(part)
                .literal_separator(true)
                .backslash_escape(true)
                .build()
                .map_err(|error| format!("The filter [{pattern}] is not a valid glob: {error}"))?;
            builder.add(glob);
        }
        builder
            .build()
            .map(Self::Globs)
            .map_err(|error| format!("The filter [{pattern}] is not a valid glob: {error}"))
    }

    /// Reports whether `path` passes the filter.
    ///
    /// # Errors
    /// Returns an error when a closure filter fails or no code context is
    /// available to call it.
    fn matches(&self, vm: &mut dyn BxVM, path: &Path) -> Result<bool, String> {
        match self {
            Self::All => Ok(true),
            Self::Globs(globs) => Ok(globs.is_match(file_name(path))),
            Self::Closure(closure) => {
                let chunk = vm
                    .current_chunk()
                    .ok_or_else(|| "No code context is available to call the filter".to_string())?;
                let argument = vm.string_new(path.to_string_lossy().into_owned());
                let result = vm.call_function_by_value(closure, vec![BxValue::new_ptr(argument)], chunk)?;
                Ok(vm.is_truthy(result))
            }
        }
    }
}

/// Reads an optional boolean argument, using `default` when it is missing or null.
///
/// A caller can pass null explicitly, and a named call skipping a parameter
/// with no declared default passes null, so null means "use the default"
/// rather than `false`.
#[cfg(feature = "bif-io")]
fn optional_bool(value: Option<&BxValue>, default: bool) -> bool {
    value
        .filter(|value| !value.is_null())
        .map(|value| value.as_bool())
        .unwrap_or(default)
}

/// Returns the last component of `path`.
///
/// Returns an empty string when the path has no final component or the name
/// is not valid UTF-8.
#[cfg(feature = "bif-io")]
fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

/// Sorts directory entries in place by a BoxLang sort directive.
///
/// `sort` is a column name optionally followed by `asc` or `desc`, such as
/// `"size desc"`. The columns are `size`, `date` (or `datelastmodified`),
/// `type` (files before directories) and `name`. An unknown column sorts by
/// name. Names compare case-sensitively.
#[cfg(feature = "bif-io")]
fn sort_directory_entries(entries: &mut [PathBuf], sort: &str) {
    let mut directives = sort.split_whitespace();
    let column = directives.next().unwrap_or("name").to_lowercase();
    let descending = directives
        .next()
        .map(|direction| direction.eq_ignore_ascii_case("desc"))
        .unwrap_or(false);

    entries.sort_by(|left, right| {
        let ordering = match column.as_str() {
            "size" => file_size(left).cmp(&file_size(right)),
            "date" | "datelastmodified" => file_modified(left).cmp(&file_modified(right)),
            "type" => is_directory(left).cmp(&is_directory(right)),
            _ => file_name(left).cmp(file_name(right)),
        };
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

/// Returns the size of `path` in bytes, or `0` when its metadata cannot be read.
#[cfg(feature = "bif-io")]
fn file_size(path: &Path) -> u64 {
    path.metadata().map(|metadata| metadata.len()).unwrap_or(0)
}

/// Returns the last modified time of `path`.
///
/// Returns the Unix epoch when the time cannot be read.
#[cfg(feature = "bif-io")]
fn file_modified(path: &Path) -> std::time::SystemTime {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .unwrap_or(std::time::UNIX_EPOCH)
}

/// Reports whether `path` is a directory, following symbolic links.
#[cfg(feature = "bif-io")]
fn is_directory(path: &Path) -> bool {
    path.is_dir()
}

/// Builds the `directoryList( listInfo = "query" )` result for `entries`.
///
/// The columns are `name`, `size`, `type` (`"Dir"` or `"File"`),
/// `dateLastModified` (RFC 3339 text), `attributes` (see [`file_attributes`]),
/// `mode` (always empty) and `directory` (the parent directory).
#[cfg(all(feature = "bif-io", not(target_arch = "wasm32")))]
fn directory_entries_to_query(entries: &[PathBuf]) -> QueryResult {
    let columns = vec![
        QueryColumn {
            name: "name".to_string(),
            col_type: QueryColumnType::Varchar,
        },
        QueryColumn {
            name: "size".to_string(),
            col_type: QueryColumnType::BigInt,
        },
        QueryColumn {
            name: "type".to_string(),
            col_type: QueryColumnType::Varchar,
        },
        QueryColumn {
            name: "dateLastModified".to_string(),
            col_type: QueryColumnType::Timestamp,
        },
        QueryColumn {
            name: "attributes".to_string(),
            col_type: QueryColumnType::Varchar,
        },
        QueryColumn {
            name: "mode".to_string(),
            col_type: QueryColumnType::Varchar,
        },
        QueryColumn {
            name: "directory".to_string(),
            col_type: QueryColumnType::Varchar,
        },
    ];
    let rows = entries
        .iter()
        .map(|path| {
            let metadata = path.metadata().ok();
            let size = metadata
                .as_ref()
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            let modified = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .map(DateTime::<Utc>::from)
                .map(|date| date.to_rfc3339())
                .unwrap_or_default();
            let directory = path
                .parent()
                .map(|parent| parent.to_string_lossy().to_string())
                .unwrap_or_default();
            vec![
                SqlValue::Text(file_name(path).to_string()),
                SqlValue::Int(size as i64),
                SqlValue::Text(if path.is_dir() { "Dir" } else { "File" }.to_string()),
                SqlValue::Text(modified),
                SqlValue::Text(file_attributes(path)),
                SqlValue::Text(String::new()),
                SqlValue::Text(directory),
            ]
        })
        .collect();
    QueryResult { columns, rows }
}

/// Returns the attribute letters for `path`.
///
/// The result has `R` when the metadata can be read and `W` when the entry is
/// also not read-only. It is empty when the metadata cannot be read.
#[cfg(feature = "bif-io")]
fn file_attributes(path: &Path) -> String {
    let mut attributes = String::new();
    if fs::metadata(path).is_ok() {
        attributes.push('R');
        if !fs::metadata(path)
            .map(|metadata| metadata.permissions().readonly())
            .unwrap_or(true)
        {
            attributes.push('W');
        }
    }
    attributes
}

/// Implements `fileExists( path )`.
///
/// Returns `true` when `path` names an existing entry that is not a directory,
/// following symbolic links, as BoxLang does. A dangling link returns `false`.
///
/// # Errors
/// Returns an error when no path argument is given.
#[cfg(feature = "bif-io")]
pub fn file_exists(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fileExists() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let exists = fs::metadata(&path_str)
        .map(|metadata| !metadata.is_dir())
        .unwrap_or(false);
    Ok(BxValue::new_bool(exists))
}

/// Implements `fileDelete( path )`.
///
/// Removes the file at `path` and returns `true`.
///
/// # Errors
/// Returns an error when no path argument is given, or when the file cannot be
/// removed.
#[cfg(feature = "bif-io")]
pub fn file_delete(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fileDelete() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    fs::remove_file(path_str).map_err(|e| e.to_string())?;
    Ok(BxValue::new_bool(true))
}

/// Implements `fileMove( source, destination )`.
///
/// Renames `source` to `destination` and returns `true`. The move is atomic
/// and only works within one file system. On Unix an existing destination file
/// is replaced.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, or when the
/// rename fails, for example across file systems.
#[cfg(feature = "bif-io")]
pub fn file_move(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileMove() expects 2 arguments: (source, destination)".to_string());
    }
    let src = vm.to_string(args[0]);
    let dest = vm.to_string(args[1]);

    // ponytail: atomic same-filesystem moves; cross-device moves need a staged-copy implementation.
    // Never remove the destination first: a failed move must leave its contents intact.
    fs::rename(&src, &dest).map_err(|e| e.to_string())?;

    Ok(BxValue::new_bool(true))
}

/// Implements `filePublishExclusive( source, destination )`.
///
/// Moves a finished file into place only when `destination` does not exist
/// yet. It hard-links `source` to `destination`, so readers never see a
/// partial file, and then removes `source`. Returns `true`.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, when
/// `destination` already exists, when the paths are on different file
/// systems, or when `source` cannot be removed. In that last case the new
/// `destination` link is removed again.
#[cfg(feature = "bif-io")]
pub fn file_publish_exclusive(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err(
            "filePublishExclusive() expects 2 arguments: (source, destination)".to_string(),
        );
    }
    let src = vm.to_string(args[0]);
    let dest = vm.to_string(args[1]);

    // hard_link publishes the complete same-filesystem file atomically and fails if dest exists.
    fs::hard_link(&src, &dest).map_err(|e| e.to_string())?;
    if let Err(error) = fs::remove_file(&src) {
        let _ = fs::remove_file(&dest);
        return Err(error.to_string());
    }

    Ok(BxValue::new_bool(true))
}

/// Implements `fileCopy( source, destination )`.
///
/// Copies the file, replacing `destination` if it exists, and returns `true`.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, or when the copy
/// fails.
#[cfg(feature = "bif-io")]
pub fn file_copy(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileCopy() expects 2 arguments: (source, destination)".to_string());
    }
    let src = vm.to_string(args[0]);
    let dest = vm.to_string(args[1]);

    fs::copy(src, dest).map_err(|e| e.to_string())?;

    Ok(BxValue::new_bool(true))
}

/// Implements `fileInfo( path )` and its alias `getFileInfo( path )`.
///
/// Returns a struct with `size`, `type` (`"Dir"` or `"File"`), `is_directory`,
/// `is_file`, `is_symlink` and `is_readonly`. Symbolic links are not followed:
/// for a link the struct describes the link itself and adds `target`.
///
/// # Errors
/// Returns an error when no path argument is given, or when the metadata
/// cannot be read.
#[cfg(feature = "bif-io")]
pub fn file_info(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fileInfo() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;

    let struct_id = vm.struct_new();

    vm.struct_set(
        struct_id,
        "size",
        BxValue::new_number(metadata.len() as f64),
    );
    let type_id = vm.string_new(if metadata.is_dir() { "Dir" } else { "File" }.to_string());
    vm.struct_set(struct_id, "type", BxValue::new_ptr(type_id));
    vm.struct_set(
        struct_id,
        "is_directory",
        BxValue::new_bool(metadata.is_dir()),
    );
    vm.struct_set(struct_id, "is_file", BxValue::new_bool(metadata.is_file()));
    vm.struct_set(
        struct_id,
        "is_symlink",
        BxValue::new_bool(metadata.file_type().is_symlink()),
    );
    vm.struct_set(
        struct_id,
        "is_readonly",
        BxValue::new_bool(metadata.permissions().readonly()),
    );

    if metadata.file_type().is_symlink() {
        if let Ok(target) = fs::read_link(path) {
            let target_str = target.to_string_lossy().to_string();
            let s_id = vm.string_new(target_str);
            vm.struct_set(struct_id, "target", BxValue::new_ptr(s_id));
        }
    }

    Ok(BxValue::new_ptr(struct_id))
}

/// Implements `fileCreateSymlink( link, target )`.
///
/// Creates a symbolic link at `link` pointing to `target` and returns `true`.
/// On Windows a directory link is created when `target` is a directory.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, when the link
/// cannot be created, or on platforms without symbolic links.
#[cfg(feature = "bif-io")]
pub fn file_create_symlink(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileCreateSymlink() expects 2 arguments: (link, target)".to_string());
    }
    let link = vm.to_string(args[0]);
    let target = vm.to_string(args[1]);
    create_symlink(Path::new(&target), Path::new(&link)).map_err(|e| e.to_string())?;

    Ok(BxValue::new_bool(true))
}

/// Implements `fileSetExecutable( path )`.
///
/// On Unix, adds the execute bit for owner, group and others. On other
/// platforms it does nothing. Returns `true`.
///
/// # Errors
/// Returns an error when no path argument is given, or on Unix when the
/// permissions cannot be read or changed.
#[cfg(feature = "bif-io")]
pub fn file_set_executable(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fileSetExecutable() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path_str)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(perms.mode() | 0o111);
        fs::set_permissions(path_str, perms).map_err(|e| e.to_string())?;
    }

    #[cfg(not(unix))]
    {
        // No-op on Windows for executable bit
        let _ = path_str;
    }

    Ok(BxValue::new_bool(true))
}

/// Implements `fileRead( file, charsetOrBufferSize, bufferSize )`.
///
/// For a file object from [`file_open`], reads from the current position: up
/// to `bufferSize` characters (or bytes in `readBinary` mode) when a buffer
/// size is given, otherwise everything to the end of the file. As in BoxLang,
/// a numeric second argument is the buffer size.
///
/// For a path, returns the whole file as a string; the charset and buffer
/// size are ignored.
///
/// # Errors
/// Returns an error when no argument is given, when a buffer size is negative,
/// when a file object is not open in a read mode, when the file cannot be
/// read, or when a path's content is not valid UTF-8.
#[cfg(feature = "bif-io")]
pub fn file_read(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let file = *args
        .first()
        .ok_or_else(|| "fileRead() expects at least 1 argument".to_string())?;
    if let Some(id) = file_object_id(vm, file) {
        let buffer_size = match (args.get(1), args.get(2)) {
            (_, Some(size)) if !size.is_null() => Some(*size),
            (Some(size), _) if size.is_int() || size.is_number() => Some(*size),
            (Some(size), _) if !size.is_null() && vm.to_string(*size).trim().parse::<u64>().is_ok() => Some(*size),
            _ => None,
        };
        return match buffer_size {
            Some(size) => vm.native_object_call_method(id, "read", &[size]),
            None => vm.native_object_call_method(id, "readall", &[]),
        };
    }
    let path_str = vm.to_string(file);
    let content = fs::read_to_string(path_str).map_err(|e| e.to_string())?;
    let s_id = vm.string_new(content);
    Ok(BxValue::new_ptr(s_id))
}

/// Reads a local path as binary without UTF-8 conversion.
///
/// This path-only implementation does not support URLs or open file objects.
///
/// # Errors
/// Returns an error when the path is absent, unreadable or not a regular file.
#[cfg(feature = "bif-io")]
pub fn file_read_binary(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let path = args.first().ok_or("fileReadBinary() expects a file path")?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    let file = options.open(vm.to_string(*path)).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("fileReadBinary() requires a regular file".into());
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::BufReader::new(file).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(BxValue::new_ptr(vm.bytes_new(bytes)))
}

/// Implements `fileWrite( path, content )`.
///
/// Creates or replaces the file. Binary content is written as it is; any other
/// value is written as its string form. Returns `true`.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, or when the file
/// cannot be written.
#[cfg(feature = "bif-io")]
pub fn file_write(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileWrite() expects 2 arguments: (path, content)".to_string());
    }
    let path_str = vm.to_string(args[0]);
    // binary content is written as it is (as BoxLang does); anything else as text
    if vm.is_bytes(args[1]) {
        let bytes = vm.to_bytes(args[1])?;
        fs::write(path_str, bytes).map_err(|e| e.to_string())?;
    } else {
        let content = vm.to_string(args[1]);
        fs::write(path_str, content).map_err(|e| e.to_string())?;
    }
    Ok(BxValue::new_bool(true))
}

/// Implements `fileWriteExclusive( path, content, mode = "0600" )`.
///
/// Creates a new file and writes `content` to it, failing if the file already
/// exists. On Unix the file gets the octal permission `mode`; elsewhere `mode`
/// is ignored. Returns `true`.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, when `mode` is
/// not an octal number (Unix only), when the file exists or cannot be created,
/// or when the write fails.
#[cfg(feature = "bif-io")]
pub fn file_write_exclusive(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err(
            "fileWriteExclusive() expects at least 2 arguments: (path, content, mode)".to_string(),
        );
    }
    let path = vm.to_string(args[0]);
    let content = vm.to_string(args[1]);
    let mode = args
        .get(2)
        .map(|value| vm.to_string(*value))
        .unwrap_or_else(|| "0600".to_string());
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mode = u32::from_str_radix(&mode, 8)
            .map_err(|_| format!("Invalid POSIX mode string: {mode}"))?;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options
        .open(&path)
        .map_err(|error| format!("Could not exclusively create file '{path}': {error}"))?;
    use std::io::Write;
    file.write_all(content.as_bytes())
        .map_err(|error| format!("Could not write file '{path}': {error}"))?;
    Ok(BxValue::new_bool(true))
}

/// Implements `fileAppend( path, content )`.
///
/// Appends the string form of `content` to the file, creating it when missing.
/// No newline is added. Returns `true`.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, or when the file
/// cannot be opened or written.
#[cfg(feature = "bif-io")]
pub fn file_append(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileAppend() expects 2 arguments: (path, content)".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let content = vm.to_string(args[1]);
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path_str)
        .map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(BxValue::new_bool(true))
}

/// Implements `contractPath( path )`.
///
/// Makes `path` relative to the current working directory. A path inside it
/// becomes `/relative/path`, and the working directory itself becomes `.`. A
/// path outside it is returned as given. Backslashes become forward slashes.
///
/// # Errors
/// Returns an error when no path argument is given, or when the current
/// working directory cannot be read.
#[cfg(feature = "bif-io")]
pub fn contract_path(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("contractPath() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path_text = normalize_path_text(&path_str);
    let path = Path::new(&path_text);
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let cwd_text = normalize_path_string(&cwd);
    let cwd = Path::new(&cwd_text);
    let result = match path.strip_prefix(cwd) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_string(),
        Ok(relative) => format!("/{}", relative.to_string_lossy().replace('\\', "/")),
        Err(_) => path_text.replace('\\', "/"),
    };
    let s_id = vm.string_new(result);
    Ok(BxValue::new_ptr(s_id))
}

/// Appends the platform path separator to `directory` unless it already ends
/// with `/` or `\\`.
#[cfg(feature = "bif-io")]
fn with_trailing_separator(mut directory: String) -> String {
    if !directory.ends_with('/') && !directory.ends_with('\\') {
        directory.push(std::path::MAIN_SEPARATOR);
    }
    directory
}

/// Implements `getTempDirectory()`.
///
/// Returns the system temporary directory, ending with a path separator.
///
/// # Errors
/// Returns an error when any argument is given.
#[cfg(feature = "bif-io")]
pub fn get_temp_directory(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if !args.is_empty() {
        return Err("getTempDirectory() expects no arguments".to_string());
    }

    let directory = with_trailing_separator(std::env::temp_dir().to_string_lossy().to_string());
    Ok(BxValue::new_ptr(vm.string_new(directory)))
}

/// Implements `createTempDirectory( directory, prefix )`.
///
/// Creates a new directory named `<prefix>_<nanoseconds>` inside `directory`
/// and returns its canonical path. `directory` defaults to the system
/// temporary directory and is created when missing. `prefix` defaults to
/// `tmp`.
///
/// # Errors
/// Returns an error when `directory` or the new directory cannot be created.
#[cfg(feature = "bif-io")]
pub fn create_temp_directory(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let directory = if !args.is_empty() {
        vm.to_string(args[0])
    } else {
        std::env::temp_dir().to_string_lossy().to_string()
    };
    let prefix = if args.len() > 1 {
        vm.to_string(args[1])
    } else {
        String::new()
    };

    let base = Path::new(&directory);
    if !base.exists() {
        fs::create_dir_all(base).map_err(|e| e.to_string())?;
    }

    let mut name = if prefix.is_empty() {
        "tmp".to_string()
    } else {
        prefix
    };
    let random_suffix: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    name.push_str(&format!("_{}", random_suffix));

    let dir_path = base.join(&name);
    fs::create_dir_all(&dir_path).map_err(|e| e.to_string())?;
    let canonical = normalize_path_string(&dir_path.canonicalize().unwrap_or(dir_path));
    let s_id = vm.string_new(canonical);
    Ok(BxValue::new_ptr(s_id))
}

/// Implements `createTempFile( directory, prefix, extension )`.
///
/// Creates a new empty file named `<prefix>_<uuid><extension>` inside
/// `directory` and returns its canonical path. `directory` defaults to the
/// system temporary directory and is created when missing. `prefix` defaults
/// to `tmp` and `extension` to `.tmp`. On Unix the file is readable and
/// writable by its owner only.
///
/// # Errors
/// Returns an error when `prefix` or `extension` contains a path separator,
/// when `directory` cannot be created, when the file cannot be created, or
/// when 16 attempts all hit existing names.
#[cfg(feature = "bif-io")]
pub fn create_temp_file(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let directory = if !args.is_empty() {
        vm.to_string(args[0])
    } else {
        std::env::temp_dir().to_string_lossy().to_string()
    };
    let prefix = if args.len() > 1 {
        vm.to_string(args[1])
    } else {
        String::new()
    };
    let suffix = if args.len() > 2 {
        vm.to_string(args[2])
    } else {
        ".tmp".to_string()
    };
    if prefix.contains('/')
        || prefix.contains('\\')
        || suffix.contains('/')
        || suffix.contains('\\')
    {
        return Err(
            "createTempFile() prefix and suffix must not contain path separators".to_string(),
        );
    }

    let base = Path::new(&directory);
    if !base.exists() {
        fs::create_dir_all(base).map_err(|e| e.to_string())?;
    }

    let stem = if prefix.is_empty() {
        "tmp".to_string()
    } else {
        prefix
    };
    let mut file_path = None;
    for _ in 0..16 {
        let candidate = base.join(format!("{}_{}{}", stem, Uuid::new_v4().simple(), suffix));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&candidate) {
            Ok(_) => {
                file_path = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    let file_path =
        file_path.ok_or_else(|| "Could not create a unique temporary file".to_string())?;
    let canonical = file_path
        .canonicalize()
        .unwrap_or(file_path)
        .to_string_lossy()
        .to_string();
    let s_id = vm.string_new(canonical);
    Ok(BxValue::new_ptr(s_id))
}

/// Implements `directoryCopy( source, destination, recurse, filter, createPath, overwrite, followLinks )`.
///
/// Copies `source` into `destination` and returns null, as BoxLang does.
///
/// - `recurse` (default `false`) copies the whole tree. Without it, only the
///   files directly in `source` are copied, and symbolic links among them.
/// - `filter` is a glob string or a closure, applied by [`EntryFilter`] to
///   every entry. Parent directories of a copied file are always created, so a
///   directory that fails the filter is still created when it holds a file
///   that passes.
/// - `createPath` (default `true`) creates `destination` when missing.
/// - `overwrite` (default `false`) replaces files that already exist in
///   `destination`. Without it, an existing file is an error.
/// - `followLinks` (default `false`) copies what symbolic links point to and
///   walks into linked directories. Without it, a link is copied as a link to
///   the same target.
///
/// When `source` is a file rather than a directory, it is copied to
/// `destination` as a file.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, when `source`
/// does not exist, when the filter is not a valid glob or its closure fails,
/// when a file exists in `destination` and `overwrite` is false, or when a
/// directory, file or link cannot be created or copied.
#[cfg(feature = "bif-io")]
pub fn directory_copy(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err(
            "directoryCopy() expects at least 2 arguments: (source, destination)".to_string(),
        );
    }
    let source = vm.to_string(args[0]);
    let dest = vm.to_string(args[1]);
    let recurse = optional_bool(args.get(2), false);
    let filter = EntryFilter::from_argument(vm, args.get(3))?;
    let create_path = optional_bool(args.get(4), true);
    let overwrite = optional_bool(args.get(5), false);
    let follow_links = optional_bool(args.get(6), false);

    let src_path = Path::new(&source);
    let dest_path = Path::new(&dest);
    if create_path && !dest_path.exists() {
        fs::create_dir_all(dest_path).map_err(|e| e.to_string())?;
    }

    if !src_path.is_dir() {
        if !src_path.exists() {
            return Err(format!("The source [{}] does not exist", source));
        }
        copy_entry(src_path, dest_path, overwrite, true)?;
        return Ok(BxValue::new_null());
    }

    let mut entries = list_entries(vm, src_path, recurse, follow_links, &filter, |path| {
        recurse || path.is_file() || (!follow_links && path.is_symlink())
    })?;
    entries.sort();
    for entry in entries {
        let relative = entry.strip_prefix(src_path).map_err(|e| e.to_string())?;
        let target = dest_path.join(relative);
        if recurse {
            if let Some(parent) = target.parent().filter(|parent| !parent.exists()) {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let target_is_dir = fs::symlink_metadata(&target)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false);
        if !target_is_dir {
            copy_entry(&entry, &target, overwrite, follow_links)?;
        }
    }

    Ok(BxValue::new_null())
}

/// Copies one entry to `target`, as Java's `Files.copy` does.
///
/// A symbolic link is copied as a link to the same target unless
/// `follow_links` is true. A directory becomes a new empty directory; its
/// contents are copied as their own entries. An existing `target` is replaced
/// when `overwrite` is true.
///
/// # Errors
/// Returns an error when `target` exists and `overwrite` is false, or when the
/// entry cannot be read, removed or created.
#[cfg(feature = "bif-io")]
fn copy_entry(source: &Path, target: &Path, overwrite: bool, follow_links: bool) -> Result<(), String> {
    if fs::symlink_metadata(target).is_ok() {
        if !overwrite {
            return Err(format!("The file [{}] already exists", target.display()));
        }
        fs::remove_file(target).map_err(|e| e.to_string())?;
    }
    if !follow_links && source.is_symlink() {
        let link_target = fs::read_link(source).map_err(|e| e.to_string())?;
        create_symlink(&link_target, target).map_err(|e| e.to_string())
    } else if source.is_dir() {
        fs::create_dir(target).map_err(|e| e.to_string())
    } else {
        fs::copy(source, target).map(|_| ()).map_err(|e| e.to_string())
    }
}

/// Creates a symbolic link at `link` that points to `target`.
///
/// On Windows a directory link is created when `target` is a directory.
///
/// # Errors
/// Returns an error when the link cannot be created, and always on platforms
/// without symbolic links.
#[cfg(feature = "bif-io")]
fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        let resolved = link.parent().map(|parent| parent.join(target)).unwrap_or_else(|| target.to_path_buf());
        if resolved.is_dir() {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Symlinks not supported on this platform",
        ))
    }
}

/// Implements `directoryMove( source, destination, createPath = true )`.
///
/// Renames `source` to `destination` and returns null. With `createPath` true,
/// the parent directories of `destination` are created first. The move only
/// works within one file system.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, when the parent
/// directories cannot be created, or when the rename fails.
#[cfg(feature = "bif-io")]
pub fn directory_move(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("directoryMove() expects 2 arguments: (source, destination)".to_string());
    }
    let source = vm.to_string(args[0]);
    let dest = vm.to_string(args[1]);
    let create_path = if args.len() > 2 {
        args[2].as_bool()
    } else {
        true
    };

    let dest_path = Path::new(&dest);
    if create_path {
        if let Some(parent) = dest_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
    }

    fs::rename(&source, &dest).map_err(|e| e.to_string())?;
    Ok(BxValue::new_null())
}

/// Implements `expandPath( path )`.
///
/// Resolves `path` to an absolute path with [`crate::paths::expand_for_vm`],
/// relative to the calling file and using the VM's mappings and
/// `onMissingMapping` interceptors. A trailing separator is kept.
///
/// # Errors
/// Returns an error when no path argument is given, or when the path cannot
/// be expanded.
#[cfg(feature = "bif-io")]
pub fn expand_path(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("expandPath() expects 1 argument".to_string());
    }
    let path = vm.to_string(args[0]);
    let result = crate::paths::expand_for_vm(vm, &path)?;
    let s_id = vm.string_new(result);
    Ok(BxValue::new_ptr(s_id))
}

/// Implements `fileClose( file )`.
///
/// Releases the handle of a file object from [`file_open`]. A path has no open
/// handle, so it is accepted and nothing happens, as in BoxLang. Returns null.
///
/// # Errors
/// Returns an error when no argument is given.
#[cfg(feature = "bif-io")]
pub fn file_close(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let file = *args
        .first()
        .ok_or_else(|| "fileClose() expects 1 argument".to_string())?;
    if let Some(id) = file_object_id(vm, file) {
        vm.native_object_call_method(id, "close", &[])?;
    }
    Ok(BxValue::new_null())
}

/// Implements `fileGetMimeType( path, strict = true )`.
///
/// Returns the MIME type for the file extension, from a fixed table, or
/// `application/octet-stream` for an unknown extension. The file content is
/// not inspected. With `strict` true the file must exist and not be empty.
///
/// # Errors
/// Returns an error when no path argument is given, or when `strict` is true
/// and the file is missing, empty or unreadable.
#[cfg(feature = "bif-io")]
pub fn file_get_mime_type(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fileGetMimeType() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let strict = if args.len() > 1 {
        args[1].as_bool()
    } else {
        true
    };

    let path = Path::new(&path_str);
    if strict {
        if !path.exists() {
            return Err(format!("The file [{}] does not exist", path_str));
        }
        let meta = fs::metadata(path).map_err(|e| e.to_string())?;
        if meta.len() == 0 {
            return Err(format!("The file [{}] is empty", path_str));
        }
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let mime = match ext.as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "gzip" => "application/gzip",
        "tar" => "application/x-tar",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "bxs" | "bx" => "text/plain",
        "rs" => "text/plain",
        "java" => "text/plain",
        "md" => "text/markdown",
        "yaml" | "yml" => "application/x-yaml",
        "toml" => "application/toml",
        _ => "application/octet-stream",
    };

    let s_id = vm.string_new(mime.to_string());
    Ok(BxValue::new_ptr(s_id))
}

/// Implements `fileIsEOF( file )`.
///
/// Reports whether the position of a file object from [`file_open`] is at the
/// end of the file.
///
/// # Errors
/// Returns an error when the argument is missing or is not a file object, when
/// the object is open in a write mode or closed, or when the file cannot be read.
#[cfg(feature = "bif-io")]
pub fn file_is_eof(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let id = open_file_argument(vm, args, "fileIsEOF")?;
    vm.native_object_call_method(id, "iseof", &[])
}

/// Implements `fileOpen( file, mode = "read", charset = "utf-8", seekable )`.
///
/// Opens the file and returns a file object holding an open handle (see
/// [`FileObject`]). The modes are `read`, `readBinary`, `write` and `append`.
/// The write modes create the file when it is missing and never empty it. Text
/// reads decode with `charset` and text writes encode with it; the charsets
/// are `utf-8`, `us-ascii`, `iso-8859-1` and `windows-1252`, with the short
/// names `utf8`, `ascii` and `latin1`. `seekable` defaults to `true` for the
/// read modes and `false` for the write modes.
///
/// # Errors
/// Returns an error when no path is given, when `mode` or `charset` is not
/// supported, or when the file cannot be opened in that mode.
#[cfg(feature = "bif-io")]
pub fn file_open(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let path = args
        .first()
        .map(|value| vm.to_string(*value))
        .ok_or_else(|| "fileOpen() expects at least 1 argument".to_string())?;
    let mode = match args.get(1).filter(|value| !value.is_null()) {
        Some(value) => Mode::from_name(&vm.to_string(*value))?,
        None => Mode::Read,
    };
    let charset = match args.get(2).filter(|value| !value.is_null()) {
        Some(value) => {
            let name = vm.to_string(*value);
            Charset::from_name(&name).ok_or_else(|| format!("The charset [{name}] is invalid"))?
        }
        None => Charset::Utf8,
    };
    let seekable = args
        .get(3)
        .filter(|value| !value.is_null())
        .map(|value| value.as_bool());
    let file = FileObject::open(&path, mode, charset, seekable)?;
    let id = vm.native_object_new(Rc::new(RefCell::new(file)));
    Ok(BxValue::new_ptr(id))
}

/// Implements `fileReadLine( file )`.
///
/// For a file object from [`file_open`], returns the next line and moves the
/// position past it. For a path, opens the file, returns its first line and
/// closes it again. A line ends at `\n`, `\r` or `\r\n`, which is not part of
/// the result. Returns null at the end of the file.
///
/// # Errors
/// Returns an error when no argument is given, when a file object is not open
/// in `read` mode, or when the file cannot be read.
#[cfg(feature = "bif-io")]
pub fn file_read_line(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let file = *args
        .first()
        .ok_or_else(|| "fileReadLine() expects 1 argument".to_string())?;
    if let Some(id) = file_object_id(vm, file) {
        return vm.native_object_call_method(id, "readline", &[]);
    }
    let path = vm.to_string(file);
    let line = FileObject::open(&path, Mode::Read, Charset::Utf8, None)?.read_line()?;
    Ok(match line {
        Some(line) => BxValue::new_ptr(vm.string_new(line)),
        None => BxValue::new_null(),
    })
}

/// Implements `fileSeek( file, position )` and its alias `fileSkipBytes`.
///
/// Moves the position of a file object from [`file_open`]. In `read` mode it
/// skips `position` characters forward from the current position; in the other
/// modes it moves to byte `position` from the start. Returns null.
///
/// # Errors
/// Returns an error when the file argument is missing or is not a file
/// object, when `position` is missing or negative, when the object was not
/// opened as seekable, or when the file cannot be read or positioned.
#[cfg(feature = "bif-io")]
pub fn file_seek(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let id = open_file_argument(vm, args, "fileSeek")?;
    let position = *args
        .get(1)
        .ok_or_else(|| "fileSeek() expects 2 arguments: (file, position)".to_string())?;
    vm.native_object_call_method(id, "seek", &[position])?;
    Ok(BxValue::new_null())
}

/// Returns the heap id of the file object a handle-only BIF was given.
///
/// # Errors
/// Returns an error when no argument is given or the first one is not a file
/// object from [`file_open`].
#[cfg(feature = "bif-io")]
fn open_file_argument(vm: &mut dyn BxVM, args: &[BxValue], bif: &str) -> Result<usize, String> {
    args.first()
        .and_then(|file| file_object_id(vm, *file))
        .ok_or_else(|| {
            format!("{bif}() requires an existing file object to be passed that was created with fileOpen().")
        })
}

/// Implements `fileSetAccessMode( path, mode )`.
///
/// Sets the Unix permissions of `path` from a three-digit octal string such
/// as `"644"`. On other platforms the mode is validated and then ignored.
/// Returns `true`.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, when `mode` is
/// not three octal digits, or on Unix when the permissions cannot be changed.
#[cfg(feature = "bif-io")]
pub fn file_set_access_mode(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileSetAccessMode() expects 2 arguments: (path, mode)".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let mode_str = vm.to_string(args[1]);
    if mode_str.len() != 3 || !mode_str.bytes().all(|digit| matches!(digit, b'0'..=b'7')) {
        return Err(format!("Invalid POSIX mode string: {}", mode_str));
    }
    let mode = u32::from_str_radix(&mode_str, 8).expect("validated octal mode");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path_str)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(mode);
        fs::set_permissions(&path_str, perms).map_err(|e| e.to_string())?;
    }

    #[cfg(not(unix))]
    {
        let _ = (path_str, mode);
    }

    Ok(BxValue::new_bool(true))
}

/// Implements `fileSetAttribute( path, attribute )`.
///
/// `readonly` makes the file read-only. `normal` (or `default`) sets mode
/// `664` on Unix and does nothing elsewhere. `archive`, `hidden` and `system`
/// are accepted and ignored. Returns `true`.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, when the file
/// does not exist, when the attribute is not one of the above, or when the
/// permissions cannot be changed.
#[cfg(feature = "bif-io")]
pub fn file_set_attribute(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileSetAttribute() expects 2 arguments: (path, attribute)".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let attribute = vm.to_string(args[1]).to_lowercase();

    let path = Path::new(&path_str);
    if !path.exists() {
        return Err(format!("File does not exist: {}", path_str));
    }

    match attribute.as_str() {
        "normal" | "default" => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::Permissions::from_mode(0o664);
                fs::set_permissions(path, perms).map_err(|e| e.to_string())?;
            }
        }
        "readonly" => {
            let mut perms = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
            perms.set_readonly(true);
            fs::set_permissions(path, perms).map_err(|e| e.to_string())?;
        }
        "archive" | "hidden" | "system" => {}
        _ => {
            return Err(format!(
                "The attribute provided [{}] is not valid",
                attribute
            ));
        }
    }

    Ok(BxValue::new_bool(true))
}

/// Implements `fileSetLastModified( path, date )`.
///
/// Sets the modification time of `path`. `date` is either milliseconds since
/// the Unix epoch or an RFC 3339 date-time string. Returns `true`.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, when `date`
/// cannot be parsed, or when the time cannot be set.
#[cfg(feature = "bif-io")]
pub fn file_set_last_modified(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileSetLastModified() expects 2 arguments: (path, date)".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let date_str = vm.to_string(args[1]);

    let epoch_millis: f64 = if let Ok(n) = date_str.parse::<f64>() {
        n
    } else if let Ok(date) = DateTime::parse_from_rfc3339(&date_str) {
        date.timestamp_millis() as f64
    } else {
        return Err(format!("Cannot parse date: {}", date_str));
    };

    let duration = std::time::Duration::from_millis(epoch_millis as u64);
    let system_time = std::time::UNIX_EPOCH + duration;
    let ft = filetime::FileTime::from_system_time(system_time);
    filetime::set_file_mtime(&path_str, ft).map_err(|e| e.to_string())?;

    Ok(BxValue::new_bool(true))
}

/// Implements `fileWriteLine( file, data )`.
///
/// Writes `data` as a new line: when the file is not empty, a line separator
/// is written first, so the file does not end with one, as in BoxLang. For a
/// file object from [`file_open`] it returns that object. For a path it opens
/// the file in `append` mode (creating it when missing), writes, closes it and
/// returns null.
///
/// # Errors
/// Returns an error when fewer than two arguments are given, when a file
/// object is not open in a write mode, or when the write fails.
#[cfg(feature = "bif-io")]
pub fn file_write_line(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileWriteLine() expects 2 arguments: (file, data)".to_string());
    }
    if let Some(id) = file_object_id(vm, args[0]) {
        return vm.native_object_call_method(id, "writeline", &[args[1]]);
    }
    let path = vm.to_string(args[0]);
    let data = vm.to_string(args[1]);
    FileObject::open(&path, Mode::Append, Charset::Utf8, None)?.write_line(&data)?;
    Ok(BxValue::new_null())
}

/// Implements `getCanonicalPath( path )`.
///
/// Returns the absolute path with symbolic links and `.`/`..` resolved. A
/// directory gets a trailing separator. When the path cannot be resolved, for
/// example because it does not exist, it is returned unchanged.
///
/// # Errors
/// Returns an error when no path argument is given.
#[cfg(feature = "bif-io")]
pub fn get_canonical_path(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("getCanonicalPath() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);

    match path.canonicalize() {
        Ok(canonical) => {
            let mut result = normalize_path_string(&canonical);
            if canonical.is_dir() && !result.ends_with(std::path::MAIN_SEPARATOR) {
                result.push(std::path::MAIN_SEPARATOR);
            }
            let s_id = vm.string_new(result);
            Ok(BxValue::new_ptr(s_id))
        }
        Err(_) => {
            let s_id = vm.string_new(path_str);
            Ok(BxValue::new_ptr(s_id))
        }
    }
}

/// Implements `getDirectoryFromPath( path )`.
///
/// Returns the directory part of `path`, including the trailing separator. A
/// path that already ends with `/` or `\\` is returned as given. A path with
/// no separator, or an empty path, gives `/`. The path is not checked on disk.
///
/// # Errors
/// Returns an error when no path argument is given.
#[cfg(feature = "bif-io")]
pub fn get_directory_from_path(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("getDirectoryFromPath() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);

    if path_str.is_empty() {
        let s_id = vm.string_new("/".to_string());
        return Ok(BxValue::new_ptr(s_id));
    }

    if path_str.ends_with('/') || path_str.ends_with('\\') {
        let s_id = vm.string_new(path_str);
        return Ok(BxValue::new_ptr(s_id));
    }

    let last_sep = path_str.rfind('/').or_else(|| path_str.rfind('\\'));
    let result = match last_sep {
        Some(idx) => path_str[..=idx].to_string(),
        None => "/".to_string(),
    };

    let s_id = vm.string_new(result);
    Ok(BxValue::new_ptr(s_id))
}

/// Implements `propertyFile( path )`.
///
/// Reads a Java-style properties file into a struct of strings. Blank lines,
/// lines starting with `#` or `!`, and lines without `=` are skipped. Keys and
/// values are trimmed. Line continuations, `:` separators and escapes are not
/// supported. With no path, or an empty path, an empty struct is returned.
///
/// # Errors
/// Returns an error when the file cannot be read as UTF-8 text.
#[cfg(feature = "bif-io")]
pub fn property_file(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        let struct_id = vm.struct_new();
        return Ok(BxValue::new_ptr(struct_id));
    }
    let path_str = vm.to_string(args[0]);
    if path_str.is_empty() {
        let struct_id = vm.struct_new();
        return Ok(BxValue::new_ptr(struct_id));
    }

    let content = fs::read_to_string(&path_str)
        .map_err(|e| format!("Cannot read property file [{}]: {}", path_str, e))?;

    let struct_id = vm.struct_new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let k = key.trim();
            let v = value.trim();
            let v_id = vm.string_new(v.to_string());
            vm.struct_set(struct_id, k, BxValue::new_ptr(v_id));
        }
    }

    Ok(BxValue::new_ptr(struct_id))
}

#[cfg(all(test, feature = "bif-io"))]
mod tests {
    use super::with_trailing_separator;

    #[test]
    fn preserves_either_supported_separator() {
        assert_eq!(
            with_trailing_separator("C:\\Temp\\".to_string()),
            "C:\\Temp\\"
        );
        assert_eq!(with_trailing_separator("C:/Temp/".to_string()), "C:/Temp/");
    }

    #[test]
    fn appends_the_host_separator_when_missing() {
        let result = with_trailing_separator("C:/Temp".to_string());
        assert!(result.ends_with(std::path::MAIN_SEPARATOR));
    }
}
