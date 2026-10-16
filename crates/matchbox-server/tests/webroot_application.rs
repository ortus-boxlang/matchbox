use matchbox_server::webroot_core::{
    EmbeddedAssetStore, FileSystemAssetStore, WebrootConfig, WebrootEngine, WebrootRequest,
    WebrootRuntime,
};
use std::{collections::HashMap, sync::Arc};

fn request(path: &str) -> WebrootRequest {
    WebrootRequest {
        method: "GET".into(),
        path: path.into(),
        ..Default::default()
    }
}
fn engine(files: &[(&str, &str)]) -> WebrootEngine<EmbeddedAssetStore> {
    WebrootEngine::new(
        EmbeddedAssetStore::new(
            files
                .iter()
                .map(|(path, source)| (path.to_string(), source.as_bytes().to_vec()))
                .collect(),
        ),
        WebrootConfig::default(),
    )
}

#[test]
fn application_lifecycle_keeps_typed_state_and_recreates_request_instance() {
    let engine = engine(&[
        (
            "Application.bx",
            r#"class {
        this.name = "typed"; variables.localCount = 0;
        function init() { throw "Application init must not run"; }
        function onApplicationStart() { application.count = 0; application.items = [42]; application.answer = function() { return 7; }; writeOutput("start|"); }
        function onRequestStart(target) { variables.localCount++; application.count++; request.marker = "ok"; }
        function onRequestEnd(target) { writeOutput("|end"); }
    }"#,
        ),
        (
            "index.bxs",
            r#"writeOutput(application.count & "|" & application.items[1] & "|" & application.answer() & "|" & request.marker & "|" & variables.localCount);"#,
        ),
    ]);
    assert_eq!(
        engine.handle(request("/index.bxs")).unwrap().body,
        b"start|1|42|7|ok|1|end"
    );
    assert_eq!(
        engine.handle(request("/index.bxs")).unwrap().body,
        b"2|42|7|ok|1|end"
    );
}

#[test]
fn dynamic_includes_work_in_the_pseudo_constructor() {
    let engine = engine(&[
        (
            "Application.bx",
            r#"class {
        variables.config = "setup.bxs"; include variables.config;
        this.name = variables.marker;
        function onRequestStart(target) { writeOutput(variables.marker & "|"); }
    }"#,
        ),
        ("setup.bxs", "variables.marker='configured';"),
        ("index.bxs", "writeOutput('ok');"),
    ]);
    assert_eq!(
        engine.handle(request("/index.bxs")).unwrap().body,
        b"configured|ok"
    );
}

#[test]
fn request_override_includes_template_in_the_listener_context() {
    let engine = engine(&[
        (
            "Application.bx",
            r#"class {
        variables.marker = "context";
        function helper() { return "method"; }
        function onRequest(target) { writeOutput("before|"); include target; bx:header name="X-After-Include" value="works"; writeOutput("|tail"); }
        function onRequestEnd(target) { writeOutput("|after"); }
    }"#,
        ),
        (
            "index.bxs",
            "writeOutput(variables.marker & ':' & helper());",
        ),
    ]);
    let response = engine.handle(request("/index.bxs")).unwrap();
    assert_eq!(response.body, b"before|context:method|tail|after");
    assert!(
        response
            .headers
            .iter()
            .any(|(key, value)| key == "x-after-include" && value == "works")
    );
}

#[test]
fn false_start_still_runs_end_and_nearest_application_wins() {
    let engine = engine(&[
        (
            "Application.bx",
            r#"class {
        function onRequestStart(target) { writeOutput("stop"); return false; }
        function onRequestEnd(target) { writeOutput("|end"); }
    }"#,
        ),
        ("index.bxs", "throw 'must not execute';"),
        (
            "nested/Application.bx",
            r#"class { function onRequestStart(target) { writeOutput("child|"); } }"#,
        ),
        ("nested/index.bxs", "writeOutput('ok');"),
    ]);
    assert_eq!(
        engine.handle(request("/index.bxs")).unwrap().body,
        b"stop|end"
    );
    assert_eq!(
        engine.handle(request("/nested/index.bxs")).unwrap().body,
        b"child|ok"
    );
}

#[test]
fn error_skips_end_and_next_request_has_clean_globals() {
    let engine = engine(&[
        (
            "Application.bx",
            r#"class {
        function onError(error, event) { bx:header statusCode=409; writeOutput("handled"); }
        function onRequestEnd(target) { writeOutput("|end"); }
    }"#,
        ),
        ("bad.bxs", "temporary = 'private'; throw 'expected';"),
        (
            "ok.bxs",
            "writeOutput(structKeyExists(variables, 'temporary'));",
        ),
    ]);
    let response = engine.handle(request("/bad.bxs")).unwrap();
    assert_eq!(response.status, 409);
    assert_eq!(response.body, b"handled");
    assert_eq!(
        engine.handle(request("/ok.bxs")).unwrap().body,
        b"false|end"
    );
}

