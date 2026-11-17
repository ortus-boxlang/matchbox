//! Standard output that survives a closed pipe.
//!
//! `print!` and `println!` panic when stdout is closed (`failed printing to stdout: Broken pipe`), for example
//! `matchbox script.bxs | head -1` or a native binary piped into `grep -q`. A command line tool should stop
//! quietly there, like ripgrep or cat do.

use std::io::{self, Write};

/// Writes `text` to stdout, then `\n` when `newline` is set.
///
/// A closed reader (`ErrorKind::BrokenPipe`) ends the process with exit code 0 and no message: nobody is left
/// to read the output. Any other write error is ignored, as output is best effort, and never panics.
pub fn write_stdout(text: &str, newline: bool) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let result = out
        .write_all(text.as_bytes())
        .and_then(|_| if newline { out.write_all(b"\n") } else { Ok(()) });
    if let Err(error) = result {
        exit_on_broken_pipe(&error);
    }
}

/// Flushes stdout; a closed pipe ends the process like `write_stdout` does.
pub fn flush_stdout() {
    if let Err(error) = io::stdout().flush() {
        exit_on_broken_pipe(&error);
    }
}

/// Ends the process with exit code 0 when `error` is a closed pipe; other errors are ignored.
fn exit_on_broken_pipe(error: &io::Error) {
    if error.kind() == io::ErrorKind::BrokenPipe {
        std::process::exit(0);
    }
}
