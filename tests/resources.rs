//! Resources: `resources/` next to the program and `--resources DIR` folders, read with
//! resourceRead / resourceReadBinary / resourceExists / resourceList; read from disk when run from
//! source and packed into a native binary (which then runs from any directory without them).

use std::{fs, path::PathBuf, process::Command};

fn project(files: &[(&str, &[u8])]) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "matchbox-resources-{}-{}",
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

fn matchbox(dir: &PathBuf, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox")).current_dir(dir).args(args).output().unwrap();
    (output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned())
}

const PROGRAM: &str = r#"
    apps = jsonDeserialize( resourceRead( "catalog/apps.json" ) );
    println( "apps=" & apps.len() & " first=" & apps[1].id );
    png = resourceReadBinary( "pictures/dot.png" );
    println( "png=" & bytesLen( png ) & " binary=" & isBinary( png ) );
    println( "extra=" & resourceRead( "extra.txt" ) );
    println( "exists=" & resourceExists( "catalog/apps.json" ) & "," & resourceExists( "nope.txt" ) );
    println( "all=" & resourceList().toList( "," ) );
    println( "catalog=" & resourceList( "catalog" ).toList( "," ) );
    try { resourceRead( "missing.json" ); } catch ( any e ) { println( "missing=" & e.message ); }
"#;

/// 0..=255 so a binary read is checked byte for byte
fn picture() -> Vec<u8> {
    (0..=255u8).collect()
}

fn app() -> PathBuf {
    let png = picture();
    project(&[
        ("src/main.bxs", PROGRAM.as_bytes()),
        ("src/resources/catalog/apps.json", br#"[{"id":"git"},{"id":"neovim"}]"#),
        ("src/resources/pictures/dot.png", &png),
        ("more/extra.txt", b"from a --resources folder"),
    ])
}

fn check(out: &str) {
    assert!(out.contains("apps=2 first=git"), "{out}");
    assert!(out.contains("png=256 binary=true"), "{out}");
    assert!(out.contains("extra=from a --resources folder"), "{out}");
    assert!(out.contains("exists=true,false"), "{out}");
    assert!(out.contains("all=catalog/apps.json,extra.txt,pictures/dot.png"), "{out}");
    assert!(out.contains("catalog=catalog/apps.json\n"), "{out}");
    assert!(out.contains("missing=Resource missing.json not found"), "{out}");
}

#[test]
fn run_from_source_reads_the_folders() {
    let dir = app();
    let (ok, out, err) = matchbox(&dir, &["--resources", "more", "src/main.bxs"]);
    assert!(ok, "{err}");
    check(&out);
}

#[test]
fn a_native_binary_carries_its_resources() {
    let dir = app();
    let exe = dir.join("app");
    let (ok, _, err) = matchbox(&dir, &["--resources", "more", "--target", "native", "--output", exe.to_str().unwrap(), "src/main.bxs"]);
    assert!(ok, "{err}");
    // the folders are gone: the binary must serve the files itself
    fs::remove_dir_all(dir.join("src/resources")).unwrap();
    fs::remove_dir_all(dir.join("more")).unwrap();
    let elsewhere = std::env::temp_dir();
    let output = Command::new(&exe).current_dir(&elsewhere).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let out = String::from_utf8_lossy(&output.stdout);
    check(&out);
    // packed raw, once: the picture's 256 bytes appear as they are
    let bytes = fs::read(&exe).unwrap();
    assert!(bytes.windows(256).any(|w| w == picture().as_slice()));
}

#[test]
fn file_write_of_binary_writes_the_bytes() {
    let png = picture();
    let dir = project(&[
        ("main.bxs", b"fileWrite( 'copy.png', resourceReadBinary( 'dot.png' ) );"),
        ("resources/dot.png", &png),
    ]);
    let (ok, _, err) = matchbox(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(fs::read(dir.join("copy.png")).unwrap(), png);
}

#[test]
fn the_same_path_in_two_folders_is_an_error() {
    let dir = project(&[
        ("main.bxs", b"println( resourceRead( 'a.txt' ) );"),
        ("resources/a.txt", b"one"),
        ("other/a.txt", b"two"),
    ]);
    let (ok, _, err) = matchbox(&dir, &["--resources", "other", "main.bxs"]);
    assert!(!ok);
    assert!(err.contains("a.txt is in two folders"), "{err}");
    let (ok, _, err) = matchbox(&dir, &["--resources", "other", "--target", "native", "--output", "x", "main.bxs"]);
    assert!(!ok);
    assert!(err.contains("a.txt is in two folders"), "{err}");
}

#[test]
fn bad_paths_and_text_reads_of_binary_are_errors() {
    let dir = project(&[
        ("main.bxs", b"try { resourceRead( '../main.bxs' ); } catch ( any e ) { println( e.message ); } try { resourceRead( 'bin.dat' ); } catch ( any e ) { println( e.message ); }"),
        ("resources/bin.dat", &[0xff, 0xfe, 0x00]),
    ]);
    let (ok, out, err) = matchbox(&dir, &["main.bxs"]);
    assert!(ok, "{err}");
    assert!(out.contains("may not contain .."), "{out}");
    assert!(out.contains("not UTF-8 text; read it with resourceReadBinary()"), "{out}");
}

#[test]
fn a_program_without_resources_builds_as_before() {
    let dir = project(&[("main.bxs", b"println( 'plain' ); println( resourceExists( 'x' ) );")]);
    let exe = dir.join("app");
    let (ok, _, err) = matchbox(&dir, &["--target", "native", "--output", exe.to_str().unwrap(), "main.bxs"]);
    assert!(ok, "{err}");
    let output = Command::new(&exe).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "plain\nfalse\n");
    let bytes = fs::read(&exe).unwrap();
    assert!(bytes.ends_with(b"BOXLANG\x02"), "no resources: the plain native layout");
}

#[test]
fn a_missing_resources_folder_is_a_usage_error() {
    let dir = project(&[("main.bxs", b"println( 1 );")]);
    let (ok, _, err) = matchbox(&dir, &["--resources", "nope", "main.bxs"]);
    assert!(!ok);
    assert!(err.contains("--resources nope: not a folder"), "{err}");
}
