//! Logging, modeled on the BoxLang `LoggingService`.
//!
//! A *logger* is a named destination. `runtime` is the runtime's own log, `application` is where
//! `writeLog()` and `bx:log` write by default, and any other name writes `<logsDirectory>/<name>.log`.
//! An absolute path writes that file. Each logger has its own level (the root level when unset),
//! appender (`file` or `console`) and encoder (`text` or `json`), configured under `logging.loggers`.
//! File loggers rotate by size and are pruned by age and total size.

use std::{
    collections::{BTreeMap, HashMap},
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime},
};

pub const RUNTIME_LOGGER: &str = "runtime";
pub const APPLICATION_LOGGER: &str = "application";
/// The application name used when a script has none, as in BoxLang.
pub const DEFAULT_APPLICATION: &str = "no-application";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    /// A level as written in configuration: `OFF`, `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE`.
    pub fn parse(value: &str) -> Option<Level> {
        match value.trim().to_ascii_uppercase().as_str() {
            "OFF" => Some(Level::Off),
            "ERROR" => Some(Level::Error),
            "WARN" => Some(Level::Warn),
            "INFO" => Some(Level::Info),
            "DEBUG" => Some(Level::Debug),
            "TRACE" => Some(Level::Trace),
            _ => None,
        }
    }

    /// A `type` as `writeLog()` accepts it: the levels above plus `fatal`, `warning` and `information`.
    pub fn parse_type(value: &str) -> Option<Level> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fatal" | "fatal information" => Some(Level::Error),
            "warning" => Some(Level::Warn),
            "information" => Some(Level::Info),
            other => Level::parse(other),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Level::Off => "OFF",
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoder {
    Text,
    Json,
}

