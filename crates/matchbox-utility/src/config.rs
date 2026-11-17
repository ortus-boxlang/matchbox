//! Runtime configuration: the BoxLang `boxlang.json` spec, stored as `matchbox.json`.
//! Comments (`//`, `/* */`) are allowed. Unknown keys are kept in `raw` and otherwise ignored.

use crate::logger::{self, Encoder, Level, LoggingConfig};
use serde_json::Value;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Config {
    pub debug_mode: bool,
    pub logging: LoggingConfig,
    /// The whole document after placeholder expansion.
    pub raw: Value,
}

/// Remove `//` and `/* */` comments, leaving string contents untouched.
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

fn expand(value: &mut Value, home: &Path, cwd: &Path) {
    match value {
        Value::String(s) => {
            *s = s
                .replace("${matchbox-home}", &home.to_string_lossy())
                .replace("${user-dir}", &cwd.to_string_lossy());
        }
        Value::Array(items) => items.iter_mut().for_each(|v| expand(v, home, cwd)),
        Value::Object(map) => map.values_mut().for_each(|v| expand(v, home, cwd)),
        _ => {}
    }
}

/// Pick the config file: `--bx-config`, `MATCHBOX_CONFIG`, `./.matchbox.json`, then the home's file.
/// An explicitly named file must exist.
pub fn resolve_path(
    flag: Option<&str>,
    env_config: Option<&str>,
    cwd: &Path,
    home: &Path,
) -> io::Result<PathBuf> {
    if let Some(path) = flag.or(env_config).filter(|p| !p.is_empty()) {
        let path = cwd.join(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("config file not found: {}", path.display()),
            ))
        };
    }
    let project = cwd.join(".matchbox.json");
    Ok(if project.is_file() {
        project
    } else {
        crate::home::config_path(home)
    })
}

pub fn load(path: &Path, home: &Path, cwd: &Path) -> io::Result<Config> {
    let text = std::fs::read_to_string(path)?;
    let mut raw: Value = serde_json::from_str(&strip_comments(&text)).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid config {}: {e}", path.display()),
        )
    })?;
    expand(&mut raw, home, cwd);
    Ok(from_value(raw, home))
}

fn from_value(raw: Value, home: &Path) -> Config {
    let debug_mode = raw
        .get("debugMode")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let log = raw.get("logging").cloned().unwrap_or(Value::Null);
    let text = |key: &str| log.get(key).and_then(Value::as_str);
    let number = |key: &str, default: u64| log.get(key).and_then(Value::as_u64).unwrap_or(default);
    let size = |key: &str, default: u64| text(key).and_then(logger::parse_size).unwrap_or(default);

    let mut root_level = text("rootLevel")
        .and_then(Level::parse)
        .unwrap_or(Level::Warn);
    if debug_mode && root_level < Level::Debug {
        root_level = Level::Debug;
    }
    let logging = LoggingConfig {
        logs_directory: text("logsDirectory").map_or_else(|| home.join("logs"), PathBuf::from),
        max_log_days: number("maxLogDays", 90),
        max_file_size: size("maxFileSize", 100 << 20),
        total_cap_size: size("totalCapSize", 5 << 30),
        root_level,
        encoder: if text("defaultEncoder").is_some_and(|e| e.eq_ignore_ascii_case("json")) {
            Encoder::Json
        } else {
            Encoder::Text
        },
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

    #[test]
    fn strips_comments_but_not_strings() {
        let out = strip_comments("{ // c\n \"a\": \"http://x\", /* b\n */ \"b\": 1 }");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["a"], "http://x");
        assert_eq!(v["b"], 1);
    }

    #[test]
    fn seeded_default_config_loads() {
        let home = Path::new("/h");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("matchbox.json");
        std::fs::write(&path, crate::home::DEFAULT_CONFIG).unwrap();
        let config = load(&path, home, Path::new("/cwd")).unwrap();
        assert!(!config.debug_mode);
        assert_eq!(config.logging.logs_directory, PathBuf::from("/h/logs"));
        assert_eq!(config.logging.root_level, Level::Warn);
        assert_eq!(config.logging.max_file_size, 100 << 20);
        assert_eq!(config.logging.total_cap_size, 5 << 30);
        assert_eq!(config.logging.encoder, Encoder::Text);
    }

    #[test]
    fn debug_mode_raises_level_and_unknown_keys_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(&path, r#"{ "debugMode": true, "logging": { "rootLevel": "ERROR", "defaultEncoder": "json" }, "mappings": { "/": "${user-dir}" } }"#).unwrap();
        let config = load(&path, Path::new("/h"), Path::new("/cwd")).unwrap();
        assert_eq!(config.logging.root_level, Level::Debug);
        assert_eq!(config.logging.encoder, Encoder::Json);
        assert_eq!(config.raw["mappings"]["/"], "/cwd");
    }

    #[test]
    fn invalid_json_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(&path, "{ nope").unwrap();
        assert!(load(&path, Path::new("/h"), dir.path()).is_err());
    }

    #[test]
    fn path_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let home = cwd.join("home");
        // default: home config
        assert_eq!(
            resolve_path(None, None, cwd, &home).unwrap(),
            home.join("config/matchbox.json")
        );
        // project file beats home
        std::fs::write(cwd.join(".matchbox.json"), "{}").unwrap();
        assert_eq!(
            resolve_path(None, None, cwd, &home).unwrap(),
            cwd.join(".matchbox.json")
        );
        // env beats project, flag beats env
        std::fs::write(cwd.join("env.json"), "{}").unwrap();
        std::fs::write(cwd.join("flag.json"), "{}").unwrap();
        assert_eq!(
            resolve_path(None, Some("env.json"), cwd, &home).unwrap(),
            cwd.join("env.json")
        );
        assert_eq!(
            resolve_path(Some("flag.json"), Some("env.json"), cwd, &home).unwrap(),
            cwd.join("flag.json")
        );
        // an explicit file must exist
        assert!(resolve_path(Some("missing.json"), None, cwd, &home).is_err());
    }
}
