//! Runtime configuration in the BoxLang `boxlang.json` format.
//!
//! Layers, later ones override earlier ones (objects merge key by key, other values replace):
//! 1. `<home>/config/boxlang.json`
//! 2. `./boxlang.json` in the working directory
//! 3. `--bx-config <file>` or `MATCHBOX_CONFIG`
//!
//! Comments (`//`, `/* */`) are allowed. Unknown keys are kept in `raw` and otherwise ignored.

use crate::logger::{Appender, Encoder, Level, LoggerConfig, LoggingConfig, parse_size};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

/// The runtime configuration after layering and placeholder expansion.
///
/// Only the keys MatchBox uses are read into fields; everything else stays available in [`Config::raw`].
#[derive(Clone, Debug)]
pub struct Config {
    /// The `debugMode` key. When true the log levels are raised to `DEBUG`.
    pub debug_mode: bool,
    /// The `logging` block, with defaults for missing keys.
    pub logging: LoggingConfig,
    /// The merged document after placeholder expansion.
    pub raw: Value,
}

/// Remove `//` and `/* */` comments, leaving string contents untouched.
///
/// # Examples
///
/// ```
/// use matchbox_utility::config::strip_comments;
///
/// let json = strip_comments("{ // a comment\n \"url\": \"http://example.com\" /* more */ }");
/// let value: serde_json::Value = serde_json::from_str(&json).unwrap();
/// assert_eq!(value["url"], "http://example.com");
/// ```
pub fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match (c, chars.peek().copied()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = '\0';
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                    }
                    if prev == '*' && next == '/' {
                        break;
                    }
                    prev = next;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Merge `overlay` into `base`: objects merge key by key, anything else replaces.
fn merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

/// Replace `${matchbox-home}`, `${boxlang-home}` and `${user-dir}` in every string of a JSON value, in place.
fn expand(value: &mut Value, home: &Path, cwd: &Path) {
    match value {
        Value::String(s) => {
            *s = s
                .replace("${matchbox-home}", &home.to_string_lossy())
                .replace("${boxlang-home}", &home.to_string_lossy())
                .replace("${user-dir}", &cwd.to_string_lossy());
        }
        Value::Array(items) => items.iter_mut().for_each(|v| expand(v, home, cwd)),
        Value::Object(map) => map.values_mut().for_each(|v| expand(v, home, cwd)),
        _ => {}
    }
}

/// The config files to layer, lowest priority first: the home's file, `./boxlang.json` if it exists,
/// then `--bx-config` or `MATCHBOX_CONFIG`. A file named by flag or variable must exist.
///
/// # Errors
///
/// Returns [`io::ErrorKind::NotFound`] when `flag` or `env_config` names a file that does not exist.
pub fn resolve_layers(
    flag: Option<&str>,
    env_config: Option<&str>,
    cwd: &Path,
    home: &Path,
) -> io::Result<Vec<PathBuf>> {
    let home_file = crate::home::config_path(home);
    let mut layers = vec![home_file.clone()];
    let project = cwd.join("boxlang.json");
    if project.is_file() && !same_file(&project, &home_file) {
        layers.push(project);
    }
    if let Some(path) = flag.or(env_config).filter(|p| !p.is_empty()) {
        let path = cwd.join(path);
        if !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("config file not found: {}", path.display()),
            ));
        }
        layers.push(path);
    }
    Ok(layers)
}

/// Whether two paths are the same file once resolved. A path that cannot be resolved is never the same.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Read and merge the layers. A layer that is not valid JSON is an error that names the file.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidData`] naming the file when a layer is not valid JSON, or the I/O error when a layer
/// exists but cannot be read.
pub fn load(layers: &[PathBuf], home: &Path, cwd: &Path) -> io::Result<Config> {
    let mut merged = Value::Object(Default::default());
    for path in layers {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            // The home file is seeded by `home::ensure`; a missing optional layer is skipped.
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let layer: Value = serde_json::from_str(&strip_comments(&text)).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid config {}: {e}", path.display()),
            )
        })?;
        merge(&mut merged, layer);
    }
    expand(&mut merged, home, cwd);
    Ok(from_value(merged, home))
}

