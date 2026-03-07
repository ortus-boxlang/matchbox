use std::process::Command;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // 1. Get Git commit hash
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // 2. Get build date
    let date = Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_COMMIT={}", commit);
    println!("cargo:rustc-env=BUILD_DATE={}", date);

    // 3. Build the runner stub (Independent of Workspace)
    let root_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let runner_dir = Path::new(&root_dir).join("crates/matchbox-runner");
    let stub_dest_dir = Path::new(&root_dir).join("stubs");
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not found");
    let stub_target_dir = Path::new(&out_dir).join("runner_target");

    // Ensure stubs directory exists
    if !stub_dest_dir.exists() {
        fs::create_dir_all(&stub_dest_dir).expect("Failed to create stubs directory");
    }

    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    
    println!("cargo:rerun-if-changed=crates/matchbox-runner/src/main.rs");
    println!("cargo:rerun-if-changed=crates/matchbox-runner/Cargo.toml");

    // Crucial: We build the runner in its own directory with its own target folder
    // to bypass the workspace lock.
    let status = Command::new(&cargo)
        .arg("build")
        .arg("--release")
        .current_dir(&runner_dir)
        .env("CARGO_TARGET_DIR", &stub_target_dir)
        .status()
        .expect("Failed to build matchbox-runner stub");

    if status.success() {
        let stub_name = if cfg!(windows) { "matchbox_runner.exe" } else { "matchbox_runner" };
        let stub_src = stub_target_dir.join("release").join(stub_name);
        let stub_dest = stub_dest_dir.join("runner_stub_native");
        
        fs::copy(&stub_src, &stub_dest).expect("Failed to copy runner stub to stubs directory");
        println!("cargo:warning=Runner stub built and copied to {}", stub_dest.display());
    } else {
        println!("cargo:warning=Failed to build matchbox-runner stub. Standalone native binary production may fail.");
    }
}
