//! Browser tests for `matchbox --target js`.
//!
//! Compiles `tests/browser/spike.bxs`, serves it with `boxlang-miniserver` and drives
//! `tests/browser/web/index.html` in Chromium with the bx-playwright module
//! (`tests/browser/run.bxs`, run on BoxLang JVM). The test skips with a message when
//! `boxlang`, `boxlang-miniserver`, `wasm-bindgen`, or the bx-playwright module is missing.
//!
//! Set `WASM_BINDGEN` to a wasm-bindgen 0.2.114 binary if it is not on PATH, and
//! `BX_PLAYWRIGHT_HOME` / `PATH` so bx-playwright finds Node.js and Chromium.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Test ids asserted by run.bxs. Grows as each fix lands.
const ENABLED: &[&str] = &["ping"];

fn tool(name: &str) -> Option<PathBuf> {
    Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

#[test]
fn browser_js_target() {
    let wasm_bindgen = std::env::var_os("WASM_BINDGEN").map(PathBuf::from).or_else(|| tool("wasm-bindgen"));
    let (Some(boxlang), Some(miniserver), Some(wasm_bindgen)) =
        (tool("boxlang"), tool("boxlang-miniserver"), wasm_bindgen)
    else {
        eprintln!("skipping: needs boxlang, boxlang-miniserver and wasm-bindgen");
        return;
    };
    let playwright_module = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".boxlang/modules/bx-playwright"))
        .filter(|p| p.exists());
    if playwright_module.is_none() {
        eprintln!("skipping: install the module with `install-bx-module bx-playwright`");
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let stage = std::env::temp_dir().join(format!("matchbox-browser-{}", std::process::id()));
    let _ = fs::remove_dir_all(&stage);
    copy_dir(&root.join("web"), &stage);
    fs::copy(root.join("spike.bxs"), stage.join("spike.bxs")).unwrap();

    let build = Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .args(["--target", "js", "spike.bxs"])
        .current_dir(&stage)
        .env("WASM_BINDGEN", &wasm_bindgen)
        .output()
        .unwrap();
    assert!(
        build.status.success() && stage.join("spike.js").exists(),
        "matchbox --target js failed:\n{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );

    let port = free_port();
    let _server = Server(
        Command::new(miniserver)
            .args(["--webroot", stage.to_str().unwrap(), "--port", &port.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(60), "miniserver did not start");
        std::thread::sleep(Duration::from_millis(200));
    }

    let run = Command::new(boxlang)
        .arg(root.join("run.bxs"))
        .env("MB_BASE_URL", format!("http://127.0.0.1:{port}"))
        .env("MB_ONLY", ENABLED.join(","))
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success() && out.contains("browser tests passed"),
        "browser tests failed:\n{out}\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let _ = fs::remove_dir_all(&stage);
}
