use std::sync::atomic::{AtomicBool, Ordering};

static SHOULD_LOG: AtomicBool = AtomicBool::new(false);

pub fn enable_logging() {
    SHOULD_LOG.store(true, Ordering::SeqCst);
}

pub fn disable_logging() {
    SHOULD_LOG.store(false, Ordering::SeqCst);
}

pub fn is_logging_enabled() -> bool {
    SHOULD_LOG.load(Ordering::SeqCst)
}

pub fn log_message(message: impl AsRef<str>) {
    if is_logging_enabled() {
        eprintln!("{}", message.as_ref());
    }
}

#[macro_export]
macro_rules! try_log {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {{
        if $crate::is_logging_enabled() {
            $crate::log_message(&format!(concat!("[LOG] ", $fmt), $($arg),*));
        }
    }};
    ($message:expr $(,)?) => {{
        if $crate::is_logging_enabled() {
            $crate::log_message(&format!("[LOG] {}", $message));
        }
    }};
}
