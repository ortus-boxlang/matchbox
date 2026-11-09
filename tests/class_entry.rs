//! `matchbox Task.bx a b`: a class file given as the program runs its `main( args )`,
//! like `boxlang Task.bx a b` (BoxLang docs, "CLI Scripting" / "Executing Classes").

use std::{fs, path::PathBuf, process::Command};

fn project(files: &[(&str, &str)]) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "matchbox-class-entry-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    for (name, body) in files {
        fs::write(dir.join(name), body).unwrap();
    }
    dir
}

fn run(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox")).args(args).output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

const TASK: &str = r#"
class {
    function main( args = [] ) {
        println( "args=" & serializeJSON( args ) );
        println( "parsed=" & serializeJSON( cliGetArgs() ) );
    }
}
"#;

#[test]
fn main_receives_the_raw_arguments_after_the_file() {
    let dir = project(&[("Task.bx", TASK)]);
    let file = dir.join("Task.bx");
    let (ok, out, err) = run(&[file.to_str().unwrap(), "hola", "--many", "options=test"]);
    assert!(ok, "{err}");
    // docs: boxlang task.bx hola --many options=test -> [ hola, --many, options=test ]
    assert!(out.contains(r#"args=["hola","--many","options=test"]"#), "{out}");
    assert!(out.contains(r#""positionals":["hola","options=test"]"#) && out.contains(r#""many":true"#), "{out}");
}

#[test]
fn main_without_arguments_gets_an_empty_array() {
    let dir = project(&[("Task.bx", TASK)]);
    let (ok, out, err) = run(&[dir.join("Task.bx").to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("args=[]"), "{out}");
}

#[test]
fn a_named_class_and_an_instance_main_with_state_work() {
    let dir = project(&[(
        "Counter.bx",
        r#"class Counter {
            property n;
            function init() { this.n = 41; return this; }
            function main( args ) { println( "n=" & ( this.n + 1 ) ); }
        }"#,
    )]);
    let (ok, out, err) = run(&[dir.join("Counter.bx").to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("n=42"), "{out}");
}

#[test]
fn a_class_without_main_fails_with_the_boxlang_message() {
    let dir = project(&[("Plain.bx", "class { function other() {} }")]);
    let (ok, _, err) = run(&[dir.join("Plain.bx").to_str().unwrap()]);
    assert!(!ok);
    assert!(err.contains("does not have a main method to execute"), "{err}");
}

#[test]
fn a_native_binary_runs_main_with_its_arguments() {
    let dir = project(&[("Task.bx", TASK)]);
    let out_bin = dir.join("task");
    let (ok, _, err) = run(&[
        "--target",
        "native",
        "--output",
        out_bin.to_str().unwrap(),
        dir.join("Task.bx").to_str().unwrap(),
    ]);
    assert!(ok, "{err}");
    let output = Command::new(&out_bin).args(["one", "two"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains(r#"args=["one","two"]"#), "{stdout}");
}