impl Encoder {
    pub fn parse(value: &str) -> Option<Encoder> {
        match value.trim().to_ascii_lowercase().as_str() {
            "text" => Some(Encoder::Text),
            "json" => Some(Encoder::Json),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appender {
    File,
    Console,
}

impl Appender {
    pub fn parse(value: &str) -> Option<Appender> {
        match value.trim().to_ascii_lowercase().as_str() {
            "file" => Some(Appender::File),
            "console" => Some(Appender::Console),
            _ => None,
        }
    }
}

/// One entry of `logging.loggers`. Unset values inherit from the root settings.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoggerConfig {
    pub level: Option<Level>,
    pub appender: Option<Appender>,
    pub encoder: Option<Encoder>,
}

#[derive(Clone, Debug)]
pub struct LoggingConfig {
    pub logs_directory: PathBuf,
    /// Days to keep rotated files. 0 keeps them forever.
    pub max_log_days: u64,
    /// Bytes per file before rotation. 0 never rotates.
    pub max_file_size: u64,
    /// Bytes across one logger's files. 0 means no cap.
    pub total_cap_size: u64,
    pub root_level: Level,
    pub encoder: Encoder,
    /// Keyed by lowercase logger name.
    pub loggers: BTreeMap<String, LoggerConfig>,
}

/// Parse `100MB`, `5GB`, `512KB` or a plain byte count.
pub fn parse_size(value: &str) -> Option<u64> {
    let value = value.trim().to_ascii_uppercase();
    let (digits, unit) = match value.find(|c: char| !c.is_ascii_digit()) {
        Some(at) => value.split_at(at),
        None => (value.as_str(), ""),
    };
    let factor: u64 = match unit.trim() {
        "" | "B" => 1,
        "KB" => 1 << 10,
        "MB" => 1 << 20,
        "GB" => 1 << 30,
        _ => return None,
    };
    digits.parse::<u64>().ok()?.checked_mul(factor)
}

enum Sink {
    File(Mutex<FileState>),
    Console,
}

struct FileState {
    file: File,
    size: u64,
}

pub struct Logger {
    level: Level,
    encoder: Encoder,
    sink: Sink,
    /// Only used by file loggers.
    path: PathBuf,
    max_file_size: u64,
    max_log_days: u64,
    total_cap_size: u64,
}

impl Logger {
    fn new(config: &LoggingConfig, logger: &LoggerConfig, path: PathBuf) -> io::Result<Logger> {
        let level = logger.level.unwrap_or(config.root_level);
        let encoder = logger.encoder.unwrap_or(config.encoder);
        let sink = match logger.appender.unwrap_or(Appender::File) {
            Appender::Console => Sink::Console,
            Appender::File => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                prune(&path, config.max_log_days, config.total_cap_size);
                let file = OpenOptions::new().create(true).append(true).open(&path)?;
                let size = file.metadata()?.len();
                Sink::File(Mutex::new(FileState { file, size }))
            }
        };
        Ok(Logger {
            level,
            encoder,
            sink,
            path,
            max_file_size: config.max_file_size,
            max_log_days: config.max_log_days,
            total_cap_size: config.total_cap_size,
        })
    }

    pub fn enabled(&self, level: Level) -> bool {
        level != Level::Off && level <= self.level
    }

    pub fn log(&self, level: Level, message: &str) {
        if !self.enabled(level) {
            return;
        }
        let now = chrono::Utc::now();
        let line = match self.encoder {
            Encoder::Text => format!(
                "{} [{}] {}\n",
                now.format("%Y-%m-%d %H:%M:%S%.3f"),
                level.name(),
                message
            ),
            Encoder::Json => {
                let entry = serde_json::json!({
                    "timestamp": now.to_rfc3339(),
                    "level": level.name(),
                    "message": message,
                });
                format!("{entry}\n")
            }
        };
        match &self.sink {
            Sink::Console => {
                let _ = io::stderr().write_all(line.as_bytes());
            }
            Sink::File(state) => {
                // Logging must never take the runtime down.
                let Ok(mut state) = state.lock() else { return };
                let max = self.max_file_size;
                if max > 0 && state.size > 0 && state.size + line.len() as u64 > max {
                    self.rotate(&mut state, now);
                }
                if state.file.write_all(line.as_bytes()).is_ok() {
                    state.size += line.len() as u64;
                }
            }
        }
    }

    fn rotate(&self, state: &mut FileState, now: chrono::DateTime<chrono::Utc>) {
        let rotated = rotated_path(&self.path, &now.format("%Y%m%d-%H%M%S%.3f").to_string());
        let _ = state.file.flush();
        if std::fs::rename(&self.path, rotated).is_ok()
            && let Ok(file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
        {
            state.file = file;
            state.size = 0;
            prune(&self.path, self.max_log_days, self.total_cap_size);
        }
    }
}

/// `orders.log` rotates to `orders.<stamp>.log`.
fn rotated_path(active: &Path, stamp: &str) -> PathBuf {
    let stem = active
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    active.with_file_name(format!("{stem}.{stamp}.log"))
}

/// Delete rotated files of one logger older than `max_log_days`, then the oldest until `total_cap` is met.
fn prune(active: &Path, max_log_days: u64, total_cap: u64) {
    let (Some(dir), Some(stem)) = (
        active.parent(),
        active.file_stem().map(|s| s.to_string_lossy().into_owned()),
    ) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let prefix = format!("{stem}.");
    let mut rotated: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    let mut total = std::fs::metadata(active).map_or(0, |m| m.len());
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stamp) = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".log"))
        else {
            continue;
        };
        // Only our own rotated files: digits, '-' and '.' between the stem and ".log".
        if stamp.is_empty()
            || !stamp
                .chars()
                .all(|c| c.is_ascii_digit() || c == '-' || c == '.')
        {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        total += meta.len();
        rotated.push((
            entry.path(),
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        ));
    }
    rotated.sort_by_key(|(_, modified, _)| *modified);

    if max_log_days > 0
        && let Some(cutoff) =
            SystemTime::now().checked_sub(Duration::from_secs(max_log_days * 86_400))
    {
        rotated.retain(|(path, modified, len)| {
            let expired = *modified < cutoff;
            if expired && std::fs::remove_file(path).is_ok() {
                total = total.saturating_sub(*len);
            }
            !expired
        });
    }
    if total_cap > 0 {
        for (path, _, len) in rotated {
            if total <= total_cap {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

/// The set of loggers for one runtime.
pub struct LogService {
    config: LoggingConfig,
    loggers: Mutex<HashMap<String, Arc<Logger>>>,
    runtime: Arc<Logger>,
}

impl LogService {
    pub fn new(config: LoggingConfig) -> io::Result<LogService> {
        let mut service = LogService {
            config,
            loggers: Mutex::new(HashMap::new()),
            // Replaced below: the runtime logger needs the service's config.
            runtime: Arc::new(Logger {
                level: Level::Off,
                encoder: Encoder::Text,
                sink: Sink::Console,
                path: PathBuf::new(),
                max_file_size: 0,
                max_log_days: 0,
                total_cap_size: 0,
            }),
        };
        service.runtime = service.logger(RUNTIME_LOGGER)?;
        Ok(service)
    }

    pub fn config(&self) -> &LoggingConfig {
        &self.config
    }

    /// The logger for a name or an absolute path, created on first use.
    ///
    /// A relative name may not contain path separators or `..`: it always stays in the logs directory.
    pub fn logger(&self, name: &str) -> io::Result<Arc<Logger>> {
        let name = name.trim();
        let name = if name.is_empty() {
            APPLICATION_LOGGER
        } else {
            name
        };
        let key = name.to_ascii_lowercase();
        if let Ok(loggers) = self.loggers.lock()
            && let Some(logger) = loggers.get(&key)
        {
            return Ok(logger.clone());
        }

        let with_extension = if name.to_ascii_lowercase().ends_with(".log") {
            name.to_string()
        } else {
            format!("{name}.log")
        };
        let path = if Path::new(name).is_absolute() {
            PathBuf::from(with_extension)
        } else {
            if name.contains(['/', '\\']) || name.contains("..") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Invalid logger name [{name}]"),
                ));
            }
            self.config.logs_directory.join(with_extension)
        };
        let settings = self.config.loggers.get(&key).copied().unwrap_or_default();
        let logger = Arc::new(Logger::new(&self.config, &settings, path)?);
        if let Ok(mut loggers) = self.loggers.lock() {
            loggers.insert(key, logger.clone());
        }
        Ok(logger)
    }

    /// `writeLog()` and `bx:log`: write `[application] text` at the level named by `type` to a logger.
    pub fn write(
        &self,
        text: &str,
        log_type: &str,
        logger: &str,
        application: &str,
    ) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        let level =
            Level::parse_type(log_type).ok_or_else(|| format!("Invalid log level: {log_type}"))?;
        let application = if application.is_empty() {
            DEFAULT_APPLICATION
        } else {
            application
        };
        let logger = self.logger(logger).map_err(|e| e.to_string())?;
        logger.log(level, &format!("[{application}] {text}"));
        Ok(())
    }

    pub fn runtime(&self) -> &Logger {
        &self.runtime
    }
}

