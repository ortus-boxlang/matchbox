//! `matchbox file.bxs ...`: options only count before the file, everything after is the script's.

use std::{fs, path::PathBuf, process::Command};

fn script(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("matchbox-cli-args-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, "println( serializeJSON( cliGetArgs() ) );\n").unwrap();
    path
}

fn run(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox")).args(args).output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn script_arguments_are_not_matchbox_options() {
    let file = script("args.bxs");
    let file = file.to_str().unwrap();
    let (ok, out, _) = run(&[file, "-v", "--build", "-h", "--eval", "-e"]);
    assert!(ok);
    assert!(!out.contains("matchbox version"), "{out}");
    assert!(out.contains(r#""v":true"#) && out.contains(r#""build":true"#) && out.contains(r#""h":true"#), "{out}");
    assert!(out.contains(r#""eval":true"#) && out.contains(r#""e":true"#), "{out}");
    assert!(!PathBuf::from(file).with_extension("bxb").exists());
}

#[test]
fn cli_get_args_matches_the_boxlang_cli_scripting_docs() {
    // docs: `boxlang hello.bxs hola luis=majano --test`
    let file = script("docs.bxs");
    let (ok, out, _) = run(&[file.to_str().unwrap(), "hola", "luis=majano", "--test"]);
    assert!(ok);
    assert!(out.contains(r#""positionals":["hola","luis=majano"]"#), "{out}");
    assert!(out.contains(r#""options":{"test":true}"#), "{out}");
}

#[test]
fn option_values_need_equals_and_formats_follow_boxlang() {
    let file = script("formats.bxs");
    let (ok, out, _) = run(&[
        file.to_str().unwrap(), "--name", "luis", "--debug", "--!verbose", "--no-color", "--config=prod.json",
        "--message=\"Hello World\"", "-o='/path/to/file'", "-abc", "my/path/template", "--", "last",
    ]);
    assert!(ok);
    assert!(out.contains(r#""name":true"#), "--name does not take the next token: {out}");
    assert!(out.contains(r#""debug":true"#) && out.contains(r#""verbose":false"#) && out.contains(r#""color":false"#), "{out}");
    assert!(out.contains(r#""config":"prod.json""#) && out.contains(r#""message":"Hello World""#), "{out}");
    assert!(out.contains(r#""o":"/path/to/file""#), "{out}");
    assert!(out.contains(r#""a":true"#) && out.contains(r#""b":true"#) && out.contains(r#""c":true"#), "{out}");
    assert!(out.contains(r#""positionals":["luis","my/path/template","last"]"#), "{out}");
}

#[test]
fn unknown_options_before_the_file_do_not_run_the_script_like_boxlang() {
    // BoxLang treats the first unrecognized token as the target; a missing target is an error.
    let file = script("unknown.bxs");
    let (ok, out, _) = run(&["--bogus", file.to_str().unwrap()]);
    assert!(!ok);
    assert!(out.is_empty(), "the script must not run: {out}");
}

#[test]
fn help_and_version_are_global_only_as_the_sole_argument() {
    let (ok, out, _) = run(&["--help"]);
    assert!(ok && out.contains("Usage: matchbox"));
    let (ok, out, _) = run(&["-h"]);
    assert!(ok && out.contains("Usage: matchbox"));
    let (ok, out, _) = run(&["-v"]);
    assert!(ok && out.contains("matchbox version"));
    let (ok, out, _) = run(&["--version"]);
    assert!(ok && out.contains("matchbox version"));
    // a script and its own -h / -v
    let file = script("sole.bxs");
    let (ok, out, _) = run(&[file.to_str().unwrap(), "-h"]);
    assert!(ok && !out.contains("Usage: matchbox") && out.contains(r#""h":true"#), "{out}");
}

#[test]
fn bx_flags_are_honored_anywhere_and_not_passed_to_the_script() {
    let file = script("bx.bxs");
    let (ok, out, err) = run(&[file.to_str().unwrap(), "one", "--bx-config=/x.json", "--bx-debug", "two"]);
    assert!(ok, "{err}");
    assert!(out.contains(r#""positionals":["one","two"]"#) && out.contains(r#""options":{}"#), "{out}");
    assert!(err.contains("--bx-config /x.json is not supported"), "{err}");
}

#[test]
fn bx_code_runs_inline_code() {
    let (ok, out, _) = run(&["--bx-code", "println( 'Hello BoxLang!' ); println( 1 + 2 );"]);
    assert!(ok);
    assert!(out.contains("Hello BoxLang!") && out.contains('3'), "{out}");
    let (ok, _, err) = run(&["--bx-code"]);
    assert!(!ok && err.contains("Missing inline code to execute with --bx-code flag."), "{err}");
}

#[test]
fn bx_print_ast_prints_json_for_inline_code_and_files() {
    let (ok, out, _) = run(&["--bx-printAST", "--bx-code", "x = 1 + 2"]);
    assert!(ok && out.contains(r#""ASTType": "BoxScript""#), "{out}");
    let file = script("ast.bxs");
    let (ok, out, _) = run(&["--bx-printAST", file.to_str().unwrap()]);
    assert!(ok && out.contains(r#""ASTType": "BoxScript""#), "{out}");
}

#[test]
fn bx_transpile_is_rejected_clearly() {
    let file = script("transpile.bxs");
    let (ok, out, err) = run(&["--bx-transpile", file.to_str().unwrap()]);
    assert!(!ok && out.is_empty());
    assert!(err.contains("--bx-transpile is not supported"), "{err}");
}

#[test]
fn server_cli_exposes_the_command_line_like_boxlang() {
    let dir = std::env::temp_dir().join(format!("matchbox-server-cli-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("server_cli.bxs");
    fs::write(
        &file,
        "println( serializeJSON( server.cli.args ) );\nprintln( serializeJSON( server.cli.parsed ) );\nprintln( server.cli.executionPath );\nprintln( server.cli.command );\nprintln( server.boxlang.cliMode );\n",
    )
    .unwrap();
    let (ok, out, _) = run(&[file.to_str().unwrap(), "hola", "luis=majano", "--test"]);
    assert!(ok);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], r#"["hola","luis=majano","--test"]"#);
    assert_eq!(lines[1], r#"{"options":{"test":true},"positionals":["hola","luis=majano"]}"#);
    assert!(!lines[2].is_empty());
    assert!(lines[3].ends_with("hola luis=majano --test"), "{}", lines[3]);
    assert_eq!(lines[4], "true");
}
