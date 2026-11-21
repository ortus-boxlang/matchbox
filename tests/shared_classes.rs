//! A class file used from many places is compiled once and stored once: every `new`, `extends`
//! and nested use shares one compiled class (types::shared), in `--build` bytecode and in a
//! native binary alike, and the program behaves as before.

use std::{fs, path::PathBuf, process::Command};

/// Writes a fresh scratch project with the given source files and returns its directory.
fn project(files: &[(&str, &str)]) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "matchbox-shared-classes-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    for (name, body) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    dir
}

/// Runs the MatchBox binary in `dir`; returns success, stdout and stderr.
fn matchbox(dir: &PathBuf, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox")).current_dir(dir).args(args).output().unwrap();
    (output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned())
}

/// Counts occurrences of a class literal in compiled bytecode or a native binary.
fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack.windows(needle.len()).filter(|w| *w == needle).count()
}

/// `lib.Shared` is used by A and B (twice each), by `lib.Child` through `extends`, and by main.
/// Before sharing, its literal landed in the bytecode once per use, nested uses included.
fn diamond() -> PathBuf {
    project(&[
        ("lib/Shared.bx", r#"class {
            function init() { variables.mark = "SHARED-CLASS-LITERAL-7f3a"; return this; }
            function mark() { return variables.mark; }
        }"#),
        ("lib/Base.bx", r#"class {
            function base() { return "BASE-CLASS-LITERAL-91c2:" & new lib.Shared().mark(); }
        }"#),
        ("lib/Child.bx", r#"class extends="lib.Base" {
            function child() { return "child+" & base(); }
        }"#),
        ("lib/A.bx", r#"class {
            function one() { return "a1:" & new lib.Shared().mark(); }
            function two() { return "a2:" & new lib.Shared().mark(); }
            function child() { return new lib.Child().child(); }
        }"#),
        ("lib/B.bx", r#"class {
            function one() { return "b1:" & new lib.Shared().mark() & "/" & new lib.A().one(); }
            function two() { return "b2:" & new lib.Child().base(); }
        }"#),
        ("main.bxs", r#"
            println( new lib.A().one() );
            println( new lib.A().two() );
            println( new lib.A().child() );
            println( new lib.B().one() );
            println( new lib.B().two() );
            println( new lib.Shared().mark() );
        "#),
    ])
}

const EXPECTED: &str = "a1:SHARED-CLASS-LITERAL-7f3a\na2:SHARED-CLASS-LITERAL-7f3a\nchild+BASE-CLASS-LITERAL-91c2:SHARED-CLASS-LITERAL-7f3a\nb1:SHARED-CLASS-LITERAL-7f3a/a1:SHARED-CLASS-LITERAL-7f3a\nb2:BASE-CLASS-LITERAL-91c2:SHARED-CLASS-LITERAL-7f3a\nSHARED-CLASS-LITERAL-7f3a\n";

/// Sharing does not change what the program prints.
#[test]
fn a_class_used_many_times_runs_the_same() {
    let dir = diamond();
    let (ok, out, err) = matchbox(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(out, EXPECTED);
}

/// Bytecode keeps one copy of each class and still runs from the `.bxb` file.
#[test]
fn bytecode_holds_each_class_once_and_runs() {
    let dir = diamond();
    let (ok, _, err) = matchbox(&dir, &["--build", "main.bxs"]);
    assert!(ok, "{err}");
    let bxb = dir.join("main.bxb");
    let bytes = fs::read(&bxb).unwrap();
    assert_eq!(count(&bytes, b"SHARED-CLASS-LITERAL-7f3a"), 1, "lib.Shared is stored once");
    assert_eq!(count(&bytes, b"BASE-CLASS-LITERAL-91c2"), 1, "lib.Base (a parent and a class of its own) is stored once");
    let (ok, out, err) = matchbox(&dir, &["main.bxb"]);
    assert!(ok, "{err}");
    assert_eq!(out, EXPECTED);
}

/// A native binary keeps one copy of each class and still runs on its own.
#[test]
fn a_native_binary_holds_each_class_once_and_runs() {
    let dir = diamond();
    let exe = dir.join("app");
    let (ok, _, err) = matchbox(&dir, &["--target", "native", "--output", exe.to_str().unwrap(), "main.bxs"]);
    assert!(ok, "{err}");
    let bytes = fs::read(&exe).unwrap();
    assert_eq!(count(&bytes, b"SHARED-CLASS-LITERAL-7f3a"), 1);
    let output = Command::new(&exe).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), EXPECTED);
}

/// Stripping source text must not undo the sharing of class constants.
#[test]
fn strip_source_keeps_classes_shared() {
    let dir = diamond();
    let (ok, _, err) = matchbox(&dir, &["--build", "--strip-source", "main.bxs"]);
    assert!(ok, "{err}");
    let bytes = fs::read(dir.join("main.bxb")).unwrap();
    assert_eq!(count(&bytes, b"SHARED-CLASS-LITERAL-7f3a"), 1);
    let (ok, out, err) = matchbox(&dir, &["main.bxb"]);
    assert!(ok, "{err}");
    assert_eq!(out, EXPECTED);
}

/// A class that constructs itself during compilation reports a cycle, not a stack overflow.
#[test]
fn a_class_file_that_needs_itself_is_a_clear_error() {
    let dir = project(&[
        ("lib/Loop.bx", r#"class { function again() { return new lib.Loop(); } }"#),
        ("main.bxs", "println( new lib.Loop() );"),
    ]);
    let (ok, _, err) = matchbox(&dir, &["main.bxs"]);
    assert!(!ok);
    assert!(err.contains("Circular class reference") && err.contains("Loop.bx"), "{err}");
}

/// Caching is per compile: a new MatchBox process sees an edited class file.
#[test]
fn an_edited_class_file_is_read_again_by_the_next_compile() {
    let dir = project(&[
        ("lib/V.bx", r#"class { function v() { return "one"; } }"#),
        ("main.bxs", "println( new lib.V().v() );"),
    ]);
    let (_, out, _) = matchbox(&dir, &["main.bxs"]);
    assert_eq!(out.trim(), "one");
    fs::write(dir.join("lib/V.bx"), r#"class { function v() { return "two"; } }"#).unwrap();
    let (_, out, _) = matchbox(&dir, &["main.bxs"]);
    assert_eq!(out.trim(), "two");
}
