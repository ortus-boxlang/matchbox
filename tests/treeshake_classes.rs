//! A prelude function used only inside a class file loaded with `new` must survive tree-shaking.

use std::{fs, process::Command};

#[test]
fn a_prelude_function_used_only_by_a_class_file_is_kept() {
    let dir = std::env::temp_dir().join(format!("matchbox-shake-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("models")).unwrap();
    fs::write(
        dir.join("models/Sorter.bx"),
        r#"class Sorter {
            function sorted( items ) { var copy = items.map( function( i ) { return i; } ); arraySort( copy, "text" ); return copy.toList( "," ); }
        }"#,
    )
    .unwrap();
    fs::write(dir.join("main.bxs"), "import models.Sorter;\nprintln( ( new Sorter() ).sorted( [ \"b\", \"c\", \"a\" ] ) );\n").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_matchbox")).current_dir(&dir).args(args).output().unwrap()
    };
    let output = run(&["main.bxs"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("a,b,c"));

    // and in a native binary, where the program is compiled once up front
    let bin = dir.join("app");
    let built = run(&["--target", "native", "--output", bin.to_str().unwrap(), "main.bxs"]);
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let output = Command::new(&bin).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("a,b,c"));
}
