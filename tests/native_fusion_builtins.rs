//! A Native Fusion program gets the same built-ins as a regular native build, and its command line:
//! `CLIGetArgs()` and file I/O work next to a Rust function in one binary.

use std::{fs, path::PathBuf, process::Command};

/// Creates a Fusion project (a script plus a `native/` Rust function) in a fresh temp directory.
fn fusion_project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("matchbox-fusion-builtins-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("native")).unwrap();
    fs::write(
        dir.join("native/shout.rs"),
        r#"use matchbox_vm::types::{BxNativeFunction, BxVM, BxValue};
use std::collections::HashMap;

pub fn shout(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let text = vm.to_string(args[0]);
    Ok(BxValue::new_ptr(vm.string_new(text.to_uppercase())))
}

pub fn register_bifs() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("shout".to_string(), shout as BxNativeFunction);
    map
}
"#,
    )
    .unwrap();
    fs::write(
        dir.join("app.bxs"),
        "args = CLIGetArgs();\nprintln( \"first=\" & shout( args.positionals[ 1 ] ) );\nprintln( \"file=\" & fileRead( args.positionals[ 2 ] ) );\nprintln( \"count=\" & args.positionals.len() );\n",
    )
    .unwrap();
    fs::write(dir.join("data.txt"), "from disk").unwrap();
    dir
}

#[test]
fn a_fusion_binary_reads_its_arguments_and_files_and_calls_rust() {
    let dir = fusion_project();
    let binary = dir.join("app");
    let build = Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .current_dir(&dir)
        .args(["--target", "native", "--output", binary.to_str().unwrap(), "app.bxs"])
        .output()
        .unwrap();
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&binary)
        .current_dir(std::env::temp_dir())
        .args(["hello", dir.join("data.txt").to_str().unwrap()])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(out.contains("first=HELLO"), "{out}");
    assert!(out.contains("file=from disk"), "{out}");
    assert!(out.contains("count=2"), "{out}");
}