static SERVICE: OnceLock<LogService> = OnceLock::new();
static APPLICATION: Mutex<String> = Mutex::new(String::new());

/// Install the process-wide log service. The first call wins; later calls are no-ops.
pub fn init(config: LoggingConfig) -> io::Result<()> {
    if SERVICE.get().is_none() {
        let _ = SERVICE.set(LogService::new(config)?);
    }
    Ok(())
}

pub fn service() -> Option<&'static LogService> {
    SERVICE.get()
}

/// Set the application name that `writeLog()` puts in front of each message.
pub fn set_application(name: &str) {
    if let Ok(mut current) = APPLICATION.lock() {
        *current = name.to_string();
    }
}

pub fn application() -> String {
    APPLICATION
        .lock()
        .map(|name| name.clone())
        .unwrap_or_default()
}

/// `writeLog()` and `bx:log` on the process-wide service. Does nothing when logging is not set up.
pub fn write(
    text: &str,
    log_type: &str,
    logger: &str,
    use_application: bool,
) -> Result<(), String> {
    let Some(service) = service() else {
        return Ok(());
    };
    let application = if use_application {
        application()
    } else {
        String::new()
    };
    service.write(text, log_type, logger, &application)
}

pub fn enabled(level: Level) -> bool {
    service().is_some_and(|service| service.runtime().enabled(level))
}

/// A message from the runtime itself, written to the `runtime` logger.
pub fn log(level: Level, message: impl AsRef<str>) {
    if let Some(service) = service() {
        service.runtime().log(level, message.as_ref());
    }
}

