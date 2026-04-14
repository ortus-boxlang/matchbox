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
        if filled >= 4 && buffer[..filled].windows(4).any(|window| window == b"\r\n\r\n") {
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

    if method != "GET" {
        respond(&mut stream, "405 Method Not Allowed", "text/plain", b"method not allowed");
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

fn run_browser_page(test_name: &str, source: &str, html: &str) {
    if !firefox_available() {
        eprintln!("skipping {test_name}: firefox is unavailable");
        return;
    }

    let root = unique_test_dir(test_name);
    fs::create_dir_all(&root).unwrap();

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
        &[],
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
    let report = report_rx
        .recv_timeout(Duration::from_secs(20))
        .unwrap_or_else(|_| panic!("browser test {test_name} timed out waiting for report"));

    stop.store(true, Ordering::SeqCst);
    let _ = firefox.kill();
    let _ = firefox.wait();
    let _ = server.join();
    let _ = fs::remove_dir_all(&root);

    assert_eq!(report, "ok", "browser page reported failure: {report}");
}

#[test]
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

async function report(status) {
  await fetch(`/report/${status}`);
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
  await report("ok");
} catch (_error) {
  await report("fail");
}
</script>
</body>
</html>
"#;

    run_browser_page("browser_bundle_state_helper_and_readiness_work", source, html);
}

#[test]
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

async function report(status) {
  await fetch(`/report/${status}`);
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
    await report("bad-summary");
    throw new Error("bad-summary");
  }

  const target = document.getElementById("target");
  const result = await setNodeText(target, "after");
  if (result.text !== "after" || target.textContent !== "after") {
    await report("bad-node");
    throw new Error("bad-node");
  }

  await report("ok");
} catch (_error) {
  await report("fail");
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
fn browser_bundle_supports_multiple_modules_on_one_page() {
    // We need to compile two different modules. 
    // run_browser_page currently only compiles one.
    // I'll adjust it or do it manually here.
    
    let test_name = "browser_bundle_multiple_modules";
    if !firefox_available() {
        eprintln!("skipping {test_name}: firefox is unavailable");
        return;
    }

    let root = unique_test_dir(test_name);
    fs::create_dir_all(&root).unwrap();

    // Module A
    let source_a = "function getName() { return 'ModuleA' }";
    let source_path_a = root.join("moduleA.bxs");
    let output_path_a = root.join("moduleA.js");
    fs::write(&source_path_a, source_a).unwrap();
    matchbox::process_file(&source_path_a, false, Some("js"), vec![], false, false, false, Some(&output_path_a), &[], false, None, false, false, false, false).unwrap();

    // Module B
    let source_b = "function getName() { return 'ModuleB' }";
    let source_path_b = root.join("moduleB.bxs");
    let output_path_b = root.join("moduleB.js");
    fs::write(&source_path_b, source_b).unwrap();
    matchbox::process_file(&source_path_b, false, Some("js"), vec![], false, false, false, Some(&output_path_b), &[], false, None, false, false, false, false).unwrap();

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { getName as getA, ready as readyA } from "./moduleA.js";
import { getName as getB, ready as readyB } from "./moduleB.js";

async function report(status) {
  await fetch(`/report/${status}`);
}

try {
  await Promise.all([readyA, readyB]);
  const nameA = await getA();
  const nameB = await getB();
  
  if (nameA === 'ModuleA' && nameB === 'ModuleB') {
    await report("ok");
  } else {
    await report(`fail-${nameA}-${nameB}`);
  }
} catch (e) {
  await report("fail-exception");
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
fn browser_bundle_supports_callbacks_and_error_propagation() {
    let source = r#"
function runWithCallback(cb) {
    return cb(42)
}

function failMe() {
    throw "BoxLang Error"
}
"#;

    let html = r#"<!DOCTYPE html>
<html lang="en">
<body>
<script type="module">
import { runWithCallback, failMe, ready } from "./browser_bundle_callbacks_and_errors.js";

async function report(status) {
  await fetch(`/report/${status}`);
}

try {
  await ready;
  
  // Callback test
  const result = await runWithCallback((n) => n * 2);
  if (result !== 84) {
    await report(`fail-callback-${result}`);
    throw new Error("bad-callback");
  }

  // Error propagation test
  try {
    await failMe();
    await report("fail-no-error");
  } catch (e) {
    if (String(e).includes("BoxLang Error")) {
      await report("ok");
    } else {
      await report(`fail-wrong-error-${e}`);
    }
  }
} catch (e) {
  await report(`fail-exception-${e}`);
}
</script>
</body>
</html>
"#;

    run_browser_page("browser_bundle_callbacks_and_errors", source, html);
}