#[test]
fn missing_template_callback_and_application_source_protection() {
    let engine = engine(&[(
        "Application.bx",
        r#"class {
        function onMissingTemplate(target) { writeOutput("missing:" & target); return true; }
        function onRequestEnd(target) { throw "Missing templates must skip end"; }
    }"#,
    )]);
    let response = engine.handle(request("/absent.bxs")).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"missing:/absent.bxs");
    assert_eq!(
        engine.handle(request("/Application.bx")).unwrap().status,
        501
    );
}

#[test]
fn recursive_dynamic_includes_fail_without_poisoning_the_next_request() {
    let engine = engine(&[
        (
            "Application.bx",
            "class { function onRequest(target) { include target; } }",
        ),
        ("loop.bxs", "target='/loop.bxs'; include target;"),
        ("ok.bxs", "writeOutput('ok');"),
    ]);
    assert!(
        engine
            .handle(request("/loop.bxs"))
            .unwrap_err()
            .to_string()
            .contains("Maximum dynamic include depth")
    );
    assert_eq!(engine.handle(request("/ok.bxs")).unwrap().body, b"ok");
}

#[test]
fn retained_web_bifs_use_the_active_request_instead_of_capturing_old_bodies() {
    let engine = engine(&[
        (
            "Application.bx",
            "class { function onApplicationStart() { application.read=getHTTPRequestData; } }",
        ),
        (
            "index.bxs",
            "read=application.read; data=read(); writeOutput(data.content);",
        ),
    ]);
    for body in ["first-user", "second-user"] {
        let mut request = request("/index.bxs");
        request.method = "POST".into();
        request.body = body.as_bytes().to_vec();
        request
            .headers
            .insert("content-type".into(), "text/plain".into());
        assert_eq!(engine.handle(request).unwrap().body, body.as_bytes());
    }
}

#[test]
fn shared_runtime_shares_named_applications_between_mounts() {
    let runtime = WebrootRuntime::default();
    let files = HashMap::from([("Application.bx".into(), br#"class { this.name="shared"; function onApplicationStart() { application.count=0; } function onRequestStart(target) { application.count++; } }"#.to_vec()), ("index.bxs".into(), b"writeOutput(application.count);".to_vec())]);
    let a = WebrootEngine::with_runtime(
        EmbeddedAssetStore::new(files.clone()),
        WebrootConfig::default(),
        runtime.clone(),
    );
    let b = WebrootEngine::with_runtime(
        EmbeddedAssetStore::new(files),
        WebrootConfig::default(),
        runtime,
    );
    assert_eq!(a.handle(request("/index.bxs")).unwrap().body, b"1");
    assert_eq!(b.handle(request("/index.bxs")).unwrap().body, b"2");
}

#[test]
fn concurrent_requests_do_not_lose_application_updates() {
    let engine = Arc::new(engine(&[
        (
            "Application.bx",
            r#"class { this.name="counter"; function onApplicationStart() { application.count=0; } function onRequestStart(target) { application.count++; } }"#,
        ),
        ("index.bxs", "writeOutput(application.count);"),
    ]));
    let handles: Vec<_> = (0..12)
        .map(|_| {
            let engine = engine.clone();
            std::thread::spawn(move || {
                String::from_utf8(engine.handle(request("/index.bxs")).unwrap().body)
                    .unwrap()
                    .parse::<u32>()
                    .unwrap()
            })
        })
        .collect();
    let mut counts: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    counts.sort();
    assert_eq!(counts, (1..=12).collect::<Vec<_>>());
}

#[test]
fn application_end_runs_on_expiry_and_runtime_drop() {
    let root = tempfile::tempdir().unwrap();
    let log = root.path().join("end.txt");
    let log_path = log
        .to_string_lossy()
        .replace('\\', "/")
        .replace('"', "\"\"");
    std::fs::write(
        root.path().join("Application.bx"),
        format!(
            r#"class {{
        this.name="expiry"; this.applicationTimeout=createTimeSpan(0,0,0,0,100);
        function onApplicationStart() {{ application.count=0; }}
        function onRequestStart(target) {{ application.count++; }}
        function onApplicationEnd(scope) {{ fileAppend("{log_path}", scope.count); }}
    }}"#
        ),
    )
    .unwrap();
    std::fs::write(
        root.path().join("index.bxs"),
        "writeOutput(application.count);",
    )
    .unwrap();
    let engine = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().to_path_buf()),
        WebrootConfig::default(),
    );
    assert_eq!(engine.handle(request("/index.bxs")).unwrap().body, b"1");
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert_eq!(engine.handle(request("/index.bxs")).unwrap().body, b"1");
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "1");
    drop(engine);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "11");
}
