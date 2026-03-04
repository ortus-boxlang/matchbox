use std::process::Command;
use std::fs;
use std::path::PathBuf;

#[test]
fn run_all_boxlang_scripts() {
    let mut scripts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    scripts_dir.push("tests/scripts");

    let entries = fs::read_dir(scripts_dir).expect("Failed to read tests/scripts directory");

    let mut failed_scripts = Vec::new();

    for entry in entries {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("bxs") {
            println!("Running test script: {:?}", path.file_name().unwrap());
            
            let output = Command::new("cargo")
                .arg("run")
                .arg("--quiet")
                .arg("--")
                .arg(&path)
                .output()
                .expect("Failed to execute cargo run");

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                failed_scripts.push(format!(
                    "Script {:?} failed with exit code {:?}
Stderr: {}",
                    path.file_name().unwrap(),
                    output.status.code(),
                    stderr
                ));
            }
        }
    }

    if !failed_scripts.is_empty() {
        panic!("The following test scripts failed:

{}", failed_scripts.join("
---
"));
    }
}
