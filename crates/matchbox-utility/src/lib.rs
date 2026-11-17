//! Runtime support shared by the MatchBox CLI, runners and server: the `.env` loader, the MatchBox home,
//! the `boxlang.json` configuration, and logging.
//!
//! Startup calls [`bootstrap::bootstrap`] once, which uses the other modules in this order:
//!
//! 1. [`env_file`] loads `~/.matchbox.env` and `./.env` into the process environment.
//! 2. [`home`] resolves and creates the MatchBox home (`~/.matchbox`, or `MATCHBOX_HOME`).
//! 3. [`config`] layers the `boxlang.json` files and expands placeholders.
//! 4. [`logger`] installs the process-wide log service that `writeLog()` and `bx:log` write to.
//!
//! The crate also keeps the older `--verbose` switch: [`enable_logging`] and the [`try_log!`] macro print to stderr.
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/// Process startup: runs the other modules in order and returns a [`bootstrap::Runtime`].
pub mod bootstrap;
/// The layered `boxlang.json` runtime configuration.
pub mod config;
/// Loading of `.env` files into the process environment.
pub mod env_file;
/// The MatchBox home directory and its default `config/boxlang.json`.
pub mod home;
/// Named loggers with levels, file or console output, rotation and pruning.
pub mod logger;

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether [`log_message`] and [`try_log!`] also print to stderr (the `--verbose` switch).
static SHOULD_LOG: AtomicBool = AtomicBool::new(false);

/// Turn on stderr output for [`log_message`] and [`try_log!`].
pub fn enable_logging() {
    SHOULD_LOG.store(true, Ordering::SeqCst);
}

/// Turn off stderr output for [`log_message`] and [`try_log!`].
pub fn disable_logging() {
    SHOULD_LOG.store(false, Ordering::SeqCst);
}

/// Whether stderr output is on. See [`enable_logging`].
pub fn is_logging_enabled() -> bool {
    SHOULD_LOG.load(Ordering::SeqCst)
}

/// Print a message to stderr when [`enable_logging`] was called, and write it at `DEBUG` to the `runtime` log.
///
/// The file write is filtered by the `runtime` logger's level, so it is dropped unless that level is `DEBUG` or lower.
pub fn log_message(message: impl AsRef<str>) {
    if is_logging_enabled() {
        eprintln!("{}", message.as_ref());
    }
    logger::debug(message);
}

/// Log a formatted message through [`log_message`], building the string only when something will use it.
///
/// Accepts a format string with arguments, or a single displayable expression. Each message is prefixed with `[LOG] `.
///
/// # Examples
///
/// ```
/// matchbox_utility::try_log!("compiled {} functions", 3);
/// matchbox_utility::try_log!("done");
/// ```
#[macro_export]
macro_rules! try_log {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {{
        if $crate::is_logging_enabled() || $crate::logger::enabled($crate::logger::Level::Debug) {
            $crate::log_message(&format!(concat!("[LOG] ", $fmt), $($arg),*));
        }
    }};
    ($message:expr $(,)?) => {{
        if $crate::is_logging_enabled() || $crate::logger::enabled($crate::logger::Level::Debug) {
            $crate::log_message(&format!("[LOG] {}", $message));
        }
    }};
}
