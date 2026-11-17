//! File logger writing to `<logsDirectory>/matchbox.log` with level filtering, size rotation,
//! age pruning, and the `text` or `json` encoder from the `logging` config block.

use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime},
};

const ACTIVE_FILE: &str = "matchbox.log";
const ROTATED_PREFIX: &str = "matchbox-";

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

#[derive(Clone, Debug)]
pub struct LoggingConfig {
    pub logs_directory: PathBuf,
    /// Days to keep rotated files. 0 keeps them forever.
    pub max_log_days: u64,
    /// Bytes per file before rotation. 0 never rotates.
    pub max_file_size: u64,
    /// Bytes across all log files. 0 means no cap.
    pub total_cap_size: u64,
    pub root_level: Level,
    pub encoder: Encoder,
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

struct State {
    file: File,
    size: u64,
}

pub struct Logger {
    config: LoggingConfig,
    state: Mutex<State>,
}

impl Logger {
    pub fn new(config: LoggingConfig) -> io::Result<Logger> {
        std::fs::create_dir_all(&config.logs_directory)?;
        prune(&config);
        let path = config.logs_directory.join(ACTIVE_FILE);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        Ok(Logger {
            config,
            state: Mutex::new(State { file, size }),
        })
    }

    pub fn enabled(&self, level: Level) -> bool {
        level != Level::Off && level <= self.config.root_level
    }

    pub fn log(&self, level: Level, message: &str) {
        if !self.enabled(level) {
            return;
        }
        let now = chrono::Utc::now();
        let line = match self.config.encoder {
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
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let max = self.config.max_file_size;
        if max > 0 && state.size > 0 && state.size + line.len() as u64 > max {
            self.rotate(&mut state, now);
        }
        // Logging must never take the runtime down.
        if state.file.write_all(line.as_bytes()).is_ok() {
            state.size += line.len() as u64;
        }
    }

    fn rotate(&self, state: &mut State, now: chrono::DateTime<chrono::Utc>) {
        let dir = &self.config.logs_directory;
        let rotated = dir.join(format!(
            "{ROTATED_PREFIX}{}.log",
            now.format("%Y%m%d-%H%M%S%.3f")
        ));
        let _ = state.file.flush();
        if std::fs::rename(dir.join(ACTIVE_FILE), rotated).is_ok()
            && let Ok(file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join(ACTIVE_FILE))
        {
            state.file = file;
            state.size = 0;
            prune(&self.config);
        }
    }
}

/// Delete rotated files older than `max_log_days`, then the oldest until the total cap is met.
fn prune(config: &LoggingConfig) {
    let Ok(entries) = std::fs::read_dir(&config.logs_directory) else {
        return;
    };
    let mut rotated: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    let mut total = 0u64;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = entry.metadata() else { continue };
        if name == ACTIVE_FILE {
            total += meta.len();
        } else if name.starts_with(ROTATED_PREFIX) && name.ends_with(".log") {
            total += meta.len();
            rotated.push((
                entry.path(),
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                meta.len(),
            ));
        }
    }
    rotated.sort_by_key(|(_, modified, _)| *modified);

    if config.max_log_days > 0
        && let Some(cutoff) =
            SystemTime::now().checked_sub(Duration::from_secs(config.max_log_days * 86_400))
    {
        rotated.retain(|(path, modified, len)| {
            let expired = *modified < cutoff;
            if expired && std::fs::remove_file(path).is_ok() {
                total = total.saturating_sub(*len);
            }
            !expired
        });
    }
    if config.total_cap_size > 0 {
        for (path, _, len) in rotated {
            if total <= config.total_cap_size {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Install the process-wide logger. The first call wins; later calls are no-ops.
pub fn init(config: LoggingConfig) -> io::Result<()> {
    if LOGGER.get().is_none() {
        let _ = LOGGER.set(Logger::new(config)?);
    }
    Ok(())
}

pub fn enabled(level: Level) -> bool {
    LOGGER.get().is_some_and(|logger| logger.enabled(level))
}

pub fn log(level: Level, message: impl AsRef<str>) {
    if let Some(logger) = LOGGER.get() {
        logger.log(level, message.as_ref());
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

/// Path of the active log file for a logs directory.
pub fn active_file(logs_directory: &Path) -> PathBuf {
    logs_directory.join(ACTIVE_FILE)
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
        }
    }

    #[test]
    fn parses_sizes_and_levels() {
        assert_eq!(parse_size("100MB"), Some(100 << 20));
        assert_eq!(parse_size("5gb"), Some(5 << 30));
        assert_eq!(parse_size("512 KB"), Some(512 << 10));
        assert_eq!(parse_size("42"), Some(42));
        assert_eq!(parse_size("lots"), None);
        assert_eq!(Level::parse("warn"), Some(Level::Warn));
        assert_eq!(Level::parse("nope"), None);
    }

    #[test]
    fn filters_by_level_and_writes_text() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(config(dir.path())).unwrap();
        logger.log(Level::Debug, "hidden");
        logger.log(Level::Warn, "shown");
        let text = std::fs::read_to_string(active_file(dir.path())).unwrap();
        assert!(!text.contains("hidden"));
        assert!(text.contains("[WARN] shown"), "{text}");
    }

    #[test]
    fn writes_json_encoder() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(LoggingConfig {
            encoder: Encoder::Json,
            ..config(dir.path())
        })
        .unwrap();
        logger.log(Level::Error, "boom");
        let text = std::fs::read_to_string(active_file(dir.path())).unwrap();
        let entry: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(entry["level"], "ERROR");
        assert_eq!(entry["message"], "boom");
    }

    #[test]
    fn rotates_when_file_exceeds_max_size() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(LoggingConfig {
            max_file_size: 200,
            ..config(dir.path())
        })
        .unwrap();
        for i in 0..20 {
            logger.log(Level::Error, &format!("message number {i}"));
        }
        let rotated = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(ROTATED_PREFIX))
            .count();
        assert!(rotated > 0, "expected rotated files");
        assert!(std::fs::metadata(active_file(dir.path())).unwrap().len() <= 200);
    }

    #[test]
    fn total_cap_deletes_oldest_rotated_files() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..4 {
            std::fs::write(
                dir.path().join(format!("{ROTATED_PREFIX}{i}.log")),
                vec![b'x'; 100],
            )
            .unwrap();
        }
        prune(&LoggingConfig {
            total_cap_size: 250,
            ..config(dir.path())
        });
        let remaining = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(remaining, 2);
    }
}
