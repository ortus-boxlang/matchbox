//! `expandPath` as BoxLang resolves it: relative to the calling template, through `boxlang.json` and
//! `Application.bx` mappings, and from the root (the working directory) otherwise.

use std::{fs, path::PathBuf, process::Command};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("matchbox-expandpath-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&dir);
    for (file, body) in files {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    fs::canonicalize(&dir).unwrap()
}

fn run_in(dir: &PathBuf, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox")).current_dir(dir).args(args).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().replace('\\', "/")
}

/// Formats expected paths without Windows canonicalization prefixes.
///
/// # Examples
/// ```ignore
/// assert_eq!(text(PathBuf::from(r"\\?\C:\work\file")), "C:/work/file");
/// ```
fn text(path: PathBuf) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = path.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else {
        path.strip_prefix("//?/").unwrap_or(&path).to_string()
    }
}

/// Checks expected-path formatting independently of the host operating system.
#[test]
fn expected_paths_omit_windows_verbatim_prefixes() {
    for (path, expected) in [
        (r"\\?\C:\work\file", "C:/work/file"),
        (r"\\?\UNC\server\share\file", "//server/share/file"),
        (r"C:\work\file", "C:/work/file"),
        (r"\\server\share\file", "//server/share/file"),
        ("/work/file", "/work/file"),
    ] {
        assert_eq!(text(PathBuf::from(path)), expected);
    }
}

#[test]
fn relative_paths_start_in_the_templates_folder() {
    let dir = project("relative", &[("views/page.bxs", "println( expandPath( 'data.toml' ) );\nprintln( expandPath( '../up.toml' ) );")]);
    let out = run_in(&dir, &["views/page.bxs"]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], text(dir.join("views/data.toml")));
    assert_eq!(lines[1], text(dir.join("up.toml")));
}

#[test]
fn a_rooted_path_starts_at_the_working_directory() {
    let dir = project("root", &[("views/page.bxs", "println( expandPath( '/data.toml' ) );")]);
    assert_eq!(run_in(&dir, &["views/page.bxs"]), text(dir.join("data.toml")));
}

#[test]
fn boxlang_json_mappings_apply_and_the_longest_wins() {
    let dir = project(
        "json",
        &[
            ("boxlang.json", r#"{ "mappings": { "/shared": "${user-dir}/lib", "/shared/deep": "${user-dir}/other" } }"#),
            ("main.bxs", "println( expandPath( '/shared/a.toml' ) );\nprintln( expandPath( '/SHARED/deep/b.toml' ) );\nprintln( expandPath( '/sharedfoo/c.toml' ) );"),
        ],
    );
    let out = run_in(&dir, &["main.bxs"]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], text(dir.join("lib/a.toml")));
    assert_eq!(lines[1], text(dir.join("other/b.toml")));
    assert_eq!(lines[2], text(dir.join("sharedfoo/c.toml")));
}

#[test]
fn application_bx_mappings_apply() {
    let dir = project(
        "application",
        &[
            ("Application.bx", r#"class { this.mappings = { "/shared": getDirectoryFromPath( getCurrentTemplatePath() ) & "lib" }; }"#),
            ("main.bxs", "println( expandPath( '/shared/a.toml' ) );"),
        ],
    );
    assert_eq!(run_in(&dir, &["main.bxs"]), text(dir.join("lib/a.toml")));
}

#[test]
fn a_trailing_slash_is_kept() {
    let dir = project("trailing", &[("main.bxs", "println( expandPath( '/sub/' ) );")]);
    assert_eq!(run_in(&dir, &["main.bxs"]), format!("{}/", text(dir.join("sub"))));
}