pub fn error(message: impl AsRef<str>) {
    log(Level::Error, message);
}
pub fn warn(message: impl AsRef<str>) {
    log(Level::Warn, message);
}
pub fn info(message: impl AsRef<str>) {
    log(Level::Info, message);
}
pub fn debug(message: impl AsRef<str>) {
    log(Level::Debug, message);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: &Path) -> LoggingConfig {
        LoggingConfig {
            logs_directory: dir.to_path_buf(),
            max_log_days: 90,
            max_file_size: 0,
            total_cap_size: 0,
            root_level: Level::Info,
            encoder: Encoder::Text,
            loggers: BTreeMap::new(),
        }
    }

    fn read(dir: &Path, file: &str) -> String {
        std::fs::read_to_string(dir.join(file)).unwrap_or_default()
    }

    #[test]
    fn parses_sizes_levels_and_types() {
        assert_eq!(parse_size("100MB"), Some(100 << 20));
        assert_eq!(parse_size("5gb"), Some(5 << 30));
        assert_eq!(parse_size("512 KB"), Some(512 << 10));
        assert_eq!(parse_size("42"), Some(42));
        assert_eq!(parse_size("lots"), None);
        assert_eq!(Level::parse("warn"), Some(Level::Warn));
        assert_eq!(Level::parse("information"), None);
        assert_eq!(Level::parse_type("Information"), Some(Level::Info));
        assert_eq!(Level::parse_type("WARNING"), Some(Level::Warn));
        assert_eq!(Level::parse_type("fatal"), Some(Level::Error));
        assert_eq!(Level::parse_type("fatal information"), Some(Level::Error));
        assert_eq!(Level::parse_type("nope"), None);
    }

    #[test]
    fn filters_by_level_and_writes_text() {
        let dir = tempfile::tempdir().unwrap();
        let service = LogService::new(config(dir.path())).unwrap();
        let runtime = service.runtime();
        runtime.log(Level::Debug, "hidden");
        runtime.log(Level::Warn, "shown");
        let text = read(dir.path(), "runtime.log");
        assert!(!text.contains("hidden"));
        assert!(text.contains("[WARN] shown"), "{text}");
    }

    #[test]
    fn writes_json_encoder() {
        let dir = tempfile::tempdir().unwrap();
        let service = LogService::new(LoggingConfig {
            encoder: Encoder::Json,
            ..config(dir.path())
        })
        .unwrap();
        service.runtime().log(Level::Error, "boom");
        let entry: serde_json::Value =
            serde_json::from_str(read(dir.path(), "runtime.log").trim()).unwrap();
        assert_eq!(entry["level"], "ERROR");
        assert_eq!(entry["message"], "boom");
    }

    #[test]
    fn write_uses_application_log_with_jvm_message_format() {
        let dir = tempfile::tempdir().unwrap();
        let service = LogService::new(config(dir.path())).unwrap();
        service
            .write("Order shipped", "information", "", "shop")
            .unwrap();
        service.write("No app", "error", "", "").unwrap();
        let text = read(dir.path(), "application.log");
        assert!(text.contains("[INFO] [shop] Order shipped"), "{text}");
        assert!(text.contains("[ERROR] [no-application] No app"), "{text}");
    }

    #[test]
    fn named_loggers_write_their_own_file_and_respect_their_level() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config(dir.path());
        config.loggers.insert(
            "orders".into(),
            LoggerConfig {
                level: Some(Level::Error),
                ..Default::default()
            },
        );
        let service = LogService::new(config).unwrap();
        service.write("skipped", "warning", "orders", "").unwrap();
        service.write("kept", "error", "Orders", "").unwrap();
        service.write("other", "info", "audit.log", "").unwrap();
        let orders = read(dir.path(), "orders.log");
        assert!(
            !orders.contains("skipped") && orders.contains("kept"),
            "{orders}"
        );
        assert!(read(dir.path(), "audit.log").contains("other"));
    }

    #[test]
    fn absolute_path_writes_that_file() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let service = LogService::new(config(dir.path())).unwrap();
        let target = elsewhere.path().join("custom");
        service
            .write("hello", "info", target.to_str().unwrap(), "")
            .unwrap();
        assert!(read(elsewhere.path(), "custom.log").contains("hello"));
    }

    #[test]
    fn relative_names_cannot_leave_the_logs_directory() {
        let dir = tempfile::tempdir().unwrap();
        let service = LogService::new(config(dir.path())).unwrap();
        for bad in ["../escape", "sub/dir", "a\\b", ".."] {
            assert!(service.write("x", "info", bad, "").is_err(), "{bad}");
        }
    }

    #[test]
    fn invalid_type_is_an_error_and_empty_text_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let service = LogService::new(config(dir.path())).unwrap();
        assert_eq!(
            service.write("x", "loud", "", "").unwrap_err(),
            "Invalid log level: loud"
        );
        service.write("", "info", "", "").unwrap();
        assert!(!dir.path().join("application.log").exists());
    }

    #[test]
    fn rotates_when_file_exceeds_max_size() {
        let dir = tempfile::tempdir().unwrap();
        let service = LogService::new(LoggingConfig {
            max_file_size: 200,
            ..config(dir.path())
        })
        .unwrap();
        for i in 0..20 {
            service
                .runtime()
                .log(Level::Error, &format!("message number {i}"));
        }
        let rotated = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("runtime.2"))
            .count();
        assert!(rotated > 0, "expected rotated files");
        assert!(
            std::fs::metadata(dir.path().join("runtime.log"))
                .unwrap()
                .len()
                <= 200
        );
    }

    #[test]
    fn total_cap_deletes_oldest_rotated_files_of_one_logger_only() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..4 {
            std::fs::write(
                dir.path().join(format!("runtime.2026{i}.log")),
                vec![b'x'; 100],
            )
            .unwrap();
        }
        std::fs::write(dir.path().join("runtime-other.log"), vec![b'x'; 100]).unwrap();
        std::fs::write(dir.path().join("runtime.notes.log"), vec![b'x'; 100]).unwrap();
        prune(&dir.path().join("runtime.log"), 90, 250);
        let remaining = |name: &str| dir.path().join(name).exists();
        assert!(remaining("runtime-other.log") && remaining("runtime.notes.log"));
        let rotated = (0..4)
            .filter(|i| remaining(&format!("runtime.2026{i}.log")))
            .count();
        assert_eq!(rotated, 2);
    }
}
