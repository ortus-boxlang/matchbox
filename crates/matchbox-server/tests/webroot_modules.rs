#![cfg(not(target_family = "wasm"))]
use matchbox_server::webroot_core::{
    FileSystemAssetStore, WebrootConfig, WebrootEngine, WebrootRequest, WebrootRuntime,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};
fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}
fn request(path: &str) -> WebrootRequest {
    WebrootRequest {
        method: "GET".into(),
        path: path.into(),
        ..Default::default()
    }
}
fn runtime(root: &Path) -> anyhow::Result<WebrootRuntime> {
    WebrootRuntime::from_project(root.to_path_buf(), Some(root.join("home")), None)
}
fn engine(root: &Path, runtime: WebrootRuntime) -> WebrootEngine<FileSystemAssetStore> {
    WebrootEngine::with_runtime(
        FileSystemAssetStore::new(root.to_path_buf()),
        WebrootConfig::default(),
        runtime,
    )
}
fn literal(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace('"', "\"\"")
}

#[test]
fn modules_survive_requests_gc_and_source_removal_with_classes_members_and_components() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write(
        root,
        "boxlang_modules/greeter/ModuleConfig.bx",
        "class { function configure() { settings={prefix:'Hello:', count:0}; } }",
    );
    write(
        root,
        "boxlang_modules/greeter/bifs/ModuleGreet.bx",
        "@BoxMember({string:{name:'moduleGreet',objectArgument:'name'}}) class { function invoke(name) { moduleRecord.settings.count=moduleRecord.settings.count+1; return moduleRecord.settings.prefix & name & ':' & moduleRecord.settings.count; } }",
    );
    write(
        root,
        "boxlang_modules/greeter/bifs/WebMarker.bx",
        "class { function invoke() { return getHTTPRequestData().headers['x-test']; } }",
    );
    write(
        root,
        "boxlang_modules/greeter/models/Counter.bx",
        "class { function init(value) { variables.value=value; return this; } function answer() { return variables.value; } }",
    );
    write(
        root,
        "boxlang_modules/greeter/components/Wrap.bx",
        "@AllowsBody(true) class { function invoke(context, attributes, body, executionState) { var buffer=newBuffer(); var result=processBody(context,body,buffer); writeOutput('[' & buffer.toString() & ']'); return result; } }",
    );
    write(
        root,
        "boxlang.json",
        r#"{"modules":{"greeter":{"settings":{"prefix":"Hi:"}}}}"#,
    );
    write(
        root,
        "Application.bx",
        "class { this.name='module-app'; function onApplicationStart() { application.counter=new bxModules.greeter.models.Counter(42); } }",
    );
    write(
        root,
        "index.bxs",
        "bx:wrap { writeOutput('X'.moduleGreet() & '|' & application.counter.answer() & '|' & webMarker()); } ",
    );
    let runtime = runtime(root).unwrap();
    let engine = engine(root, runtime.clone());
    // Runtime catalogs, not the original descriptor/class files, own the live callables.
    fs::remove_dir_all(root.join("boxlang_modules")).unwrap();
    for count in 1..=20 {
        let mut req = request("/index.bxs");
        req.headers.insert("x-test".into(), count.to_string());
        assert_eq!(
            engine.handle(req).unwrap().body,
            format!("[Hi:X:{count}|42|{count}]").as_bytes()
        );
    }
}
#[test]
fn startup_failure_unloads_partial_descriptors_in_reverse_order_preserving_original_error() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let log = root.join("life.txt");
    let path = literal(&log);
    for (name, deps, fail) in [("a", "['b']", false), ("b", "[]", false), ("c", "[]", true)] {
        write(
            root,
            &format!("boxlang_modules/{name}/ModuleConfig.bx"),
            &format!(
                "class {{ this.dependencies={deps}; function onLoad() {{ fileAppend(\"{path}\",\"{}\"); {} }} function onUnload() {{ fileAppend(\"{path}\",\"{name}\"); {} }} }}",
                name.to_uppercase(),
                if fail { "throw 'startup-failure';" } else { "" },
                if name == "a" {
                    "throw 'unload-failure';"
                } else {
                    ""
                }
            ),
        );
    }
    let error = runtime(root).err().expect("startup must fail").to_string();
    assert!(error.contains("startup-failure"), "{error}");
    assert!(!error.contains("unload-failure"), "{error}");
    assert_eq!(fs::read_to_string(log).unwrap(), "BACcab");
}
#[test]
fn runtime_home_is_lower_priority_than_project_packages_and_disabled_packages_are_not_parsed() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write(root, "boxlang_modules/greeter/ModuleConfig.bx", "class {}");
    write(
        root,
        "boxlang_modules/greeter/bifs/Selected.bx",
        "class { function invoke() { return 'project'; } }",
    );
    write(
        root,
        "home/modules/greeter/ModuleConfig.bx",
        "not valid BoxLang",
    );
    write(root, "home/modules/extra/ModuleConfig.bx", "class {}");
    write(
        root,
        "home/modules/extra/bifs/HomeExtra.bx",
        "class { function invoke() { return 'home'; } }",
    );
    write(
        root,
        "boxlang_modules/disabled/ModuleConfig.bx",
        "not valid BoxLang",
    );
    write(
        root,
        "boxlang.json",
        r#"{"modules":{"disabled":{"enabled":false}}}"#,
    );
    write(
        root,
        "index.bxs",
        "writeOutput(selected() & '|' & homeExtra() & '|' & structCount(getModuleList()));",
    );
    let runtime = runtime(root).unwrap();
    assert!(
        runtime
            .module_paths()
            .unwrap()
            .contains(&root.join("home/modules").canonicalize().unwrap())
    );
    assert_eq!(
        engine(root, runtime)
            .handle(request("/index.bxs"))
            .unwrap()
            .body,
        b"project|home|2"
    );
}
#[test]
fn discovery_errors_and_source_only_or_remote_artifacts_fail_before_activation() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write(
        root,
        "boxlang_modules/bad/ModuleConfig.bx",
        "class { this.dependencies=['missing']; }",
    );
    assert!(
        runtime(root)
            .err()
            .unwrap()
            .to_string()
            .contains("missing module")
    );
    write(root, "boxlang_modules/bad/ModuleConfig.bx", "class {}");
    write(
        root,
        "boxlang_modules/bad/matchbox/Cargo.toml",
        "not a real Rust project",
    );
    assert!(
        runtime(root)
            .err()
            .unwrap()
            .to_string()
            .contains("Source-only native")
    );
    write(root, "boxlang_modules/bad/box.json", &serde_json::json!({"matchbox":{"artifacts":{(matchbox_vm::bundle::TARGET):{"url":"https://127.0.0.1:9/no-download", "sha256":"0".repeat(64), "abi":1}}}}).to_string());
    assert!(
        runtime(root)
            .err()
            .unwrap()
            .to_string()
            .contains("remote downloads are not allowed")
    );
}
#[test]
fn detached_module_startup_work_is_rejected_before_accepting_requests_and_unloads() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let log = root.join("unloaded.txt");
    write(
        root,
        "boxlang_modules/background/ModuleConfig.bx",
        &format!(
            "class {{ function onLoad() {{ runAsync(function() {{ sleep(10000); }}); }} function onUnload() {{ fileAppend(\"{}\",\"U\"); }} }}",
            literal(&log)
        ),
    );
    let error = runtime(root)
        .err()
        .expect("background startup must not create an unusable server")
        .to_string();
    assert!(error.contains("background work"), "{error}");
    assert_eq!(fs::read_to_string(log).unwrap(), "U");
}

