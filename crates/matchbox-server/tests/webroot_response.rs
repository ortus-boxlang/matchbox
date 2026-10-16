use matchbox_server::webroot_core::{
    EmbeddedAssetStore, FileSystemAssetStore, WebrootConfig, WebrootEngine, WebrootRequest,
};
fn request(path: &str) -> WebrootRequest {
    WebrootRequest {
        path: path.into(),
        method: "GET".into(),
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
fn redirect_bypasses_catch_runs_finally_and_does_not_poison_later_requests() {
    let engine = engine(&[
        (
            "index.bxs",
            "try { location(url='/next',statusCode=307); } catch(any e) { writeOutput('caught'); } finally { writeOutput('cleanup'); } writeOutput('late');",
        ),
        ("ok.bxs", "writeOutput('next');"),
    ]);
    let result = engine.handle(request("/index.bxs")).unwrap();
    assert_eq!(result.status, 307);
    assert!(
        result
            .headers
            .contains(&("location".into(), "/next".into()))
    );
    assert_eq!(result.body, b"cleanup");
    assert_eq!(engine.handle(request("/ok.bxs")).unwrap().body, b"next");
}
#[test]
fn page_abort_stops_only_the_included_template_and_still_runs_request_end() {
    let engine = engine(&[
        (
            "Application.bx",
            "class { function onAbort(target) { writeOutput('BAD'); } function onRequestEnd(target) { writeOutput('|end'); } }",
        ),
        (
            "index.bxs",
            "writeOutput('before|'); p='piece.bxs'; include p; writeOutput('|after');",
        ),
        (
            "piece.bxs",
            "writeOutput('piece'); abort type='page' showError='ignored'; writeOutput('late');",
        ),
    ]);
    assert_eq!(
        engine.handle(request("/index.bxs")).unwrap().body,
        b"before|piece|after|end"
    );
}
#[test]
fn content_variable_preserves_binary_bytes_and_uses_abort_not_request_end() {
    let engine = engine(&[
        (
            "Application.bx",
            "class { function onAbort(target) { writeOutput('|abort'); } function onRequestEnd(target) { writeOutput('BAD'); } }",
        ),
        (
            "binary.bxs",
            "writeOutput('prefix'); bx:content variable=toBinary('AP8B') reset=false; writeOutput('late');",
        ),
        (
            "text.bxs",
            "writeOutput('discard'); bx:content variable='text'; writeOutput('late');",
        ),
        (
            "empty.bxs",
            "bx:content variable=toBinary(''); writeOutput('late');",
        ),
        (
            "explicit.bxs",
            "bx:content type='text/html'; bx:content variable=toBinary('AP8B');",
        ),
    ]);
    let result = engine.handle(request("/binary.bxs")).unwrap();
    assert_eq!(result.body, b"\x00\xff\x01prefix|abort");
    assert!(
        result
            .headers
            .contains(&("content-type".into(), "application/octet-stream".into()))
    );
    assert_eq!(
        engine.handle(request("/text.bxs")).unwrap().body,
        b"text|abort"
    );
    assert_eq!(
        engine.handle(request("/empty.bxs")).unwrap().body,
        b"|abort"
    );
    assert!(
        engine
            .handle(request("/explicit.bxs"))
            .unwrap()
            .headers
            .contains(&("content-type".into(), "text/html".into()))
    );
}
#[test]
fn file_content_resolves_relative_and_mount_paths_and_deletes_only_after_a_successful_read() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("sub")).unwrap();
    std::fs::write(root.path().join("payload.bin"), [0, 255, 1]).unwrap();
    std::fs::write(
        root.path().join("sub/index.bxs"),
        "writeOutput('discard'); bx:content file='/payload.bin'; writeOutput('late');",
    )
    .unwrap();
    std::fs::write(
        root.path().join("delete.bxs"),
        "bx:content file='payload.bin' deleteFile=true;",
    )
    .unwrap();
    std::fs::write(root.path().join("bad.bxs"), "try { bx:content file='payload.bin' deleteFile=true type=('bad' & chr(13)); } catch(any e) { writeOutput('rejected'); }").unwrap();
    let engine = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().to_path_buf()),
        WebrootConfig::default(),
    );
    assert_eq!(
        engine.handle(request("/bad.bxs")).unwrap().body,
        b"rejected"
    );
    assert!(root.path().join("payload.bin").exists());
    let result = engine.handle(request("/sub/index.bxs")).unwrap();
    assert_eq!(result.body, [0, 255, 1]);
    assert!(
        result
            .headers
            .contains(&("content-type".into(), "application/octet-stream".into()))
    );
    assert_eq!(
        engine.handle(request("/delete.bxs")).unwrap().body,
        [0, 255, 1]
    );
    assert!(!root.path().join("payload.bin").exists());
}
#[test]
fn abort_causes_run_abort_then_error_and_an_abort_in_error_is_ignored() {
    let engine = engine(&[
        (
            "Application.bx",
            "class { function onAbort(target) { writeOutput('|abort'); } function onError(error,event) { writeOutput('|error:' & error.message); abort; } function onRequestEnd(target) { writeOutput('BAD'); } }",
        ),
        (
            "index.bxs",
            "writeOutput('before'); bx:abort showError='shown'; writeOutput('late');",
        ),
    ]);
    assert_eq!(
        engine.handle(request("/index.bxs")).unwrap().body,
        b"before|abort|error:shown"
    );
}
#[test]
fn pseudo_constructor_redirect_has_no_listener_hooks_yet() {
    let engine = engine(&[
        (
            "Application.bx",
            "class { location('/login'); function onAbort(target) { writeOutput('BAD'); } }",
        ),
        ("index.bxs", "writeOutput('BAD');"),
    ]);
    let result = engine.handle(request("/index.bxs")).unwrap();
    assert_eq!(result.status, 302);
    assert_eq!(result.body, b"");
    assert!(
        result
            .headers
            .contains(&("location".into(), "/login".into()))
    );
}
#[test]
fn finally_return_can_override_abort_and_ordinary_lookalike_exceptions_are_catchable() {
    let engine = engine(&[(
        "index.bxs",
        "function suppress() { try { location('/next'); } finally { return 7; } } writeOutput(suppress()); try { throw {type:'AbortException', message:'ordinary'}; } catch(any e) { writeOutput('|caught'); } writeOutput('|continued');",
    )]);
    let result = engine.handle(request("/index.bxs")).unwrap();
    assert_eq!(result.status, 302);
    assert_eq!(result.body, b"7|caught|continued");
}
