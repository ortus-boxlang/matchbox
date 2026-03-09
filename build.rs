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
    let src_dir = Path::new(&root_dir).join("src");
    let stubs_rs_path = src_dir.join("stubs.rs");

    // Ensure stubs directory exists
    if !stub_dest_dir.exists() {
        fs::create_dir_all(&stub_dest_dir).expect("Failed to create stubs directory");
    }

    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    
    println!("cargo:rerun-if-changed=crates/matchbox-runner/src/main.rs");
    println!("cargo:rerun-if-changed=crates/matchbox-runner/Cargo.toml");
    println!("cargo:rerun-if-changed=crates/matchbox-vm/src/vm/mod.rs");
    println!("cargo:rerun-if-changed=crates/matchbox-vm/src/vm/opcode.rs");
    println!("cargo:rerun-if-changed=crates/matchbox-vm/src/lib.rs");
    println!("cargo:rerun-if-changed=crates/matchbox-vm/Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");

    let mut stubs_rs_content = String::from("use std::collections::HashMap;\n\n");
    stubs_rs_content.push_str("pub fn get_stub(target: &str) -> Option<&'static [u8]> {\n");
    stubs_rs_content.push_str("    let mut stubs: HashMap<&str, &[u8]> = HashMap::new();\n");

    // Helper closure to build and copy a stub
    let build_stub = |target: Option<&str>, dest_name: &str, src_name: &str, alias: &str, stubs_rs: &mut String| {
        let dest_path = stub_dest_dir.join(dest_name);

        // Determine whether we need (re)build: missing stub, zero-length stub,
        // or any tracked source file is newer than the stub.
        let sources_to_watch = [
            Path::new(&root_dir).join("crates/matchbox-runner/src/main.rs"),
            Path::new(&root_dir).join("crates/matchbox-runner/Cargo.toml"),
            Path::new(&root_dir).join("crates/matchbox-vm/src/vm/mod.rs"),
            Path::new(&root_dir).join("crates/matchbox-vm/src/vm/opcode.rs"),
            Path::new(&root_dir).join("crates/matchbox-vm/src/lib.rs"),
            Path::new(&root_dir).join("crates/matchbox-vm/Cargo.toml"),
        ];
        let stub_mtime = dest_path.metadata()
            .and_then(|m| m.modified())
            .ok();
        let needs_rebuild = stub_mtime.map_or(true, |stub_time| {
            fs::metadata(&dest_path).map(|m| m.len() == 0).unwrap_or(true)
            || sources_to_watch.iter().any(|src| {
                src.metadata()
                    .and_then(|m| m.modified())
                    .map(|src_time| src_time > stub_time)
                    .unwrap_or(false)
            })
        });

        if needs_rebuild {
            let mut cmd = Command::new(&cargo);
            cmd.arg("build").arg("--release")
               .current_dir(&runner_dir)
               .env("CARGO_TARGET_DIR", &stub_target_dir);
               
            if let Some(t) = target {
                cmd.arg("--target").arg(t);
            }

            let output = cmd.output();

            let mut success = false;
            if let Ok(out) = output {
                if out.status.success() {
                    let mut src_path = stub_target_dir.clone();
                    if let Some(t) = target {
                        src_path = src_path.join(t);
                    }
                    src_path = src_path.join("release").join(src_name);
                    
                    if fs::copy(&src_path, &dest_path).is_ok() {
                        success = true;
                        println!("cargo:warning=Runner stub built and copied to {}", dest_path.display());
                    } else {
                        println!("cargo:warning=Failed to copy stub from {} to {}", src_path.display(), dest_path.display());
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    println!("cargo:warning=Failed to build stub: {}. Error: {}", dest_name, stderr);
                    if !stdout.is_empty() {
                        println!("cargo:warning=Stdout: {}", stdout);
                    }
                }
            } else if let Err(e) = output {
                println!("cargo:warning=Failed to execute build command for {}: {}", dest_name, e);
            }

            if !success {
                println!("cargo:warning=Using dummy file for stub: {}.", dest_name);
                if !dest_path.exists() {
                    let _ = fs::write(&dest_path, b"");
                }
            }
        } else {
            println!("cargo:warning=Using pre-existing stub for {}", dest_name);
        }
        
        stubs_rs.push_str(&format!("    stubs.insert(\"{}\", include_bytes!(\"../stubs/{}\"));\n", alias, dest_name));
        if let Some(t) = target {
            stubs_rs.push_str(&format!("    stubs.insert(\"{}\", include_bytes!(\"../stubs/{}\"));\n", t, dest_name));
        }
    };

    let host = env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    
    // Always build WASI if possible
    build_stub(Some("wasm32-wasip1"), "runner_stub_wasip1.wasm", "matchbox_runner.wasm", "wasi", &mut stubs_rs_content);

    if cfg!(feature = "cross-compile") {
        let targets = vec![
            ("x86_64-unknown-linux-gnu", "runner_stub_x86_64-unknown-linux-gnu", "matchbox_runner"),
            ("i686-unknown-linux-gnu", "runner_stub_i686-unknown-linux-gnu", "matchbox_runner"),
            ("aarch64-unknown-linux-gnu", "runner_stub_aarch64-unknown-linux-gnu", "matchbox_runner"),
            ("armv7-unknown-linux-gnueabihf", "runner_stub_armv7-unknown-linux-gnueabihf", "matchbox_runner"),
            ("x86_64-apple-darwin", "runner_stub_x86_64-apple-darwin", "matchbox_runner"),
            ("aarch64-apple-darwin", "runner_stub_aarch64-apple-darwin", "matchbox_runner"),
            ("x86_64-pc-windows-msvc", "runner_stub_x86_64-pc-windows-msvc.exe", "matchbox_runner.exe"),
            ("aarch64-pc-windows-msvc", "runner_stub_aarch64-pc-windows-msvc.exe", "matchbox_runner.exe"),
            ("x86_64-pc-windows-gnu", "runner_stub_x86_64-pc-windows-gnu.exe", "matchbox_runner.exe"),
        ];

        for (target, dest, src) in targets {
            // Skip macOS targets on Linux unless specifically requested, as they need osxcross/zig
            if cfg!(target_os = "linux") && target.contains("apple") {
                stubs_rs_content.push_str(&format!("    stubs.insert(\"{}\", include_bytes!(\"../stubs/{}\"));\n", target, dest));
                continue;
            }
            
            build_stub(Some(target), dest, src, target, &mut stubs_rs_content);
            if target == host {
                stubs_rs_content.push_str(&format!("    stubs.insert(\"host\", include_bytes!(\"../stubs/{}\"));\n", dest));
            }
        }
    } else {
        let native_src_name = if cfg!(windows) { "matchbox_runner.exe" } else { "matchbox_runner" };
        let dest_name = format!("runner_stub_{}", host);
        build_stub(None, &dest_name, native_src_name, "host", &mut stubs_rs_content);
        stubs_rs_content.push_str(&format!("    stubs.insert(\"{}\", include_bytes!(\"../stubs/{}\"));\n", host, dest_name));
    }

    stubs_rs_content.push_str("    stubs.get(target).copied()\n");
    stubs_rs_content.push_str("}\n");

    fs::write(&stubs_rs_path, stubs_rs_content).expect("Failed to write src/stubs.rs");
}
