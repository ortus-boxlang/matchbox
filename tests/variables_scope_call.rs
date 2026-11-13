//! `variables.name( args )` calls a function stored in the `variables` scope, like BoxLang.

use std::{fs, path::PathBuf, process::Command};

/// Creates a fresh project directory under the temp dir with the given `(file name, body)` files.
fn project(files: &[(&str, &str)]) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "matchbox-variables-call-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    for (name, body) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    dir
}

/// Runs the `matchbox` binary on `file` inside `dir`; returns (success, stdout, stderr).
fn run(dir: &PathBuf, file: &str) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox")).current_dir(dir).arg(file).output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn a_script_calls_a_function_stored_in_variables() {
    let dir = project(&[(
        "main.bxs",
        r#"variables.add = function( a, b ) { return a + b; };
        println( "sum=" & variables.add( 1, 2 ) );
        named = function( x, y = 5 ) { return x * y; };
        println( "named=" & variables.named( 4 ) );"#,
    )]);
    let (ok, out, err) = run(&dir, "main.bxs");
    assert!(ok, "{err}");
    assert!(out.contains("sum=3") && out.contains("named=20"), "{out}");
}

#[test]
fn a_class_calls_a_function_stored_in_its_variables_scope() {
    let dir = project(&[
        (
            "models/Holder.bx",
            r#"class Holder {
                function init() {
                    variables.callback = function( x ) { return x * 2; };
                    variables.suffix = "!";
                    return this;
                }
                function run( n ) { return variables.callback( n ) & variables.suffix; }
            }"#,
        ),
        ("main.bxs", r#"h = new models.Holder(); println( "run=" & h.run( 21 ) );"#),
    ]);
    let (ok, out, err) = run(&dir, "main.bxs");
    assert!(ok, "{err}");
    assert!(out.contains("run=42!"), "{out}");
}

#[test]
fn calling_something_that_is_not_in_variables_is_still_an_error() {
    let dir = project(&[("main.bxs", "variables.notThere( 1 );")]);
    let (ok, _out, err) = run(&dir, "main.bxs");
    assert!(!ok);
    assert!(err.to_lowercase().contains("notthere"), "{err}");
}
