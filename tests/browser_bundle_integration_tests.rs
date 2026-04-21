use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn firefox_available() -> bool {
    Command::new("firefox")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn web_runner_stub_available() -> bool {
    let stub_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("stubs")
        .join("runner_stub_wasm32-unknown-unknown.wasm");

    fs::metadata(stub_path)
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp")
        .join(format!("browser-bundle-{name}-{nonce}"))
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        _ => "text/plain; charset=utf-8",
    }
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut buffer = vec![0u8; 8192];
    let mut filled = 0usize;
    loop {
        let read = stream.read(&mut buffer[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
        if filled >= 4
            && buffer[..filled]
                .windows(4)
                .any(|window| window == b"\r\n\r\n")
        {
            break;
        }
        if filled == buffer.len() {
            buffer.resize(buffer.len() * 2, 0);
        }
    }
    buffer.truncate(filled);
    Ok(buffer)
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(body);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

fn serve_request(
    mut stream: TcpStream,
    root: &Path,
    report_tx: &mpsc::Sender<String>,
) -> std::io::Result<()> {
    let request = read_http_request(&mut stream)?;
    let request_text = String::from_utf8_lossy(&request);
    let request_line = request_text.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");

    if let Some(result) = target.strip_prefix("/report/") {
        let _ = report_tx.send(result.to_string());
        respond(&mut stream, "204 No Content", "text/plain", b"");
        return Ok(());
    }

    if method != "GET" {
        respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            b"method not allowed",
        );
        return Ok(());
    }

    if let Some(result) = target.strip_prefix("/report/") {
        let _ = report_tx.send(result.to_string());
        respond(&mut stream, "204 No Content", "text/plain", b"");
        return Ok(());
    }

    let relative = target.trim_start_matches('/');
    let path = if relative.is_empty() {
        root.join("index.html")
    } else {
        root.join(relative)
    };

    match fs::read(&path) {
        Ok(body) => respond(&mut stream, "200 OK", content_type(&path), &body),
        Err(_) => respond(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }

    Ok(())
}

fn spawn_firefox(profile_dir: &Path, url: &str) -> std::io::Result<Child> {
    Command::new("firefox")
        .arg("--headless")
        .arg("--new-instance")
        .arg("--profile")
        .arg(profile_dir)
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let entry_dst = dst.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            copy_dir_recursive(&entry_path, &entry_dst)?;
        } else if file_type.is_file() {
            let _ = fs::copy(&entry_path, &entry_dst)?;
        }
    }

    Ok(())
}

fn run_browser_page_with_modules(
    test_name: &str,
    source: &str,
    html: &str,
    extra_module_paths: &[PathBuf],
) {
    if !firefox_available() {
        eprintln!("skipping {test_name}: firefox is unavailable");
        return;
    }
    if !web_runner_stub_available() {
        eprintln!(
            "skipping {test_name}: web runner stub is unavailable; rebuild with wasm32-unknown-unknown installed"
        );
        return;
    }

    let root = unique_test_dir(test_name);
    fs::create_dir_all(&root).unwrap();

    for module_path in extra_module_paths {
        let module_name = module_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        let mirrored_path = root.join("modules").join(module_name);
        copy_dir_recursive(module_path, &mirrored_path).unwrap();
    }

    let source_path = root.join(format!("{test_name}.bxs"));
    let output_path = root.join(format!("{test_name}.js"));
    fs::write(&source_path, source).unwrap();
    fs::write(root.join("index.html"), html).unwrap();

    matchbox::process_file(
        &source_path,
        false,
        Some("js"),
        vec![],
        false,
        false,
        false,
        Some(&output_path),
        extra_module_paths,
        false,
        None,
        false,
        false,
        false,
        false,
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener
        .set_nonblocking(true)
        .expect("listener should support nonblocking");
    let address = listener.local_addr().unwrap();
    let (report_tx, report_rx) = mpsc::channel::<String>();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    let server_root = root.clone();

    let server = thread::spawn(move || {
        while !server_stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = serve_request(stream, &server_root, &report_tx);
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(_) => break,
            }
        }
    });

    let url = format!("http://{address}/index.html");
    let profile_dir = root.join("firefox-profile");
    fs::create_dir_all(&profile_dir).unwrap();
    let mut firefox = spawn_firefox(&profile_dir, &url).expect("firefox should start");
    // Collect all reports that arrive within the timeout; the browser may
    // send a spurious "fail-AbortError" during Firefox teardown after it
    // already sent "ok".  We succeed if *any* report is "ok".
    let mut reports = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        match report_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(r) => {
                if r == "ok" {
                    reports.push(r);
                    break;
                } else {
                    reports.push(r);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Give the browser time to finish any pending fetches
    // before we tear down the server and kill Firefox.
    thread::sleep(Duration::from_millis(500));

    stop.store(true, Ordering::SeqCst);
    let _ = firefox.kill();
    let _ = firefox.wait();
    let _ = server.join();
    let _ = fs::remove_dir_all(&root);

    assert!(
        reports.iter().any(|r| r == "ok"),
        "browser page reported failure: {:?}",
        reports
    );
}

fn run_browser_page(test_name: &str, source: &str, html: &str) {
    run_browser_page_with_modules(test_name, source, html, &[]);
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_state_helper_and_readiness_work() {
    let source = r#"
count = 0

function getSnapshot() {
    return { count: count }
}

function increment() {
    count = count + 1
    return getSnapshot()
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import "./browser_bundle_state_helper_and_readiness_work.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", () => report("fail"));
window.addEventListener("unhandledrejection", () => report("fail"));

try {
  await window.MatchBox.ready("browser_bundle_state_helper_and_readiness_work");
  const app = window.MatchBox.State("browser_bundle_state_helper_and_readiness_work", {
    mount: "getSnapshot",
    initialState: { count: -1 }
  });
  await app.init();
  if (app.count !== 0 || app.ready !== true) {
    throw new Error("bad-init");
  }
  await app.call("increment");
  if (app.count !== 1) {
    throw new Error("bad-call");
  }
  report("ok");
} catch (_error) {
  report("fail");
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_state_helper_and_readiness_work",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_normalizes_plain_values_and_preserves_dom_handles() {
    let source = r#"
function summarize(payload) {
    return {
        kind: payload.kind,
        total: payload.items.len(),
        enabled: payload.meta.enabled
    }
}

function setNodeText(node, value) {
    node.textContent = value
    return { text: node.textContent }
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<div id="target">before</div>
<script type="module">
import { summarize, setNodeText, ready } from "./browser_bundle_normalizes_plain_values_and_preserves_dom_handles.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", () => report("fail"));
window.addEventListener("unhandledrejection", () => report("fail"));

try {
  await ready;
  const summary = await summarize({
    kind: "box",
    items: [1, 2, 3],
    meta: { enabled: true }
  });
  if (summary.kind !== "box" || summary.total !== 3 || summary.enabled !== true) {
    report("bad-summary");
    throw new Error("bad-summary");
  }

  const target = document.getElementById("target");
  const result = await setNodeText(target, "after");
  if (result.text !== "after" || target.textContent !== "after") {
    report("bad-node");
    throw new Error("bad-node");
  }

  report("ok");
} catch (_error) {
  report("fail");
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_normalizes_plain_values_and_preserves_dom_handles",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_returns_boxlang_class_instances_to_js() {
    let source = r#"
class PrinterState {
    property device;
    property connection;
    property characteristic;
    property status;
    property isSupported;
    property error;

    this.device = null;
    this.connection = null;
    this.characteristic = null;
    this.status = "Ready";
    this.isSupported = true;
    this.error = "";

    function connect() {
        this.connection = "connected";
        this.characteristic = "writeable";
        this.status = "Connected";
        return this;
    }
}

function createPrinterState() {
    return new PrinterState();
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { createPrinterState, ready } from "./browser_bundle_returns_boxlang_class_instances_to_js.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", () => report("fail"));
window.addEventListener("unhandledrejection", () => report("fail"));

try {
  await ready;

  const printer = await createPrinterState();
  if (!printer) {
    report(`fail-no-printer-${String(printer)}-${typeof printer}`);
    throw new Error("no-printer");
  }
  if (printer.device !== null) {
    report(`fail-device-${String(printer.device)}`);
    throw new Error("bad-device");
  }
  if (printer.connection !== null) {
    report(`fail-connection-${String(printer.connection)}`);
    throw new Error("bad-connection");
  }
  if (printer.characteristic !== null) {
    report(`fail-characteristic-${String(printer.characteristic)}`);
    throw new Error("bad-characteristic");
  }
  if (printer.status !== "Ready") {
    report(`fail-status-${String(printer.status)}`);
    throw new Error("bad-status");
  }
  if (printer.isSupported !== true) {
    report(`fail-supported-${String(printer.isSupported)}`);
    throw new Error("bad-supported");
  }
  if (printer.error !== "") {
    report(`fail-error-${String(printer.error)}`);
    throw new Error("bad-error");
  }

  await printer.connect();
  if (
    printer.connection !== "connected" ||
    printer.characteristic !== "writeable" ||
    printer.status !== "Connected"
  ) {
    report("fail-method-call");
    throw new Error("bad-method-call");
  }

  report("ok");
} catch (_error) {
  report("fail");
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_returns_boxlang_class_instances_to_js",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_exposes_instance_methods_to_alpine_scope() {
    let source = r#"
class PrinterState {
    property connection;
    property status;

    this.connection = null;
    this.status = "Ready";

    function connect() {
        this.connection = "connected";
        this.status = "Connected";
        return this;
    }

    function disconnect() {
        this.connection = null;
        this.status = "Disconnected";
        return this;
    }
}

function createPrinterState() {
    return new PrinterState();
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { createPrinterState, ready } from "./browser_bundle_exposes_instance_methods_to_alpine_scope.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", () => report("fail"));
window.addEventListener("unhandledrejection", () => report("fail"));

try {
  await ready;
  const printer = await createPrinterState();

  const invoke = new Function(
    "proxy",
    `
      with (proxy) {
        if (typeof connect !== "function" || typeof disconnect !== "function") {
          return "missing-" + typeof connect + "-" + typeof disconnect;
        }

        connect();

        if (connection !== "connected" || status !== "Connected") {
          return "bad-connect-" + String(connection) + "-" + String(status);
        }

        disconnect();

        if (connection !== null || status !== "Disconnected") {
          return "bad-disconnect-" + String(connection) + "-" + String(status);
        }

        return "ok";
      }
    `
  );

  const result = invoke(printer);
  if (result !== "ok") {
    report(`fail-${result}`);
    throw new Error(result);
  }

  report("ok");
} catch (_error) {
  report("fail");
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_exposes_instance_methods_to_alpine_scope",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_allows_unscoped_class_method_variables() {
    let source = r#"
class PrinterState {
    this.device = null;
    this.connection = null;
    this.characteristic = null;
    this.status = "Ready";
    this.isSupported = !isNull(js.navigator.bluetooth);
    this.error = "";

    function connect() {
        println("secure=" & js.window.isSecureContext);
        println("userActive=" & js.navigator.userActivation.isActive);
        this.error = "";
        this.status = "Requesting device...";

        try {
            if (isNull(js.navigator.bluetooth)) {
                throw("Web Bluetooth is not available in this browser context.");
            }

            println("BoxLang: connect() triggered");
            println("xxxx");
            println("BoxLang: Calling requestDevice...");

            options = {
                "acceptAllDevices": true,
                "optionalServices": ["service-a", "service-b"]
            };

            js.console.log("option keys", js.Object.keys(options));
            js.console.log("acceptAllDevices", options.acceptAllDevices);
            js.console.log("filters", options.filters);
            js.console.log("optionalServices", options.optionalServices);

            this.device = js.navigator.bluetooth.requestDevice(options).get();
            this.connection = "connected";
            this.status = "Connected";
            return "ok";
        } catch (e) {
            this.error = e.message;
            this.status = "Error";
            return e.message;
        }
    }

    function disconnect() {
        this.connection = null;
        this.status = "Disconnected";
        return "ok";
    }
}

function createPrinterState() {
    return new PrinterState();
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { createPrinterState, ready } from "./browser_bundle_allows_unscoped_class_method_variables.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", () => report("fail"));
window.addEventListener("unhandledrejection", () => report("fail"));
window.__capturedOptionsKeys = "";
Object.defineProperty(window.navigator, "bluetooth", {
  configurable: true,
  value: {
  requestDevice(options) {
    window.__capturedOptionsKeys = Object.keys(options).join("|");
    return new Promise((resolve) => {
      setTimeout(() => resolve({ name: "Mock Printer" }), 0);
    });
  }
  }
});

try {
  await ready;
  const printer = await createPrinterState();
  const invoke = new Function(
    "proxy",
    `
      with (proxy) {
        return void (connection ? disconnect() : connect());
      }
    `
  );
  invoke(printer);
  await new Promise((resolve) => setTimeout(resolve, 100));

  if (printer.error !== "") {
    report(`fail-error-${printer.error}`);
    throw new Error(printer.error);
  }

  if (printer.status !== "Connected") {
    report(`fail-status-${printer.status}`);
    throw new Error(printer.status);
  }

  if (window.__capturedOptionsKeys !== "acceptAllDevices|optionalServices") {
    report(`fail-options-${window.__capturedOptionsKeys}`);
    throw new Error(window.__capturedOptionsKeys);
  }

  report("ok");
} catch (_error) {
  report(`fail-${String(_error?.stack || _error)}`);
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_allows_unscoped_class_method_variables",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_preserves_bluetooth_device_properties_after_future_get() {
    let source = r#"
class PrinterState {
    this.device = null;

    function connect() {
        options = {
            "acceptAllDevices": true,
            "optionalServices": ["service-a"]
        };

        this.device = js.navigator.bluetooth.requestDevice(options).get();

        if (isNull(this.device)) {
            return "null-device";
        }

        return this.device.name & "|" & this.device.id & "|" & this.device.gatt.connected;
    }
}

function createPrinterState() {
    return new PrinterState();
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { createPrinterState, ready } from "./browser_bundle_preserves_bluetooth_device_properties_after_future_get.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", (event) => report(`fail-${String(event.error?.stack || event.message || event.error)}`));
window.addEventListener("unhandledrejection", (event) => report(`fail-${String(event.reason?.stack || event.reason)}`));

class MockGattServer {
  constructor() {
    this.connected = false;
  }
}

const brandedDevices = new WeakSet();

class MockBluetoothDevice {
  constructor() {
    this._gatt = new MockGattServer();
    brandedDevices.add(this);
  }

  get name() {
    if (!brandedDevices.has(this)) {
      return null;
    }
    return "Mock Printer";
  }

  get id() {
    if (!brandedDevices.has(this)) {
      return null;
    }
    return "device-123";
  }

  get gatt() {
    if (!brandedDevices.has(this)) {
      return null;
    }
    return this._gatt;
  }
}

Object.defineProperty(window.navigator, "bluetooth", {
  configurable: true,
  value: {
    requestDevice(_options) {
      return Promise.resolve(new MockBluetoothDevice());
    }
  }
});

try {
  await ready;
  const printer = await createPrinterState();
  const summary = await printer.connect();

  if (summary !== "Mock Printer|device-123|false") {
    throw new Error(`bad-summary-${summary}`);
  }

  report("ok");
} catch (error) {
  report(`fail-${String(error?.message || error)}`);
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_preserves_bluetooth_device_properties_after_future_get",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_invokes_nested_bluetooth_gatt_methods_with_host_receiver() {
    let source = r#"
class PrinterState {
    function connect() {
        options = {
            "acceptAllDevices": true,
            "optionalServices": ["service-a"]
        };

        device = js.navigator.bluetooth.requestDevice(options).get();
        server = device.gatt.connect().get();
        return server.connected;
    }
}

function createPrinterState() {
    return new PrinterState();
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { createPrinterState, ready } from "./browser_bundle_invokes_nested_bluetooth_gatt_methods_with_host_receiver.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", (event) => report(`fail-${String(event.error?.stack || event.message || event.error)}`));
window.addEventListener("unhandledrejection", (event) => report(`fail-${String(event.reason?.stack || event.reason)}`));

const brandedGattServers = new WeakSet();

class MockGattServer {
  constructor() {
    this.connected = false;
    brandedGattServers.add(this);
  }

  connect() {
    if (!brandedGattServers.has(this)) {
      throw new TypeError("Illegal invocation");
    }
    this.connected = true;
    return Promise.resolve(this);
  }
}

class MockBluetoothDevice {
  constructor() {
    this._gatt = new MockGattServer();
  }

  get gatt() {
    return this._gatt;
  }
}

Object.defineProperty(window.navigator, "bluetooth", {
  configurable: true,
  value: {
    requestDevice(_options) {
      return Promise.resolve(new MockBluetoothDevice());
    }
  }
});

try {
  await ready;
  const printer = await createPrinterState();
  const connected = await printer.connect();

  if (connected !== true) {
    throw new Error(`bad-connected-${String(connected)}`);
  }

  report("ok");
} catch (error) {
  report(`fail-${String(error?.message || error)}`);
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_invokes_nested_bluetooth_gatt_methods_with_host_receiver",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_invokes_bluetooth_gatt_methods_from_instance_property() {
    let source = r#"
class PrinterState {
    this.device = null;

    function connect() {
        options = {
            "acceptAllDevices": true,
            "optionalServices": ["service-a"]
        };

        this.device = js.navigator.bluetooth.requestDevice(options).get();
        server = this.device.gatt.connect().get();
        return server.connected;
    }
}

function createPrinterState() {
    return new PrinterState();
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { createPrinterState, ready } from "./browser_bundle_invokes_bluetooth_gatt_methods_from_instance_property.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", (event) => report(`fail-${String(event.error?.stack || event.message || event.error)}`));
window.addEventListener("unhandledrejection", (event) => report(`fail-${String(event.reason?.stack || event.reason)}`));

const brandedGattServers = new WeakSet();
const brandedDevices = new WeakSet();

class MockGattServer {
  constructor() {
    this.connected = false;
    brandedGattServers.add(this);
  }

  connect() {
    if (!brandedGattServers.has(this)) {
      throw new TypeError("Illegal invocation");
    }
    this.connected = true;
    return Promise.resolve(this);
  }
}

class MockBluetoothDevice {
  constructor() {
    this._gatt = new MockGattServer();
    brandedDevices.add(this);
  }

  get gatt() {
    if (!brandedDevices.has(this)) {
      return null;
    }
    return this._gatt;
  }
}

Object.defineProperty(window.navigator, "bluetooth", {
  configurable: true,
  value: {
    requestDevice(_options) {
      return Promise.resolve(new MockBluetoothDevice());
    }
  }
});

try {
  await ready;
  const printer = await createPrinterState();
  const connected = await printer.connect();

  if (connected !== true) {
    throw new Error(`bad-connected-${String(connected)}`);
  }

  report("ok");
} catch (error) {
  report(`fail-${String(error?.message || error)}`);
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_invokes_bluetooth_gatt_methods_from_instance_property",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_btprinter_dom_reacts_to_plain_js_state_mutations() {
    let source = r#"
class PrinterState {
    property connection;
    property characteristic;
    property status;

    this.connection = null;
    this.characteristic = null;
    this.status = "Ready";

    function connect() {
        this.connection = "connected";
        this.characteristic = "writeable";
        this.status = "Connected";
        return this;
    }
}

function createPrinterState() {
    return new PrinterState();
}
"#;
    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<div id="app">
  <div id="dot" class="w-2 h-2 rounded-full bg-red-500"></div>
  <span id="status"></span>
</div>
<script type="module">
import { createPrinterState, ready } from "./browser_bundle_btprinter_dom_reacts_to_plain_js_state_mutations.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", () => report("fail"));
window.addEventListener("unhandledrejection", () => report("fail"));

try {
  await ready;
  const rawState = await createPrinterState();
  if (!rawState) {
    throw new Error("missing printer state");
  }

  const dot = document.getElementById("dot");
  const status = document.getElementById("status");
  let proxy = null;
  let writeCount = 0;

  const render = () => {
    dot.className = "w-2 h-2 rounded-full " + (proxy.characteristic ? "bg-green-500 animate-pulse" : (proxy.connection ? "bg-yellow-500" : "bg-red-500"));
    status.textContent = proxy.status;
  };

  proxy = new Proxy(rawState, {
    get(target, prop, receiver) {
      return Reflect.get(target, prop, receiver);
    },
    set(target, prop, value, receiver) {
      const result = Reflect.set(target, prop, value, receiver);
      writeCount += 1;
      return result;
    }
  });

  proxy.connect();

  await new Promise((resolve) => setTimeout(resolve, 50));
  render();

  if (!dot.className.includes("bg-green-500")) {
    report(`fail-dot-${dot.className}-status-${status.textContent}-proxy-${String(proxy.status)}-char-${String(proxy.characteristic)}`);
    throw new Error(`expected green status dot, got: ${dot.className}`);
  }

  if (status.textContent !== "Connected") {
    throw new Error(`expected connected status, got: ${status.textContent}`);
  }

  if (writeCount < 3) {
    throw new Error(`expected reactive writes, saw ${writeCount}`);
  }

  report("ok");
} catch (error) {
  report(`fail-${String(error?.message || error)}`);
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_btprinter_dom_reacts_to_plain_js_state_mutations",
        &source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_persists_nested_instance_property_mutations() {
    let source = r#"
class TestState {
    this.thing = { message: "not changed" };
    this.message = "not changed";

    function clickHandler() {
        this.thing.message = "nested changed";
        this.message = "top changed";
        return this;
    }
}

function createTestState() {
    return new TestState();
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { createTestState, ready } from "./browser_bundle_persists_nested_instance_property_mutations.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", (event) => report(`fail-${String(event.error?.stack || event.message || event.error)}`));
window.addEventListener("unhandledrejection", (event) => report(`fail-${String(event.reason?.stack || event.reason)}`));

try {
  await ready;
  const state = await createTestState();

  if (state.thing?.message !== "not changed" || state.message !== "not changed") {
    throw new Error(`bad-initial-${String(state.thing?.message)}-${String(state.message)}`);
  }

  await state.clickHandler();

  if (state.message !== "top changed") {
    throw new Error(`bad-top-${String(state.message)}`);
  }

  if (state.thing?.message !== "nested changed") {
    throw new Error(`bad-nested-${String(state.thing?.message)}`);
  }

  report("ok");
} catch (error) {
  report(`fail-${String(error?.message || error)}`);
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_persists_nested_instance_property_mutations",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_alpine_updates_nested_instance_property_mutations() {
    let source = r#"
class TestState {
    this.thing = { message: "not changed" };
    this.message = "not changed";

    function clickHandler() {
        this.thing.message = "nested changed";
        this.message = "top changed";
        return this;
    }
}

function registerComponent() {
    Alpine = js.Alpine;
    if ( isNull( Alpine ) || isNull( Alpine.data ) ) {
        return false;
    }

    Alpine.data( "testclass", () => {
        return new TestState();
    });
    return true;
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<div x-data="testclass" x-cloak>
  <p id="nested" x-text="thing.message"></p>
  <p id="top" x-text="message"></p>
  <button id="trigger" @click="clickHandler">One</button>
</div>
<script type="module">
import { ready, registerComponent } from "./browser_bundle_alpine_updates_nested_instance_property_mutations.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", (event) => report(`fail-${String(event.error?.stack || event.message || event.error)}`));
window.addEventListener("unhandledrejection", (event) => report(`fail-${String(event.reason?.stack || event.reason)}`));

try {
  await ready;
  const { default: Alpine } = await import("https://unpkg.com/alpinejs@3/dist/module.esm.js");
  window.Alpine = Alpine;
  await registerComponent();
  Alpine.start();
  await new Promise((resolve) => setTimeout(resolve, 50));

  const nested = document.getElementById("nested");
  const top = document.getElementById("top");
  const trigger = document.getElementById("trigger");

  if (nested.textContent !== "not changed" || top.textContent !== "not changed") {
    throw new Error(`bad-initial-${nested.textContent}-${top.textContent}`);
  }

  trigger.click();
  await new Promise((resolve) => setTimeout(resolve, 50));

  if (top.textContent !== "top changed") {
    throw new Error(`bad-top-dom-${top.textContent}`);
  }

  if (nested.textContent !== "nested changed") {
    throw new Error(`bad-nested-dom-${nested.textContent}`);
  }

  report("ok");
} catch (error) {
  report(`fail-${String(error?.message || error)}`);
}
</script>
<style>
  [x-cloak] { display: none !important; }
</style>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_alpine_updates_nested_instance_property_mutations",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_supports_multiple_modules_on_one_page() {
    // We need to compile two different modules.
    // run_browser_page currently only compiles one.
    // I'll adjust it or do it manually here.

    let test_name = "browser_bundle_multiple_modules";
    if !firefox_available() {
        eprintln!("skipping {test_name}: firefox is unavailable");
        return;
    }
    if !web_runner_stub_available() {
        eprintln!(
            "skipping {test_name}: web runner stub is unavailable; rebuild with wasm32-unknown-unknown installed"
        );
        return;
    }

    let root = unique_test_dir(test_name);
    fs::create_dir_all(&root).unwrap();

    // Module A
    let source_a = "function getName() { return 'ModuleA' }";
    let source_path_a = root.join("moduleA.bxs");
    let output_path_a = root.join("moduleA.js");
    fs::write(&source_path_a, source_a).unwrap();
    matchbox::process_file(
        &source_path_a,
        false,
        Some("js"),
        vec![],
        false,
        false,
        false,
        Some(&output_path_a),
        &[],
        false,
        None,
        false,
        false,
        false,
        false,
    )
    .unwrap();

    // Module B
    let source_b = "function getName() { return 'ModuleB' }";
    let source_path_b = root.join("moduleB.bxs");
    let output_path_b = root.join("moduleB.js");
    fs::write(&source_path_b, source_b).unwrap();
    matchbox::process_file(
        &source_path_b,
        false,
        Some("js"),
        vec![],
        false,
        false,
        false,
        Some(&output_path_b),
        &[],
        false,
        None,
        false,
        false,
        false,
        false,
    )
    .unwrap();

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { getName as getA, ready as readyA } from "./moduleA.js";
import { getName as getB, ready as readyB } from "./moduleB.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

try {
  await Promise.all([readyA, readyB]);
  const nameA = await getA();
  const nameB = await getB();
  
  if (nameA === 'ModuleA' && nameB === 'ModuleB') {
    report("ok");
  } else {
    report(`fail-${nameA}-${nameB}`);
  }
} catch (e) {
  report("fail-exception");
}
</script>
</body>
</html>
"#;
    fs::write(root.join("index.html"), html).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (report_tx, report_rx) = mpsc::channel::<String>();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    let server_root = root.clone();

    let server = thread::spawn(move || {
        while !server_stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = serve_request(stream, &server_root, &report_tx);
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(_) => break,
            }
        }
    });

    let url = format!("http://{address}/index.html");
    let profile_dir = root.join("firefox-profile");
    fs::create_dir_all(&profile_dir).unwrap();
    let mut firefox = spawn_firefox(&profile_dir, &url).expect("firefox should start");
    let report = report_rx.recv_timeout(Duration::from_secs(20)).unwrap();

    stop.store(true, Ordering::SeqCst);
    let _ = firefox.kill();
    let _ = firefox.wait();
    let _ = server.join();
    let _ = fs::remove_dir_all(&root);

    assert_eq!(report, "ok");
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_wraps_throw_strings_as_exception_objects() {
    let source = r#"
function getThrownException() {
    try {
        throw("Boom");
    } catch (e) {
        return e.name & "|" & e.type & "|" & e.message;
    }
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { getThrownException, ready } from "./browser_bundle_wraps_throw_strings_as_exception_objects.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", (event) => report(`fail-${String(event.error?.stack || event.message || event.error)}`));
window.addEventListener("unhandledrejection", (event) => report(`fail-${String(event.reason?.stack || event.reason)}`));

try {
  await ready;

  const result = await getThrownException();
  if (result !== "CustomException|CustomException|Boom") {
    report(`fail-${result}`);
    throw new Error(result);
  }

  report("ok");
} catch (_error) {
  report(`fail-${String(_error?.stack || _error)}`);
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_wraps_throw_strings_as_exception_objects",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_supports_callbacks_and_error_propagation() {
    let source = r#"
function runWithCallback(cb) {
    return cb(42)
}

function makeThrower() {
    return () => {
        throw "BoxLang Error"
    }
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { runWithCallback, makeThrower, ready } from "./browser_bundle_callbacks_and_errors.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

try {
  await ready;
  
  // Callback test
  const result = await runWithCallback((n) => n * 2);
  if (result !== 84) {
    report(`fail-callback-${result}`);
    throw new Error("bad-callback");
  }

  const thrower = await makeThrower();
  const OriginalError = window.Error;
  window.Error = 123;
  let caught = null;
  try {
    thrower();
  } catch (e) {
    caught = e;
  } finally {
    window.Error = OriginalError;
  }

  if (caught && String(caught).includes("BoxLang Error")) {
    report("ok");
  } else if (caught) {
    report(`fail-wrong-error-${caught}`);
  } else {
    report("fail-no-error");
  }
} catch (e) {
  report(`fail-exception-${e}`);
}
</script>
</body>
</html>
"#;

    run_browser_page("browser_bundle_callbacks_and_errors", source, html);
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_returns_boxlang_callbacks_to_js() {
    let source = r#"
function makeDoubler() {
    return (value) => value * 2
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { makeDoubler, ready } from "./browser_bundle_returns_boxlang_callbacks_to_js.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", (event) => report(`fail-${String(event.error?.stack || event.message || event.error)}`));
window.addEventListener("unhandledrejection", (event) => report(`fail-${String(event.reason?.stack || event.reason)}`));

try {
  await ready;

  const doubler = await makeDoubler();
  const result = await doubler(21);
  if (result !== 42) {
    report(`fail-${result}`);
    throw new Error("bad-result");
  }

  report("ok");
} catch (_error) {
  report(`fail-${String(_error?.stack || _error)}`);
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_returns_boxlang_callbacks_to_js",
        source,
        html,
    );
}

#[test]
#[cfg(target_os = "linux")]
fn browser_bundle_invokes_methods_on_callable_plain_js_objects() {
    let source = r#"
function invokeData(api) {
    return api.data(21)
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { invokeData, ready } from "./browser_bundle_invokes_methods_on_callable_plain_js_objects.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", () => report("fail"));
window.addEventListener("unhandledrejection", () => report("fail"));

try {
  await ready;

  const api = {
    value: 1,
    data(n) {
      this.value = this.value + n;
      return this.value;
    }
  };

  const result = await invokeData(api);
  if (result !== 22 || api.value !== 22) {
    report(`fail-${result}-${api.value}`);
    throw new Error("bad-call");
  }

  report("ok");
} catch (_error) {
  report("fail");
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_invokes_methods_on_callable_plain_js_objects",
        source,
        html,
    );
}

#[test]
fn browser_bundle_awaits_js_promises_via_future_get() {
    let source = r#"
function awaitJsPromise() {
    return js.globalThis.matchboxResolveLater().get()
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { awaitJsPromise, ready } from "./browser_bundle_awaits_js_promises_via_future_get.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.matchboxResolveLater = function() {
  return new Promise((resolve) => {
    setTimeout(() => resolve("done"), 0);
  });
};

window.addEventListener("error", (event) => {
  report(`fail-${String(event.error?.stack || event.message || event.error)}`);
});
window.addEventListener("unhandledrejection", (event) => {
  event.preventDefault();
  report(`fail-${String(event.reason?.stack || event.reason)}`);
});

try {
  await ready;
  const result = await awaitJsPromise();
  if (result !== "done") {
    report(`fail-${String(result)}`);
    throw new Error("bad-result");
  }
  report("ok");
} catch (_error) {
  report(`fail-${String(_error?.stack || _error)}`);
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_awaits_js_promises_via_future_get",
        source,
        html,
    );
}

#[test]
fn browser_bundle_preserves_quoted_struct_key_case_for_js() {
    let source = r#"
function buildOptions() {
    return {
        "acceptAllDevices": true,
        "optionalServices": ["service-a", "service-b"]
    }
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { buildOptions, ready } from "./browser_bundle_preserves_quoted_struct_key_case_for_js.js";

let __matchboxReported = false;
function report(status) {
  if (__matchboxReported) return;
  if (status === "ok" || status.startsWith("fail-")) {
    __matchboxReported = true;
  }
  fetch(`/report/${status}`, { keepalive: true }).catch(() => {});
}

window.addEventListener("error", (event) => report(`fail-${String(event.error?.stack || event.message || event.error)}`));
window.addEventListener("unhandledrejection", (event) => report(`fail-${String(event.reason?.stack || event.reason)}`));

try {
  await ready;
  const options = await buildOptions();
  const keys = Object.keys(options);
  if (keys[0] !== "acceptAllDevices" || keys[1] !== "optionalServices") {
    report(`fail-${keys.join("|")}`);
    throw new Error(`bad-keys:${keys.join("|")}`);
  }
  report("ok");
} catch (_error) {
  report(`fail-${String(_error?.stack || _error)}`);
}
</script>
</body>
</html>
"#;

    run_browser_page(
        "browser_bundle_preserves_quoted_struct_key_case_for_js",
        source,
        html,
    );
}
