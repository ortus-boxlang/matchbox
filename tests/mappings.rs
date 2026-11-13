//! Mappings: `/models` -> a directory, declared in `this.mappings` of an Application.bx or in the
//! `mappings` of a boxlang.json (the same places as BoxLang on the JVM). Classes found through a
//! mapping are compiled into the program, so native binaries carry them.

use std::{fs, path::PathBuf, process::Command};

/// Creates a fresh project directory under the temp dir with the given `(relative path, body)` files.
fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("matchbox-mappings-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&dir);
    for (file, body) in files {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    dir
}

/// Runs the `matchbox` binary with `dir` as the working directory; returns (success, stdout, stderr).
fn run_in(dir: &PathBuf, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

const GREETER: &str = r#"class Greeter { function hi() { return "hi from the mapping"; } }"#;
const MAIN: &str = r#"import shared.Greeter;
println( ( new shared.Greeter() ).hi() );
println( ( new Greeter() ).hi() );
"#;

#[test]
fn application_bx_mappings_resolve_new_and_import() {
    let dir = project(
        "app",
        &[
            ("lib/Greeter.bx", GREETER),
            (
                "Application.bx",
                r#"class {
                    this.name = "t";
                    this.mappings = { "/shared": getDirectoryFromPath( getCurrentTemplatePath() ) & "lib" };
                }"#,
            ),
            ("main.bxs", MAIN),
        ],
    );
    let (ok, out, err) = run_in(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(out.matches("hi from the mapping").count(), 2, "{out}");
}

#[test]
fn struct_form_with_external_and_a_literal_struct() {
    let dir = project(
        "struct",
        &[
            ("lib/Greeter.bx", GREETER),
            (
                "Application.bx",
                r#"class {
                    this.mappings = { "/shared": { path: getDirectoryFromPath( getCurrentTemplatePath() ) & "lib", external: false } };
                }"#,
            ),
            ("main.bxs", MAIN),
        ],
    );
    let (ok, out, err) = run_in(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert!(out.contains("hi from the mapping"), "{out}");
}

#[test]
fn boxlang_json_mappings_with_user_dir() {
    let dir = project(
        "json",
        &[
            ("elsewhere/Greeter.bx", GREETER),
            ("boxlang.json", r#"{ "mappings": { "shared": "${user-dir}/elsewhere" } }"#),
            ("main.bxs", MAIN),
        ],
    );
    let (ok, out, err) = run_in(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(out.matches("hi from the mapping").count(), 2, "{out}");
}

#[test]
fn application_mappings_override_global_ones() {
    let dir = project(
        "override",
        &[
            ("global/Greeter.bx", r#"class Greeter { function hi() { return "GLOBAL"; } }"#),
            ("app/Greeter.bx", r#"class Greeter { function hi() { return "APP"; } }"#),
            ("boxlang.json", r#"{ "mappings": { "/shared": "${user-dir}/global" } }"#),
            (
                "Application.bx",
                r#"class { this.mappings = { "/shared": getDirectoryFromPath( getCurrentTemplatePath() ) & "app" }; }"#,
            ),
            ("main.bxs", "import shared.Greeter;\nprintln( ( new Greeter() ).hi() );\n"),
        ],
    );
    let (ok, out, err) = run_in(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert!(out.contains("APP") && !out.contains("GLOBAL"), "{out}");
}

#[test]
fn a_native_binary_carries_the_mapped_classes() {
    let dir = project(
        "native",
        &[
            ("lib/Greeter.bx", GREETER),
            (
                "Application.bx",
                r#"class { this.mappings = { "/shared": getDirectoryFromPath( getCurrentTemplatePath() ) & "lib" }; }"#,
            ),
            ("main.bxs", MAIN),
        ],
    );
    let bin = dir.join("app");
    let (ok, _, err) = run_in(&dir, &["--target", "native", "--output", bin.to_str().unwrap(), "main.bxs"]);
    assert!(ok, "{err}");
    // the program does not need the project any more
    fs::remove_dir_all(dir.join("lib")).unwrap();
    let elsewhere = std::env::temp_dir();
    let output = Command::new(&bin).current_dir(elsewhere).output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(out.matches("hi from the mapping").count(), 2, "{out}");
}

#[test]
fn mappings_work_when_building_from_another_directory() {
    let dir = project(
        "elsewhere",
        &[
            ("lib/Greeter.bx", GREETER),
            (
                "Application.bx",
                r#"class { this.mappings = { "/shared": getDirectoryFromPath( getCurrentTemplatePath() ) & "lib" }; }"#,
            ),
            ("main.bxs", MAIN),
        ],
    );
    let other = std::env::temp_dir();
    let main = dir.join("main.bxs");
    let (ok, out, err) = run_in(&other, &[main.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("hi from the mapping"), "{out}");
}

#[test]
fn an_unknown_class_through_a_missing_mapping_still_fails_clearly() {
    let dir = project("missing", &[("main.bxs", "import nowhere.Thing;\nprintln( ( new Thing() ).hi() );\n")]);
    let (ok, _, err) = run_in(&dir, &["main.bxs"]);
    assert!(!ok);
    assert!(err.contains("Source file not found"), "{err}");
}

#[test]
fn global_mappings_are_available_while_the_application_bx_is_read() {
    // the Application.bx pseudo-constructor itself uses a class that only boxlang.json maps
    let dir = project(
        "global-first",
        &[
            ("elsewhere/Greeter.bx", GREETER),
            ("boxlang.json", r#"{ "mappings": { "/shared": "${user-dir}/elsewhere" } }"#),
            (
                "Application.bx",
                r#"class { this.greeting = ( new shared.Greeter() ).hi(); this.mappings = { "/app": "." }; }"#,
            ),
            ("main.bxs", "println( \"ok\" );"),
        ],
    );
    let (ok, out, err) = run_in(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert!(out.contains("ok"), "{out}");
}

#[test]
fn a_relative_mapping_path_in_application_bx_resolves_beside_the_file() {
    let dir = project(
        "relative-path",
        &[
            ("lib/Greeter.bx", GREETER),
            ("Application.bx", r#"class { this.mappings = { "/shared": "./lib" }; }"#),
            ("main.bxs", MAIN),
        ],
    );
    let main = dir.join("main.bxs");
    // built and run from another directory with an absolute entry path
    let (ok, out, err) = run_in(&std::env::temp_dir(), &[main.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("hi from the mapping"), "{out}");
}

#[test]
fn a_native_binary_reports_the_template_path_of_its_source_wherever_it_runs() {
    let dir = project(
        "template-path",
        &[("main.bxs", "println( \"path=\" & getCurrentTemplatePath() );")],
    );
    let bin = dir.join("app");
    // built from a relative path, run from another directory
    let (ok, _, err) = run_in(&dir, &["--target", "native", "--output", bin.to_str().unwrap(), "main.bxs"]);
    assert!(ok, "{err}");
    let output = Command::new(&bin).current_dir(std::env::temp_dir()).output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let reported = PathBuf::from(out.trim().strip_prefix("path=").expect("template path output"));
    assert!(reported.is_absolute(), "{out}");
    assert_eq!(reported.canonicalize().unwrap(), dir.join("main.bxs").canonicalize().unwrap(), "{out}");
}
