//! Output to a closed pipe must end the program quietly, not panic with
//! "failed printing to stdout: Broken pipe" (`matchbox script.bxs | head -1`, `./app | grep -q x`).

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// A program that prints far more than a pipe buffer holds, so the reader closing early is always noticed.
const FLOOD: &str = r#"
for ( i = 1; i <= 200000; i++ ) {
    println( "line " & i );
}
println( "done" );
"#;

/// Creates a fresh directory under the temp dir with `FLOOD` saved as `flood.bxs`.
fn project() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "matchbox-broken-pipe-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("flood.bxs"), FLOOD).unwrap();
    dir
}

/// Starts `program args`, reads only the first bytes of stdout, closes the pipe and returns
/// (exit success, first bytes read, stderr).
fn read_a_little_then_close(program: &Path, args: &[&str]) -> (bool, String, String) {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0u8; 16];
    let n = stdout.read(&mut first).unwrap();
    drop(stdout); // the reader goes away while the program is still writing
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&first[..n]).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn the_interpreter_ends_quietly_when_stdout_is_closed() {
    let dir = project();
    let script = dir.join("flood.bxs");
    let (ok, first, err) = read_a_little_then_close(Path::new(env!("CARGO_BIN_EXE_matchbox")), &[script.to_str().unwrap()]);
    assert!(first.starts_with("line "), "{first}");
    assert!(!err.contains("panicked") && !err.contains("Broken pipe"), "{err}");
    assert!(ok, "exit status should be 0, stderr: {err}");
}

#[test]
fn a_native_binary_ends_quietly_when_stdout_is_closed() {
    let dir = project();
    let script = dir.join("flood.bxs");
    let binary = dir.join("flood");
    let build = Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .args(["--target", "native", "--output", binary.to_str().unwrap(), script.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let (ok, first, err) = read_a_little_then_close(&binary, &[]);
    assert!(first.starts_with("line "), "{first}");
    assert!(!err.contains("panicked") && !err.contains("Broken pipe"), "{err}");
    assert!(ok, "exit status should be 0, stderr: {err}");
}

#[test]
fn the_output_is_complete_when_nobody_closes_the_pipe() {
    let dir = project();
    let script = dir.join("flood.bxs");
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox")).arg(script.to_str().unwrap()).output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(text.starts_with("line 1\n") && text.trim_end().ends_with("done"), "{}", &text[..text.len().min(80)]);
}
