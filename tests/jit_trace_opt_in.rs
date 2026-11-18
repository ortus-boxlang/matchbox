//! The JIT prints `[JIT] ...` diagnostics to stderr only when `MATCHBOX_JIT_TRACE` is set.

use std::{fs, path::PathBuf, process::Command};

/// Writes a program with a hot counting loop and a hot function, so every JIT tier is exercised.
fn program() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("matchbox-jit-trace-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hot.bxs");
    fs::write(
        &path,
        "function add( a, b ) { return a + b }\n\
         total = 0\n\
         for ( i = 0; i < 20000; i++ ) { total = add( total, i ) }\n\
         items = []\n\
         for ( i = 0; i < 5000; i++ ) { items.append( i ) }\n\
         for ( x in items ) { total += x }\n\
         println( total )\n",
    )
    .unwrap();
    path
}

/// Runs the program, optionally with `MATCHBOX_JIT_TRACE=1`, on a large stack; returns (stdout, stderr).
fn run(trace: bool) -> (String, String) {
    let home = std::env::temp_dir().join(format!("matchbox-jit-trace-home-{}", std::process::id()));
    let mut command = Command::new(env!("CARGO_BIN_EXE_matchbox"));
    command
        .arg(program())
        .env("MATCHBOX_HOME", home)
        .env_remove("MATCHBOX_JIT_TRACE");
    if trace {
        command.env("MATCHBOX_JIT_TRACE", "1");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// By default a hot program writes nothing to stderr and no `[JIT]` text to stdout.
#[test]
fn jit_is_silent_by_default() {
    let (out, err) = run(false);
    assert!(err.is_empty(), "stderr must be empty, got: {err}");
    assert!(!out.contains("[JIT]"), "{out}");
    assert!(
        out.trim().ends_with(&(199990000i64 + 12497500).to_string()),
        "{out}"
    );
}

/// With `MATCHBOX_JIT_TRACE` set the trace goes to stderr only, and the program output is unchanged.
#[test]
fn jit_trace_is_opt_in_and_on_stderr() {
    let (quiet_out, _) = run(false);
    let (out, err) = run(true);
    assert!(
        err.contains("[JIT]"),
        "expected a trace on stderr, got: {err}"
    );
    assert!(!out.contains("[JIT]"), "trace leaked to stdout: {out}");
    assert_eq!(out, quiet_out);
}
