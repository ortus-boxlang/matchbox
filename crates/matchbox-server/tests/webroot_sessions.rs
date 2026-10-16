use matchbox_server::webroot_core::{
    EmbeddedAssetStore, FileSystemAssetStore, WebrootConfig, WebrootEngine, WebrootRequest,
    WebrootRuntime,
};
use std::{collections::HashMap, sync::Arc};
fn request(path: &str, cookie: Option<&str>) -> WebrootRequest {
    WebrootRequest {
        method: "GET".into(),
        path: path.into(),
        headers: cookie
            .map(|value| HashMap::from([("cookie".into(), value.into())]))
            .unwrap_or_default(),
        ..Default::default()
    }
}
fn engine(files: &[(&str, &str)], runtime: WebrootRuntime) -> WebrootEngine<EmbeddedAssetStore> {
    WebrootEngine::with_runtime(
        EmbeddedAssetStore::new(
            files
                .iter()
                .map(|(path, source)| (path.to_string(), source.as_bytes().to_vec()))
                .collect(),
        ),
        WebrootConfig::default(),
        runtime,
    )
}
#[test]
fn typed_sessions_share_named_applications_across_mounts_but_not_other_applications() {
    let runtime = WebrootRuntime::default();
    let a = Arc::new(engine(
        &[
            (
                "Application.bx",
                "class {this.name='shared';this.sessionManagement=true;function onSessionStart(id){session.count=0;session.values=[4,5];session.answer=function(){return 42;};}}",
            ),
            (
                "index.bxs",
                "session.count++;writeOutput(session.count & '|' & session.values[2] & '|' & session.answer());",
            ),
        ],
        runtime.clone(),
    ));
    let b = engine(
        &[
            (
                "Application.bx",
                "class {this.name='other';this.sessionManagement=true;function onSessionStart(id){session.count=0;}}",
            ),
            ("index.bxs", "session.count++;writeOutput(session.count);"),
        ],
        runtime,
    );
    let first = a.handle(request("/index.bxs", None)).unwrap();
    assert_eq!(first.body, b"1|5|42");
    let cookie = first
        .headers
        .iter()
        .find(|(key, _)| key == "set-cookie")
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap()
        .to_string();
    assert!(cookie.starts_with("jsessionid="));
    let mut workers = vec![];
    for _ in 0..12 {
        let a = a.clone();
        let cookie = cookie.clone();
        workers.push(std::thread::spawn(move || {
            String::from_utf8(a.handle(request("/index.bxs", Some(&cookie))).unwrap().body).unwrap()
        }));
    }
    let mut counts: Vec<_> = workers
        .into_iter()
        .map(|worker| {
            worker
                .join()
                .unwrap()
                .split('|')
                .next()
                .unwrap()
                .parse::<usize>()
                .unwrap()
        })
        .collect();
    counts.sort();
    assert_eq!(counts, (2..=13).collect::<Vec<_>>());
    assert_eq!(
        b.handle(request("/index.bxs", Some(&cookie))).unwrap().body,
        b"1"
    );
}
#[test]
fn cookies_are_checked_during_script_assignments_and_struct_bifs_not_only_at_flush() {
    let engine = engine(
        &[(
            "index.bxs",
            "cookie.token={value:'a b',httpOnly:true,sameSite:'Strict'};structInsert(cookie,'other','x');try {cookie.bad={value:'unsafe',domain:('host;' & 'Injected=yes')};}catch(any e){writeOutput('caught|');}writeOutput(cookie.token & '|' & structCount(cookie));",
        )],
        WebrootRuntime::default(),
    );
    let response = engine.handle(request("/index.bxs", None)).unwrap();
    assert_eq!(response.body, b"caught|a b|2");
    assert!(response.headers.contains(&(
        "set-cookie".into(),
        "token=a+b; Path=/; HttpOnly; SameSite=Strict".into()
    )));
    assert_eq!(
        response
            .headers
            .iter()
            .filter(|(key, _)| key == "set-cookie")
            .count(),
        2
    );
}
#[test]
fn failed_session_start_retries_and_cookie_side_effects_survive_error_redaction() {
    let engine = engine(
        &[
            (
                "Application.bx",
                "class {this.name='retry';this.sessionManagement=true;function onApplicationStart(){application.calls=0;}function onSessionStart(id){application.calls++;if(application.calls==1){throw 'private secret';}session.ready=true;}}",
            ),
            (
                "index.bxs",
                "writeOutput(session.ready & '|' & application.calls);",
            ),
        ],
        WebrootRuntime::default(),
    );
    let response = engine.handle(request("/index.bxs", None)).unwrap();
    assert_eq!(response.status, 500);
    assert_eq!(response.body, b"Internal Server Error");
    let cookie = response
        .headers
        .iter()
        .find(|(key, _)| key == "set-cookie")
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap();
    assert_eq!(
        engine
            .handle(request("/index.bxs", Some(cookie)))
            .unwrap()
            .body,
        b"true|2"
    );
}
#[test]
fn session_end_precedes_application_end_and_clears_the_expired_scope() {
    let root = tempfile::tempdir().unwrap();
    let log = root
        .path()
        .join("lifecycle.log")
        .to_string_lossy()
        .replace('\\', "/")
        .replace('"', "\"\"");
    std::fs::write(root.path().join("Application.bx"),format!("class {{this.name='end';this.sessionManagement=true;this.sessionTimeout=createTimeSpan(0,0,0,0,50);function onSessionStart(id){{session.value='typed';}}function onSessionEnd(scope,app){{fileAppend(\"{log}\",scope.value & '|');}}function onApplicationEnd(scope){{fileAppend(\"{log}\",'app:' & structCount(scope.old));}}}}" )).unwrap();
    std::fs::write(
        root.path().join("index.bxs"),
        "application.old=session;writeOutput(session.value);",
    )
    .unwrap();
    let engine = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().into()),
        WebrootConfig::default(),
    );
    assert_eq!(
        engine.handle(request("/index.bxs", None)).unwrap().body,
        b"typed"
    );
    drop(engine);
    assert_eq!(
        std::fs::read_to_string(root.path().join("lifecycle.log")).unwrap(),
        "typed|app:0"
    );
}
