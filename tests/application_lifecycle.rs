//! Application.bx for scripts and classes, like `boxlang` runs them: the listener around the
//! program (docs: Application.bx, "Multi-Runtime Support"; CLI Scripting).

use std::{fs, path::PathBuf, process::Command};

/// Creates a fresh project directory under the temp dir with the given `(file name, body)` files.
fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("matchbox-lifecycle-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (file, body) in files {
        fs::write(dir.join(file), body).unwrap();
    }
    dir
}

/// Runs the `matchbox` binary with `dir` as the working directory; returns (success, stdout, stderr).
fn run(dir: &PathBuf, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox")).current_dir(dir).args(args).output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The non-empty, trimmed lines of a program's output.
fn lines(out: &str) -> Vec<&str> {
    out.lines().map(str::trim).filter(|l| !l.is_empty()).collect()
}

const FULL: &str = r#"class {
    this.name = "demo";
    function onApplicationStart() { application.greeting = "hello"; println( "app start" ); return true; }
    function onRequestStart( targetPage ) { println( "request start " & listLast( targetPage, "/" ) ); return true; }
    function onRequestEnd() { println( "request end" ); }
    function onApplicationEnd( applicationScope ) { println( "app end " & applicationScope.greeting ); }
}"#;

#[test]
fn hooks_run_in_the_documented_order_around_a_script() {
    let dir = project("order", &[("Application.bx", FULL), ("main.bxs", "println( \"script \" & application.greeting );")]);
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(
        lines(&out),
        ["app start", "request start main.bxs", "script hello", "request end", "app end hello"],
        "{out}"
    );
}

#[test]
fn an_unnamed_application_skips_the_application_hooks_only() {
    let app = r#"class {
        function onApplicationStart() { println( "app start" ); }
        function onRequestStart( targetPage ) { println( "request start" ); return true; }
        function onRequestEnd() { println( "request end" ); }
    }"#;
    let dir = project("unnamed", &[("Application.bx", app), ("main.bxs", "println( \"script\" );")]);
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(lines(&out), ["request start", "script", "request end"], "{out}");
}

#[test]
fn on_request_start_returning_false_stops_the_script_but_not_the_cleanup() {
    let app = r#"class {
        function onRequestStart( targetPage ) { println( "blocked" ); return false; }
        function onRequestEnd() { println( "request end" ); }
    }"#;
    let dir = project("false", &[("Application.bx", app), ("main.bxs", "println( \"script\" );")]);
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(lines(&out), ["blocked", "request end"], "{out}");
}

#[test]
fn on_error_that_returns_true_handles_the_error() {
    let app = r#"class {
        function onError( exception, eventName ) { println( "handled: " & exception.message ); return true; }
        function onRequestEnd() { println( "request end" ); }
    }"#;
    let dir = project("handled", &[("Application.bx", app), ("main.bxs", "throw( message = \"boom\" );")]);
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(lines(&out), ["handled: boom", "request end"], "{out}");
}

#[test]
fn an_error_without_a_handler_still_fails_and_runs_request_end() {
    let app = r#"class { function onRequestEnd() { println( "request end" ); } }"#;
    let dir = project("unhandled", &[("Application.bx", app), ("main.bxs", "throw( message = \"boom\" );")]);
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(!ok);
    assert!(err.contains("boom"), "{err}");
    assert!(out.contains("request end"), "{out}");
}

#[test]
fn init_is_not_called_on_the_application_class() {
    let app = r#"class { this.name = "x"; function init() { println( "INIT CALLED" ); return this; } }"#;
    let dir = project("init", &[("Application.bx", app), ("main.bxs", "println( \"script\" );")]);
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert!(!out.contains("INIT CALLED"), "{out}");
}

#[test]
fn declarations_in_the_script_are_visible_to_the_hooks() {
    let app = r#"class { function onRequestEnd() { println( "end " & helper() ); } }"#;
    let dir = project(
        "decl",
        &[("Application.bx", app), ("main.bxs", "function helper() { return \"ok\"; }\nprintln( \"script\" );")],
    );
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(lines(&out), ["script", "end ok"], "{out}");
}