fn library() -> &'static PathBuf {
    static LIBRARY: OnceLock<PathBuf> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let target = root.join("target/plugin-fixture");
        let output = Command::new(env!("CARGO"))
            .args(["build", "--manifest-path"])
            .arg(root.join("tests/native_plugins/Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        target.join("debug").join(format!(
            "{}matchbox_plugin_fixture{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ))
    })
}
#[test]
fn prebuilt_native_objects_and_java_facades_work_across_http_namespaces_without_java() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let bytes = fs::read(library()).unwrap();
    write(
        root,
        "boxlang_modules/native/ModuleConfig.bx",
        "class { function configure() { classProviders=[{name:'java:example.Counter',provider:fixtureClass}]; } }",
    );
    fs::write(root.join("boxlang_modules/native/plugin.bin"), &bytes).unwrap();
    write(root, "boxlang_modules/native/box.json", &serde_json::json!({"matchbox":{"artifacts":{(matchbox_vm::bundle::TARGET):{"path":"plugin.bin", "sha256":matchbox_vm::bundle::hash(&bytes), "abi":1}}}}).to_string());
    write(
        root,
        "Application.bx",
        "class { function onApplicationStart() { application.object=fixtureNew(32); application.counter=createObject('java','example.Counter'); application.initialized=application.counter.init(41); } }",
    );
    write(
        root,
        "index.bxs",
        "writeOutput(application.object.info().value & '|' & application.initialized.info().value & '|' & application.counter.answer());",
    );
    let engine = engine(root, runtime(root).unwrap());
    for _ in 0..20 {
        assert_eq!(
            engine.handle(request("/index.bxs")).unwrap().body,
            b"32|41|42"
        );
    }
}
#[test]
fn module_component_body_abort_preserves_http_control_flow() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write(root, "boxlang_modules/wrapper/ModuleConfig.bx", "class {}");
    write(
        root,
        "boxlang_modules/wrapper/components/Wrap.bx",
        "@AllowsBody(true) class { function invoke(context,attributes,body,executionState) { return processBody(context,body,newBuilder()); } }",
    );
    write(
        root,
        "index.bxs",
        "try { bx:wrap { location('/next'); writeOutput('late'); } } catch(any e) { writeOutput('caught'); } finally { writeOutput('cleanup'); }",
    );
    let response = engine(root, runtime(root).unwrap())
        .handle(request("/index.bxs"))
        .unwrap();
    assert_eq!(response.status, 302);
    assert_eq!(response.body, b"cleanup");
    assert!(
        response
            .headers
            .contains(&("location".into(), "/next".into()))
    );
}