/// Read the `debugMode` and `logging` keys of a merged document into a [`Config`].
///
/// Missing or invalid values fall back to the defaults of the seeded `boxlang.json`.
fn from_value(raw: Value, home: &Path) -> Config {
    let debug_mode = raw
        .get("debugMode")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let log = raw.get("logging").cloned().unwrap_or(Value::Null);
    let text = |key: &str| log.get(key).and_then(Value::as_str);
    let number = |key: &str, default: u64| log.get(key).and_then(Value::as_u64).unwrap_or(default);
    let size = |key: &str, default: u64| text(key).and_then(parse_size).unwrap_or(default);

    let mut root_level = text("rootLevel")
        .and_then(Level::parse)
        .unwrap_or(Level::Warn);
    if debug_mode && root_level < Level::Debug {
        root_level = Level::Debug;
    }

    let mut loggers = BTreeMap::new();
    if let Some(Value::Object(map)) = log.get("loggers") {
        for (name, entry) in map {
            let field = |key: &str| entry.get(key).and_then(Value::as_str);
            let mut config = LoggerConfig {
                level: field("level").and_then(Level::parse),
                appender: field("appender").and_then(Appender::parse),
                encoder: field("encoder").and_then(Encoder::parse),
            };
            if debug_mode && config.level.is_some_and(|level| level < Level::Debug) {
                config.level = Some(Level::Debug);
            }
            loggers.insert(name.to_ascii_lowercase(), config);
        }
    }

    let logging = LoggingConfig {
        logs_directory: text("logsDirectory").map_or_else(|| home.join("logs"), PathBuf::from),
        max_log_days: number("maxLogDays", 90),
        max_file_size: size("maxFileSize", 100 << 20),
        total_cap_size: size("totalCapSize", 5 << 30),
        root_level,
        encoder: text("defaultEncoder")
            .and_then(Encoder::parse)
            .unwrap_or(Encoder::Text),
        loggers,
    };
    Config {
        debug_mode,
        logging,
        raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn strips_comments_but_not_strings() {
        let out = strip_comments("{ // c\n \"a\": \"http://x\", /* b\n */ \"b\": 1 }");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["a"], "http://x");
        assert_eq!(v["b"], 1);
    }

    #[test]
    fn seeded_default_config_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "boxlang.json", crate::home::DEFAULT_CONFIG);
        let config = load(&[path], Path::new("/h"), Path::new("/cwd")).unwrap();
        assert!(!config.debug_mode);
        assert_eq!(config.logging.logs_directory, PathBuf::from("/h/logs"));
        assert_eq!(config.logging.root_level, Level::Warn);
        assert_eq!(config.logging.max_file_size, 100 << 20);
        assert_eq!(config.logging.total_cap_size, 5 << 30);
        assert_eq!(config.logging.encoder, Encoder::Text);
        assert_eq!(config.logging.loggers["runtime"].level, Some(Level::Info));
        assert_eq!(
            config.logging.loggers["application"].level,
            Some(Level::Warn)
        );
        assert_eq!(
            config.logging.loggers["runtime"].appender,
            Some(Appender::File)
        );
    }

    #[test]
    fn layers_merge_objects_and_replace_values() {
        let dir = tempfile::tempdir().unwrap();
        let home = write(
            dir.path(),
            "home.json",
            r#"{ "logging": { "rootLevel": "WARN", "maxLogDays": 30, "loggers": { "application": { "level": "WARN" }, "runtime": { "level": "INFO" } } } }"#,
        );
        let project = write(
            dir.path(),
            "project.json",
            r#"{ "mappings": { "/": "${user-dir}" }, "logging": { "rootLevel": "ERROR", "loggers": { "application": { "level": "DEBUG" } } } }"#,
        );
        let config = load(&[home, project], Path::new("/h"), Path::new("/cwd")).unwrap();
        assert_eq!(config.logging.root_level, Level::Error);
        assert_eq!(
            config.logging.max_log_days, 30,
            "untouched key survives the overlay"
        );
        assert_eq!(
            config.logging.loggers["application"].level,
            Some(Level::Debug)
        );
        assert_eq!(config.logging.loggers["runtime"].level, Some(Level::Info));
        assert_eq!(config.raw["mappings"]["/"], "/cwd");
    }

    #[test]
    fn debug_mode_raises_levels_and_placeholders_expand() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "c.json",
            r#"{ "debugMode": true, "logging": { "rootLevel": "ERROR", "defaultEncoder": "json", "logsDirectory": "${boxlang-home}/l", "loggers": { "Orders": { "level": "WARN", "appender": "console" } } } }"#,
        );
        let config = load(&[path], Path::new("/h"), Path::new("/cwd")).unwrap();
        assert_eq!(config.logging.root_level, Level::Debug);
        assert_eq!(config.logging.encoder, Encoder::Json);
        assert_eq!(config.logging.logs_directory, PathBuf::from("/h/l"));
        assert_eq!(config.logging.loggers["orders"].level, Some(Level::Debug));
        assert_eq!(
            config.logging.loggers["orders"].appender,
            Some(Appender::Console)
        );
    }

    #[test]
    fn invalid_json_is_an_error_that_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "c.json", "{ nope");
        let error = load(&[path], Path::new("/h"), dir.path()).unwrap_err();
        assert!(error.to_string().contains("c.json"), "{error}");
    }

    #[test]
    fn layer_order_and_explicit_file_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let home = cwd.join("home");
        let home_file = home.join("config/boxlang.json");
        assert_eq!(
            resolve_layers(None, None, cwd, &home).unwrap(),
            vec![home_file.clone()]
        );

        write(cwd, "boxlang.json", "{}");
        write(cwd, "env.json", "{}");
        write(cwd, "flag.json", "{}");
        assert_eq!(
            resolve_layers(None, None, cwd, &home).unwrap(),
            vec![home_file.clone(), cwd.join("boxlang.json")]
        );
        assert_eq!(
            resolve_layers(None, Some("env.json"), cwd, &home).unwrap(),
            vec![
                home_file.clone(),
                cwd.join("boxlang.json"),
                cwd.join("env.json")
            ]
        );
        assert_eq!(
            resolve_layers(Some("flag.json"), Some("env.json"), cwd, &home)
                .unwrap()
                .last()
                .unwrap(),
            &cwd.join("flag.json")
        );
        assert!(resolve_layers(Some("missing.json"), None, cwd, &home).is_err());
    }

    #[test]
    fn project_file_that_is_the_home_file_is_not_layered_twice() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        std::fs::create_dir_all(home.join("config")).unwrap();
        // cwd == <home>/config, so ./boxlang.json is the home file itself.
        let cwd = home.join("config");
        write(&cwd, "boxlang.json", "{}");
        assert_eq!(resolve_layers(None, None, &cwd, &home).unwrap().len(), 1);
    }
}