#[test]
fn a_class_with_main_runs_inside_the_lifecycle() {
    let dir = project(
        "main",
        &[
            ("Application.bx", FULL),
            ("Task.bx", "class { function main( args ) { println( \"main \" & args.len() & \" \" & application.greeting ); } }"),
        ],
    );
    let (ok, out, err) = run(&dir, &["Task.bx", "a", "b"]);
    assert!(ok, "{err}");
    assert_eq!(
        lines(&out),
        ["app start", "request start Task.bx", "main 2 hello", "request end", "app end hello"],
        "{out}"
    );
}

#[test]
fn a_program_without_an_application_bx_is_unchanged() {
    let dir = project("none", &[("main.bxs", "println( \"script\" );")]);
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(lines(&out), ["script"]);
}

#[test]
fn a_native_binary_runs_the_lifecycle() {
    let dir = project("native", &[("Application.bx", FULL), ("main.bxs", "println( \"script \" & application.greeting );")]);
    let bin = dir.join("app");
    let (ok, _, err) = run(&dir, &["--target", "native", "--output", bin.to_str().unwrap(), "main.bxs"]);
    assert!(ok, "{err}");
    fs::remove_file(dir.join("Application.bx")).unwrap();
    let output = Command::new(&bin).current_dir(std::env::temp_dir()).output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        lines(&out),
        ["app start", "request start main.bxs", "script hello", "request end", "app end hello"],
        "{out}"
    );
}

#[test]
fn script_level_var_stays_visible_to_functions_and_hooks() {
    let app = r#"class { this.name = "x"; function onRequestEnd() { println( "end sees " & answer ); } }"#;
    let dir = project(
        "var",
        &[
            ("Application.bx", app),
            ("main.bxs", "var answer = 42;\nfunction readAnswer() { return answer; }\nprintln( \"read \" & readAnswer() );"),
        ],
    );
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(lines(&out), ["read 42", "end sees 42"], "{out}");
}

#[test]
fn running_the_application_bx_itself_does_not_wrap_it_in_its_own_lifecycle() {
    let dir = project("self", &[("Application.bx", FULL)]);
    // a relative path to the descriptor: same file as the descriptor next to it
    let (ok, out, err) = run(&dir, &["Application.bx"]);
    assert!(!ok);
    assert!(err.contains("does not have a main method"), "{err}");
    assert!(!out.contains("app start"), "{out}");
}

#[test]
fn an_error_in_on_application_start_reaches_on_error_and_request_end() {
    let app = r#"class {
        this.name = "x";
        function onApplicationStart() { throw( message = "cannot start" ); }
        function onError( exception, eventName ) { println( "handled: " & exception.message ); return true; }
        function onRequestEnd() { println( "request end" ); }
        function onApplicationEnd( applicationScope ) { println( "app end" ); }
    }"#;
    let dir = project("start-error", &[("Application.bx", app), ("main.bxs", "println( \"script\" );")]);
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(lines(&out), ["handled: cannot start", "request end", "app end"], "{out}");
}

#[test]
fn on_application_end_runs_when_the_error_is_not_handled() {
    let app = r#"class { this.name = "x"; function onApplicationEnd( applicationScope ) { println( "app end" ); } }"#;
    let dir = project("end-after-error", &[("Application.bx", app), ("main.bxs", "throw( message = \"boom\" );")]);
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(!ok);
    assert!(err.contains("boom"), "{err}");
    assert!(out.contains("app end"), "{out}");
}

#[test]
fn the_pseudo_constructor_runs_once_when_it_declares_no_mappings() {
    let app = r#"class { this.name = "x"; println( "created" ); }"#;
    let dir = project("once", &[("Application.bx", app), ("main.bxs", "println( \"script\" );")]);
    let (ok, out, err) = run(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(out.matches("created").count(), 1, "{out}");
}
