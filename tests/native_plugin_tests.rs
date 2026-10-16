#![cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
use matchbox_vm::{
    bundle::{Artifact, SharedLibrary, TARGET, hash},
    types::{BxVM, BxValue},
    vm::VM,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

fn library() -> &'static PathBuf {
    static LIBRARY: OnceLock<PathBuf> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
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
fn artifact() -> Artifact {
    let path = library();
    let bytes = fs::read(path).unwrap();
    Artifact {
        target: TARGET.into(),
        abi: 1,
        filename: path.file_name().unwrap().to_string_lossy().into_owned(),
        sha256: hash(&bytes),
        bytes,
        libraries: vec![],
    }
}
fn cli(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .current_dir(dir)
        .env("MATCHBOX_NATIVE_CACHE", dir.join("cache"))
        .args(args)
        .output()
        .unwrap()
}
fn success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn package(dir: &Path) -> PathBuf {
    let module = dir.join("module");
    fs::create_dir_all(module.join("bifs")).unwrap();
    fs::write(
        module.join("ModuleConfig.bx"),
        r#"
class {
    property name="moduleRecord";
    function configure() {
        assert moduleRecord.name == "module";
        assert moduleRecord.invocationPath == "bxModules.module";
        assert !moduleRecord.isActivated();
        assert boxRuntime.getModuleService().getModuleRecord("module") == moduleRecord;
        settings = { width: 1, nested: { first: 1, second: 2 } };
    }
    function onLoad() {
        assert getModuleSettings("module").width == 32;
        assert moduleRecord.settings.width == 32;
        assert moduleRecord.asStruct().name == "module";
        assert !moduleRecord.isActivated();
        println("loaded");
    }
    function onUnload() { println("unloaded"); }
}
"#,
    )
    .unwrap();
    fs::write(
        module.join("bifs/SharedDouble.bx"),
        r#"class {
            function invoke(required numeric value) {
                assert moduleRecord.name == "module";
                assert moduleRecord.settings.width == 32;
                assert functionService.hasGlobalFunction("SharedDouble");
                assert !functionService.hasGlobalFunction("ordinaryUserFunction");
                assert boxRuntime.getFunctionService() == functionService;
                assert arrayFindNoCase(functionService.getGlobalFunctionNames(), "SharedDouble") > 0;
                return value * 2;
            }
        }"#,
    )
    .unwrap();
    fs::write(
        module.join("bifs/GenericInfo.bx"),
        r#"
@BoxMember({any: {name: "info"}})
class { function invoke(required any value) { return {value:-9}; } }
"#,
    )
    .unwrap();
    let artifact = artifact();
    fs::write(module.join(&artifact.filename), &artifact.bytes).unwrap();
    fs::write(module.join("box.json"), serde_json::json!({"slug":"fixture","type":"boxlang-modules","matchbox":{"artifacts":{TARGET:{"path":artifact.filename,"sha256":artifact.sha256,"abi":1}}}}).to_string()).unwrap();
    fs::write(
        dir.join("boxlang.json"),
        r#"{"modules":{"module":{"settings":{"width":32,"nested":{"second":9}}}}}"#,
    )
    .unwrap();
    module
}

#[test]
fn plugin_values_errors_and_object_lifetime() {
    let mut vm = VM::new();
    unsafe { matchbox_vm::plugins::load(&mut vm, library()) }.unwrap();
    // A nested ordinary BIF must not release the outer native invocation's GC guard.
    vm.gc_suspended = true;
    vm.call_function("isNull", vec![BxValue::new_null()], None)
        .unwrap();
    assert!(vm.gc_suspended);
    vm.gc_suspended = false;
    let object = vm.call_function("fixtureNew", vec![], None).unwrap();
    let result = vm.call_method_value(object, "info", vec![]).unwrap();
    let id = result.as_gc_id().unwrap();
    assert_eq!(vm.struct_get(id, "value").as_number(), 42.0);
    // Keep the callback globals and returned struct alive while collecting the temporary plugin object.
    let mut roots = vm.global_values.clone();
    roots.push(result);
    vm.heap.collect(&roots);
    assert_eq!(vm.struct_get(id, "value").as_number(), 42.0);
    assert!(
        vm.call_function("fixtureDrops", vec![], None)
            .unwrap()
            .as_number()
            >= 1.0
    );
    assert!(
        vm.call_function("fixturePanic", vec![], None)
            .unwrap_err()
            .to_string()
            .contains("panicked")
    );
    assert!(vm.call_function("fixtureBadHandle", vec![], None).is_err());
    assert!(
        vm.call_function("fixtureInfo", vec![BxValue::new_number(1.0)], None)
            .is_err()
    );
    assert!(vm.call_function("fixtureInfo", vec![], None).is_err());
    let object = vm.call_function("fixtureNew", vec![], None).unwrap();
    vm.insert_global("retainedObject".into(), object);
    // A separate provider instance cannot interpret another library's private payload.
    unsafe { matchbox_vm::plugins::load(&mut vm, library()) }.unwrap();
    assert!(vm.call_function("fixtureInfo", vec![object], None).is_err());
    assert!(vm.call_method_value(object, "info", vec![]).is_ok());
}

#[test]
fn retained_values_survive_gc_and_release_without_a_live_vm() {
    let mut vm = VM::new();
    unsafe { matchbox_vm::plugins::load(&mut vm, library()) }.unwrap();
    let payload = vm.struct_new();
    let text = vm.string_new("retained text".into());
    vm.struct_set(payload, "text", BxValue::new_ptr(text));
    vm.call_function("fixtureStore", vec![BxValue::new_ptr(payload)], None)
        .unwrap();
    for _ in 0..16 {
        vm.collect_garbage_now();
    }
    assert_eq!(
        vm.call_function("fixtureRecall", vec![], None).unwrap(),
        BxValue::new_ptr(payload)
    );
    assert_eq!(
        vm.to_string(vm.struct_get(payload, "text")),
        "retained text"
    );
    let changed = vm.string_new("changed".into());
    vm.struct_set(payload, "text", BxValue::new_ptr(changed));
    for _ in 0..8 {
        vm.collect_garbage_now();
    }
    assert_eq!(vm.to_string(vm.struct_get(payload, "text")), "changed");
    vm.call_function("fixtureStore", vec![BxValue::new_number(42.0)], None)
        .unwrap();
    for _ in 0..8 {
        vm.collect_garbage_now();
    }
    assert!(vm.heap.get_opt(payload).is_none());
    assert!(vm.heap.get_opt(changed).is_none());
    assert_eq!(
        vm.call_function("fixtureRecall", vec![], None)
            .unwrap()
            .as_number(),
        42.0
    );
    unsafe { matchbox_vm::plugins::load(&mut vm, library()) }.unwrap();
    assert!(
        vm.call_function("fixtureRecall", vec![], None)
            .unwrap_err()
            .to_string()
            .contains("another plugin provider")
    );
    vm.call_function("fixtureClear", vec![], None).unwrap();
    assert!(vm.call_function("fixtureRecall", vec![], None).is_err());

    // The containing native object owns the root and releases it without a call context.
    let payload = vm.struct_new();
    let holder = vm
        .call_function("fixtureHold", vec![BxValue::new_ptr(payload)], None)
        .unwrap();
    vm.insert_global("heldValue".into(), holder);
    for _ in 0..16 {
        vm.collect_garbage_now();
    }
    assert_eq!(
        vm.call_method_value(holder, "get", vec![]).unwrap(),
        BxValue::new_ptr(payload)
    );
    vm.insert_global("heldValue".into(), BxValue::new_null());
    for _ in 0..16 {
        vm.collect_garbage_now();
    }
    assert!(vm.heap.get_opt(payload).is_none());

    let payload = vm.struct_new();
    vm.call_function("fixtureStore", vec![BxValue::new_ptr(payload)], None)
        .unwrap();
    drop(vm);
    let mut other_vm = VM::new();
    unsafe { matchbox_vm::plugins::load(&mut other_vm, library()) }.unwrap();
    assert!(
        other_vm
            .call_function("fixtureRecall", vec![], None)
            .unwrap_err()
            .to_string()
            .contains("another VM")
    );
    other_vm
        .call_function("fixtureClear", vec![], None)
        .unwrap();
    other_vm
        .call_function("fixtureStore", vec![BxValue::new_null()], None)
        .unwrap();
    assert!(
        other_vm
            .call_function("fixtureRecall", vec![], None)
            .unwrap()
            .is_null()
    );
    other_vm
        .call_function("fixtureClear", vec![], None)
        .unwrap();
}

#[test]
fn retained_callback_keeps_its_captured_receiver_alive() {
    let mut vm = VM::new();
    unsafe { matchbox_vm::plugins::load(&mut vm, library()) }.unwrap();
    let source = r#"
class Receiver {
    this.label = "captured receiver";
    function callback() { return () => this.label; }
}
receiver = new Receiver();
fixtureStore(receiver.callback());
receiver = null;
"#;
    let ast = matchbox_compiler::parser::parse(source, Some("retained-callback.bxs")).unwrap();
    let chunk = matchbox_compiler::compiler::Compiler::new("retained-callback.bxs")
        .compile(&ast, source)
        .unwrap();
    vm.interpret_sync(chunk).unwrap();
    for _ in 0..16 {
        vm.collect_garbage_now();
    }
    let callback = vm.call_function("fixtureRecall", vec![], None).unwrap();
    let result = vm
        .call_function_by_value(
            &callback,
            vec![],
            std::rc::Rc::new(std::cell::RefCell::new(matchbox_vm::Chunk::default())),
        )
        .unwrap();
    assert_eq!(vm.to_string(result), "captured receiver");
    vm.call_function("fixtureClear", vec![], None).unwrap();
}

#[test]
fn retained_lexical_scopes_survive_mutation_collection_and_release() {
    let mut vm = VM::new();
    unsafe { matchbox_vm::plugins::load(&mut vm, library()) }.unwrap();
    let source = r#"
function retainState(prefix = "scope:") {
    var payload = {text: "original"};
    return fixtureHold(function(text = "changed") {
        payload = {text: prefix & text};
        return payload;
    });
}
holder = retainState();
retainState = null;
"#;
    let ast = matchbox_compiler::parser::parse(source, None).unwrap();
    let chunk = matchbox_compiler::compiler::Compiler::new("lexical-gc.bxs").compile(&ast, source).unwrap();
    vm.interpret_sync(chunk).unwrap();
    for _ in 0..16 { vm.collect_garbage_now(); }
    let holder = vm.get_global("holder").unwrap();
    let callback = vm.call_method_value(holder, "get", vec![]).unwrap();
    let payload = vm.call_function_value(callback, vec![], None).unwrap();
    for _ in 0..16 { vm.collect_garbage_now(); }
    assert_eq!(vm.to_string(vm.struct_get(payload.as_gc_id().unwrap(), "text")), "scope:changed");
    let next = BxValue::new_ptr(vm.string_new("next".into()));
    let replacement = vm.call_function_value(callback, vec![next], None).unwrap();
    for _ in 0..16 { vm.collect_garbage_now(); }
    assert_eq!(vm.to_string(vm.struct_get(replacement.as_gc_id().unwrap(), "text")), "scope:next");
    assert!(vm.heap.get_opt(payload.as_gc_id().unwrap()).is_none());
    vm.insert_global("holder".into(), BxValue::new_null());
    for _ in 0..16 { vm.collect_garbage_now(); }
    assert!(vm.heap.get_opt(callback.as_gc_id().unwrap()).is_none());
    assert!(vm.heap.get_opt(replacement.as_gc_id().unwrap()).is_none());
}

#[test]
fn module_contract_direct_bytecode_and_standalone() {
    let dir = tempfile::tempdir().unwrap();
    package(dir.path());
    fs::write(
        dir.path().join("app.bxs"),
        r#"
function ordinaryUserFunction() { return "user function"; }
assert sharedDouble(value=4) == 8;
assert fixtureCreate().info().value == 42;
assert (3).info().value == -9;
object = fixtureNew(VALUE=32);
assert fixtureInfo(object).value == 32;
assert object.info().value == 32;
fixtureInfo = function(object) { return {value:-1}; };
assert object.info().value == 32;
assert getModuleSettings("module").width == 32;
assert getModuleSettings("module").nested.first == 1;
assert getModuleSettings("module").nested.second == 9;
println("application");
"#,
    )
    .unwrap();
    let direct = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&direct);
    assert_eq!(
        String::from_utf8_lossy(&direct.stdout).trim(),
        "loaded\napplication\nunloaded"
    );
    // Build/packaging must not execute either lifecycle hook.
    let built = cli(
        dir.path(),
        &[
            "--build", "app.bxs", "--module", "module", "--output", "app.bxb",
        ],
    );
    success(&built);
    assert!(!String::from_utf8_lossy(&built.stdout).contains("loaded"));
    let bytecode = cli(dir.path(), &["app.bxb"]);
    success(&bytecode);
    let built = cli(
        dir.path(),
        &[
            "--target",
            "native",
            "app.bxs",
            "--module",
            "module",
            "--output",
            "application",
        ],
    );
    success(&built);
    assert!(!String::from_utf8_lossy(&built.stdout).contains("loaded"));
    fs::remove_dir_all(dir.path().join("module")).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    let execute = || {
        Command::new(dir.path().join("application"))
            .current_dir(dir.path())
            .env("MATCHBOX_NATIVE_CACHE", dir.path().join("cache"))
            .output()
            .unwrap()
    };
    let first = execute();
    success(&first);
    let second = execute();
    success(&second);
    assert_eq!(first.stdout, direct.stdout);
    assert_eq!(second.stdout, first.stdout);
}

#[cfg(target_os = "linux")]
fn linked_library() -> &'static (PathBuf, PathBuf) {
    static LIBRARY: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let target = root.join("target/plugin-fixture-libraries");
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "--features",
                "bundled-libraries",
                "--message-format=json",
                "--manifest-path",
            ])
            .arg(root.join("tests/native_plugins/Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .unwrap();
        success(&output);
        let out = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| {
                let message: serde_json::Value = serde_json::from_str(line).ok()?;
                (message["reason"] == "build-script-executed"
                    && message["package_id"]
                        .as_str()?
                        .contains("matchbox-plugin-fixture"))
                .then(|| PathBuf::from(message["out_dir"].as_str().unwrap()))
            })
            .expect("Fixture build did not report its native library directory");
        (target.join("debug/libmatchbox_plugin_fixture.so"), out)
    })
}

#[cfg(target_os = "linux")]
#[test]
fn bundled_libraries_resolve_transitive_dependencies_without_the_source_package() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("backends")).unwrap();
    fs::create_dir_all(module.join("vendor/deep")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    let (plugin, dependencies) = linked_library();
    let plugin = fs::read(plugin).unwrap();
    fs::write(module.join("backends/provider.bin"), &plugin).unwrap();
    let mut libraries = Vec::new();
    for name in ["codec", "leaf"] {
        let filename = format!("libmatchbox_fixture_{name}.so");
        let bytes = fs::read(dependencies.join(&filename)).unwrap();
        let path = format!("vendor/deep/{name}.bin");
        fs::write(module.join(&path), &bytes).unwrap();
        if name == "leaf" {
            let alias = format!("vendor/deep/{filename}");
            std::os::unix::fs::symlink("leaf.bin", module.join(&alias)).unwrap();
            libraries.push(serde_json::json!({"path":alias,"sha256":hash(&bytes)}));
        } else {
            libraries
                .push(serde_json::json!({"path":path,"filename":filename,"sha256":hash(&bytes)}));
        }
    }
    let metadata = serde_json::json!({"matchbox":{"artifacts":{TARGET:{
        "path":"backends/provider.bin","filename":"provider.so","sha256":hash(&plugin),"abi":1,"libraries":libraries
    }}}});
    fs::write(module.join("box.json"), metadata.to_string()).unwrap();
    fs::write(
        dir.path().join("app.bxs"),
        "assert fixtureLibraryValue() == 42; println('bundled libraries OK');",
    )
    .unwrap();
    let mut missing = metadata.clone();
    missing["matchbox"]["artifacts"][TARGET]["libraries"] = serde_json::json!([]);
    fs::write(module.join("box.json"), missing.to_string()).unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("libmatchbox_fixture_codec.so"));
    fs::write(module.join("box.json"), metadata.to_string()).unwrap();
    let direct = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&direct);
    assert_eq!(
        String::from_utf8_lossy(&direct.stdout).trim(),
        "bundled libraries OK"
    );
    success(&cli(
        dir.path(),
        &[
            "--build", "app.bxs", "--module", "module", "--output", "app.bxb",
        ],
    ));
    success(&cli(
        dir.path(),
        &[
            "--target",
            "native",
            "app.bxs",
            "--module",
            "module",
            "--output",
            "application",
        ],
    ));
    fs::remove_dir_all(&module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    let bytecode = cli(dir.path(), &["app.bxb"]);
    success(&bytecode);
    assert_eq!(bytecode.stdout, direct.stdout);
    let standalone = Command::new(dir.path().join("application"))
        .current_dir(dir.path())
        .env("MATCHBOX_NATIVE_CACHE", dir.path().join("fresh-cache"))
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .output()
        .unwrap();
    success(&standalone);
    assert_eq!(standalone.stdout, direct.stdout);
}

#[test]
fn cache_is_atomic_verified_and_rejects_tampering() {
    let dir = tempfile::tempdir().unwrap();
    let mut artifact = artifact();
    artifact.libraries.push(SharedLibrary {
        filename: "codec.so".into(),
        sha256: hash(b"library"),
        bytes: b"library".to_vec(),
    });
    let paths = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| artifact.extract(dir.path()).unwrap()))
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(paths.iter().all(|p| p == &paths[0]));
    assert_eq!(
        fs::read(paths[0].parent().unwrap().join("codec.so")).unwrap(),
        b"library"
    );
    let timestamp = fs::metadata(&paths[0]).unwrap().modified().unwrap();
    assert_eq!(artifact.extract(dir.path()).unwrap(), paths[0]);
    assert_eq!(
        fs::metadata(&paths[0]).unwrap().modified().unwrap(),
        timestamp
    );
    fs::write(&paths[0], b"tampered").unwrap();
    assert!(artifact.extract(dir.path()).is_err());
    let mut wrong = artifact.clone();
    wrong.target = "wrong-target".into();
    assert!(wrong.extract(dir.path()).is_err());
    let mut wrong = artifact.clone();
    wrong.abi = 999;
    assert!(wrong.extract(dir.path()).is_err());
    let mut wrong = artifact;
    wrong.filename = "../escape.so".into();
    assert!(wrong.extract(dir.path()).is_err());
}

#[test]
fn library_groups_verify_cached_dependencies_and_change_identity_with_their_contents() {
    let dir = tempfile::tempdir().unwrap();
    let mut artifact = artifact();
    for filename in ["codec.so", "helper.so"] {
        artifact.libraries.push(SharedLibrary {
            filename: filename.into(),
            sha256: hash(filename.as_bytes()),
            bytes: filename.as_bytes().to_vec(),
        });
    }
    let original = artifact.extract(dir.path()).unwrap();
    let mut reordered = artifact.clone();
    reordered.libraries.reverse();
    assert_eq!(reordered.extract(dir.path()).unwrap(), original);
    let mut updated = artifact.clone();
    updated.libraries[0].bytes = b"new dependency".to_vec();
    updated.libraries[0].sha256 = hash(&updated.libraries[0].bytes);
    assert_ne!(
        updated.extract(dir.path()).unwrap().parent(),
        original.parent()
    );
    let mut renamed = artifact.clone();
    renamed.libraries[0].filename = "renamed.so".into();
    assert_ne!(
        renamed.extract(dir.path()).unwrap().parent(),
        original.parent()
    );
    fs::write(original.parent().unwrap().join("codec.so"), b"tampered").unwrap();
    assert!(
        artifact
            .extract(dir.path())
            .unwrap_err()
            .to_string()
            .contains("failed verification")
    );
    let mut invalid = artifact.clone();
    invalid.libraries[0].bytes.push(1);
    assert!(invalid.validate().is_err());
    invalid = artifact.clone();
    invalid.libraries[0].filename = artifact.filename.clone();
    assert!(invalid.validate().is_err());
    for filename in [
        "../escape.so",
        r"..\escape.dll",
        "C:escape.dll",
        "bad\0name",
        ".",
    ] {
        invalid.libraries[0].filename = filename.into();
        assert!(invalid.validate().is_err(), "accepted {filename:?}");
    }
    invalid = artifact;
    invalid.target = "x86_64-pc-windows-msvc".into();
    invalid.libraries[0].filename = "HELPER.SO".into();
    assert!(invalid.validate_files().is_err());
    #[cfg(unix)]
    {
        let cache = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), cache.path().join(TARGET)).unwrap();
        assert!(updated.extract(cache.path()).is_err());
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }
}

#[test]
fn dependent_library_metadata_rejects_unverified_or_escaping_files_before_startup() {
    let dir = tempfile::tempdir().unwrap();
    let module = package(dir.path());
    fs::write(module.join("codec.so"), b"library").unwrap();
    fs::write(dir.path().join("outside.so"), b"library").unwrap();
    fs::write(dir.path().join("app.bxs"), "println('application');").unwrap();
    let manifest = module.join("box.json");
    let mut metadata: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    for (library, error) in [
        (
            serde_json::json!({"path":"codec.so","sha256":hash(b"wrong")}),
            "hash does not match",
        ),
        (
            serde_json::json!({"path":"../outside.so","sha256":hash(b"library")}),
            "escapes module package",
        ),
        (
            serde_json::json!({"path":"codec.so","filename":"../escape.so","sha256":hash(b"library")}),
            "Invalid artifact filename",
        ),
        (
            serde_json::json!({"path":"codec.so","filename":artifact().filename,"sha256":hash(b"library")}),
            "Duplicate native artifact filename",
        ),
        (
            serde_json::json!({"path":"codec.so","url":"https://example.invalid/library","sha256":hash(b"library")}),
            "exactly one of path or url",
        ),
        (
            serde_json::json!({"url":"https://example.invalid/library","sha256":hash(b"library")}),
            "require a filename",
        ),
        (
            serde_json::json!({"url":"http://example.invalid/library","filename":"codec.so","sha256":hash(b"library")}),
            "HTTPS",
        ),
    ] {
        metadata["matchbox"]["artifacts"][TARGET]["libraries"] = serde_json::json!([library]);
        fs::write(&manifest, metadata.to_string()).unwrap();
        let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(error), "{stderr}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("loaded"));
    }
}

#[test]
fn missing_capability_and_error_shutdown_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    package(dir.path());
    fs::write(dir.path().join("app.bxs"), "throw 'application failure';").unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("unloaded"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("application failure"));
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .current_dir(dir.path())
        .env("MATCHBOX_FIXTURE_MISSING_CAPABILITY", "1")
        .env("MATCHBOX_NATIVE_CACHE", dir.path().join("cache"))
        .args(["app.bxs", "--module", "module"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Missing required capability"));
}

#[test]
fn computed_descriptor_mappings_are_resolved_only_at_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir(&module).unwrap();
    let alias = dir.path().join("alias.txt");
    fs::write(&alias, "One").unwrap();
    fs::write(module.join("ModuleConfig.bx"), r#"
class {
    this.mapping = "runtime" & fileRead("ALIAS_FILE");
    this.publicMapping = "www";
    function configure() {
        var snapshot = moduleRecord.asStruct();
        assert snapshot.mapping.name == ("/bxModules/runtime" & fileRead("ALIAS_FILE") & "/");
        assert snapshot.invocationPath == ("bxModules.runtime" & fileRead("ALIAS_FILE"));
        assert snapshot.mapping.path == moduleRecord.path;
        assert !snapshot.mapping.external;
        assert snapshot.publicMapping.name == (snapshot.mapping.name & "www/");
        assert snapshot.publicMapping.path == (moduleRecord.path & "/www");
        assert snapshot.publicMapping.external;
        this.mapping = "tooLate";
        println("configured " & moduleRecord.invocationPath);
    }
}
"#.replace("ALIAS_FILE", &alias.to_string_lossy().replace('\\', "/"))).unwrap();
    fs::write(dir.path().join("app.bxs"), "println('mapping contract OK');").unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "configured bxModules.runtimeOne\nmapping contract OK\n");
    // Packaging must not read runtime inputs or execute configure().
    fs::remove_file(&alias).unwrap();
    for args in [
        vec!["--build", "app.bxs", "--module", "module", "--output", "app.bxb"],
        vec!["--target", "native", "app.bxs", "--module", "module", "--output", "application"],
    ] {
        let output = cli(dir.path(), &args); success(&output);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("configured"));
    }
    fs::remove_dir_all(module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    fs::write(&alias, "Two").unwrap();
    for output in [
        cli(dir.path(), &["app.bxb"]),
        Command::new(dir.path().join("application")).current_dir(dir.path()).output().unwrap(),
    ] {
        success(&output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), "configured bxModules.runtimeTwo\nmapping contract OK\n");
    }
}

#[test]
fn struct_descriptor_mappings_honor_prefix_path_visibility_and_defaults() {
    for (definition, mapping, public, invocation, mapping_path, public_path, external, public_external) in [
        (r#"this.mapping = {name:'plain', usePrefix:false, external:true, path:'src'}; this.publicMapping = {name:'/assets', usePrefix:false, external:false, path:'www'};"#,
         "/plain/", "/assets/", "bxModules.plain", "/src", "/www", true, false),
        (r#"this.mapping = {name:' ', usePrefix:'yes', external:'no', path:''}; this.publicMapping = {};"#,
         "/bxModules/module/", "/bxModules/module/", "bxModules.module", "", "", false, true),
        (r#"this.mapping = {name:'renamed'}; this.publicMapping = '';"#,
         "/bxModules/renamed/", "/bxModules/module/public/", "bxModules.renamed", "", "/public", false, true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("module")).unwrap();
        let source = format!(r#"
class {{
    {definition}
    function configure() {{
        var snapshot = moduleRecord.asStruct();
        assert snapshot.mapping.name == '{mapping}';
        assert snapshot.mapping.path == (moduleRecord.path & '{mapping_path}');
        assert snapshot.mapping.external == {external};
        assert snapshot.invocationPath == '{invocation}';
        assert snapshot.publicMapping.name == '{public}';
        assert snapshot.publicMapping.path == (moduleRecord.path & '{public_path}');
        assert snapshot.publicMapping.external == {public_external};
        assert this.mapping.name == listLast('{invocation}', '.');
        println('struct mappings OK');
    }}
}}
"#);
        fs::write(dir.path().join("module/ModuleConfig.bx"), source).unwrap();
        fs::write(dir.path().join("app.bxs"), "").unwrap();
        let output = cli(dir.path(), &["app.bxs", "--module", "module"]); success(&output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), "struct mappings OK\n");
    }
}

#[test]
fn runtime_mappings_resolve_embedded_classes_in_configure_imports_and_constructors() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("models")).unwrap();
    fs::create_dir_all(module.join("public")).unwrap();
    let alias = dir.path().join("alias.txt");
    fs::write(&alias, "One").unwrap();
    fs::write(module.join("ModuleConfig.bx"), r#"
import bxModules.lateOne.Value as MappedValue;
class {
    this.mapping = {name:"late" & fileRead("ALIAS_FILE"), path:"models/../models"};
    this.publicMapping = {name:"publicApi", usePrefix:false, path:"public"};
    function configure() {
        var created = new bxModules.lateOne.Value(value=11);
        assert created.get() == 11;
        println("configured mapped classes");
    }
}
"#.replace("ALIAS_FILE", &alias.to_string_lossy().replace('\\', "/"))).unwrap();
    fs::write(module.join("models/Value.bx"), r#"
class {
    function init(numeric value = 7) { variables.value = value; return this; }
    function get() { return variables.value; }
    static function kind() { return "mapped"; }
}
"#).unwrap();
    fs::write(module.join("public/Exposed.bx"), r#"
class {
    function init() { variables.value = 42; }
    function get() { return variables.value; }
}
"#).unwrap();
    // A class outside the resolved mapping path must not shadow its mapped namesake.
    fs::write(module.join("Value.bx"), "class { function get() { return 99; } }").unwrap();
    fs::write(dir.path().join("app.bxs"), r#"
import bxModules.lateOne.Value as MappedValue;
import publicApi.Exposed;
first = new MappedValue();
second = new bxModules.lateOne.Value(value=13);
assert first.get() == 7;
assert second.get() == 13;
assert MappedValue.kind() == "mapped";
kindReference = MappedValue.kind;
assert kindReference() == "mapped";
try { MappedValue.get(); throw 'non-static call accepted'; }
catch (any error) { assert findNoCase('requires an instance', error.message) > 0; }
visible = new Exposed();
assert visible.get() == 42;
println("mapped class contracts OK");
"#).unwrap();
    let expected = "configured mapped classes\nmapped class contracts OK\n";
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    fs::remove_file(&alias).unwrap();
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module", "--output", "app.bxb"]));
    success(&cli(dir.path(), &["--target", "native", "app.bxs", "--module", "module", "--output", "application"]));
    fs::remove_dir_all(module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    fs::write(&alias, "One").unwrap();
    for output in [
        cli(dir.path(), &["app.bxb"]),
        Command::new(dir.path().join("application")).current_dir(dir.path()).output().unwrap(),
    ] {
        success(&output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}

#[test]
fn embedding_module_classes_does_not_execute_neighboring_script_statements() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("module")).unwrap();
    fs::write(dir.path().join("module/ModuleConfig.bx"), "class {}").unwrap();
    fs::write(dir.path().join("module/Program.bxs"), "class { function get() { return 8; } } throw 'neighboring script executed';").unwrap();
    fs::write(dir.path().join("app.bxs"), "import bxModules.module.Program; target = new Program(); assert target.get() == 8;").unwrap();
    success(&cli(dir.path(), &["app.bxs", "--module", "module"]));
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module"]));
    fs::remove_dir_all(dir.path().join("module")).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    success(&cli(dir.path(), &["app.bxb"]));
}

fn class_provider_package(dir: &Path) -> PathBuf {
    let module = package(dir);
    fs::write(module.join("ModuleConfig.bx"), r#"
import bxModules.module.CounterReference;
class {
    function configure() {
        variables.facade = CounterReference;
        classProviders = [{name:"java:org.matchbox.fixture.Counter", provider:this.reference}];
    }
    function reference() { return variables.facade; }
    function onLoad() { println("provider loaded"); }
    function onUnload() { println("provider unloaded"); }
}
"#).unwrap();
    fs::write(module.join("CounterReference.bx"), r#"
class {
    this.label = "fixture class";
    this.out = fixtureNew(42);
    function init(numeric value = 7) {
        if (value < 0) { throw "fixture constructor rejected value"; }
        println("initialized " & value);
        return fixtureNew(value);
    }
    function answer() { return 42; }
}
"#).unwrap();
    module
}

#[test]
fn module_class_providers_resolve_java_creation_without_initializing_references() {
    let dir = tempfile::tempdir().unwrap();
    class_provider_package(dir.path());
    fs::write(dir.path().join("app.bxs"), r#"
reference = createObject("java", "org.matchbox.fixture.Counter");
assert reference.label == "fixture class";
assert reference.answer() == 42;
println("reference only");
first = reference.init(13);
assert first.info().value == 13;
assert reference.info().value == 13;
assert reference.answer() == 42;
second = new java:org.matchbox.fixture.Counter();
assert second.info().value == 7;
try { new java:org.matchbox.fixture.Counter(-1); throw "constructor accepted invalid value"; }
catch (any error) { assert findNoCase("fixture constructor rejected value", error.message) > 0; }
try { reference.init(-1); throw "constructor accepted invalid value"; }
catch (any error) { assert findNoCase("fixture constructor rejected value", error.message) > 0; }
assert reference.info().value == 13;
println("provider creation OK");
"#).unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "provider loaded\nreference only\ninitialized 13\ninitialized 7\nprovider creation OK\nprovider unloaded\n");
}

#[test]
fn module_class_providers_share_import_class_reference_static_and_property_protocols() {
    let dir = tempfile::tempdir().unwrap();
    let module = class_provider_package(dir.path());
    fs::write(dir.path().join("app.bxs"), r#"
import java:org.matchbox.fixture.Counter as Probe;
import org.matchbox.fixture.Counter as Unprefixed;
assert Probe.label == "fixture class";
assert Probe::answer() == 42;
assert Probe.answer() == 42;
assert Probe.out.info().value == 42;
assert Unprefixed.answer() == 42;
reference = Probe;
assert reference.init(12).info().value == 12;
assert new Probe(14).info().value == 14;
assert new Unprefixed(15).info().value == 15;
assert Probe(16).info().value == 16;
reference.label = "changed";
assert reference.label == "changed";
staticReference = reference.answer;
assert staticReference() == 42;
createObject = function() { throw "new/import used a replaced createObject BIF"; };
assert new java:org.matchbox.fixture.Counter(17).info().value == 17;
assert Probe.answer() == 42;
println("provider protocols OK");
"#).unwrap();
    let expected = "provider loaded\ninitialized 12\ninitialized 14\ninitialized 15\ninitialized 16\ninitialized 17\nprovider protocols OK\nprovider unloaded\n";
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module", "--output", "app.bxb"]));
    success(&cli(dir.path(), &["--target", "native", "app.bxs", "--module", "module", "--output", "application"]));
    fs::remove_dir_all(module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    for output in [cli(dir.path(), &["app.bxb"]), Command::new(dir.path().join("application")).env("MATCHBOX_NATIVE_CACHE", dir.path().join("fresh-cache")).current_dir(dir.path()).output().unwrap()] {
        success(&output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}

#[test]
fn module_class_provider_errors_and_frozen_class_identities_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    class_provider_package(dir.path());
    fs::write(dir.path().join("app.bxs"), r#"
reference = createObject("java", "org.matchbox.fixture.Counter");
assert reference["label"] == "fixture class";
reference["label"] = "indexed";
assert reference.label == "indexed";
try { reference.init(value=9); throw "named constructor accepted"; }
catch (any error) { assert findNoCase("Cannot use named arguments", error.message) > 0; }
try { reference.answer(value=9); throw "named method accepted"; }
catch (any error) { assert findNoCase("cannot be called with named arguments", error.message) > 0; }
try { reference.missing = 1; throw "missing field assignment accepted"; }
catch (any error) { assert findNoCase("missing or read-only", error.message) > 0; }
try { reference.missing(); throw "missing method accepted"; }
catch (any error) { assert findNoCase("not found", error.message) > 0; }
try { createObject("java", "org.matchbox.fixture.Missing"); throw "missing class accepted"; }
catch (any error) { assert findNoCase("Java class not provided", error.message) > 0; }
try { createObject("java", "org.matchbox.fixture.counter"); throw "case-insensitive class accepted"; }
catch (any error) { assert findNoCase("Java class not provided", error.message) > 0; }
try { createObject("java", "org.matchbox.fixture.Counter", "not-a-jar"); throw "JAR accepted"; }
catch (any error) { assert findNoCase("not supported", error.message) > 0; }
"#).unwrap();
    success(&cli(dir.path(), &["app.bxs", "--module", "module"]));
    // The provider returned a frozen class value, not a second mutable-global lookup.
    let source = "replaceClass(); assert createObject('java', 'org.matchbox.fixture.Counter').init(8).info().value == 8;";
    fs::write(dir.path().join("app.bxs"), source).unwrap();
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module"]));
    let app = matchbox_vm::bundle::Application::from_executable(&fs::read(dir.path().join("app.bxb")).unwrap()).unwrap();
    // Use a host VM to replace the mapped global without turning dotted assignment into a struct write.
    let mut vm = VM::new();
    vm.register_callback("replaceClass", matchbox_vm::types::NativeCallback { parameters:vec![], callback:std::rc::Rc::new(|vm, _| {
        vm.insert_global("bxModules.module.CounterReference".into(), BxValue::new_null()); Ok(BxValue::new_null())
    }) }).unwrap();
    app.run(&mut vm, &dir.path().join("cache")).unwrap();
}

#[test]
fn native_class_provider_callbacks_and_references_survive_gc_and_owner_cleanup() {
    let compile = |source: &str| {
        let ast = matchbox_compiler::parser::parse(source, None).unwrap();
        matchbox_compiler::compiler::Compiler::new("native-class-provider.bxs").compile(&ast, source).unwrap()
    };
    let mut vm = VM::new();
    unsafe { matchbox_vm::plugins::load(&mut vm, library()) }.unwrap();
    let module = matchbox_vm::bundle::Module {
        parent_module:None,nested_modules:vec![],
        name:"module".into(), path:"/module".into(), dependencies:vec![], instance:"config".into(),
        descriptor:compile(r#"
class Config {
    function configure() { classProviders = [{name:"java:org.matchbox.fixture.NativeCounter", provider:fixtureClass}]; }
    function onUnload() { assert createObject("java", "org.matchbox.fixture.NativeCounter").answer() == 42; }
}
config = new Config();
"#),
        bifs:compile(""), bif_registrations:vec![], classes:vec![], components:vec![], settings:serde_json::json!({}), artifact:None,
    };
    let app = matchbox_vm::bundle::Application { modules:vec![module], chunk:compile(r#"
reference = createObject(className="org.matchbox.fixture.NativeCounter", type="java");
assert reference.answer() == 42;
assert new java:org.matchbox.fixture.NativeCounter(11).info().value == 11;
reference.init(19);
savedInstanceMethod = reference.info;
savedStatic = reference.answer;
savedConstructor = reference.init;
reference = null;
"#) };
    let cache = tempfile::tempdir().unwrap();
    app.run(&mut vm, cache.path()).unwrap();
    // Lookups are removed, but existing references still own their target and library.
    vm.insert_global("fixtureClass".into(), BxValue::new_null());
    vm.insert_global("config".into(), BxValue::new_null());
    for _ in 0..16 { vm.collect_garbage_now(); }
    let reference = vm.get_global("savedStatic").unwrap();
    assert_eq!(vm.call_function_value(reference, vec![], None).unwrap().as_number(), 42.0);
    let instance_method = vm.get_global("savedInstanceMethod").unwrap();
    let info = vm.call_function_value(instance_method, vec![], None).unwrap();
    assert_eq!(vm.struct_get(info.as_gc_id().unwrap(), "value").as_number(), 19.0);
    let constructor = vm.get_global("savedConstructor").unwrap();
    let instance = vm.call_function_value(constructor, vec![BxValue::new_number(17.0)], None).unwrap();
    let info = vm.call_method_value(instance, "info", vec![]).unwrap();
    assert_eq!(vm.struct_get(info.as_gc_id().unwrap(), "value").as_number(), 17.0);
}

#[test]
fn class_provider_registration_unwinds_partial_startup_and_preserves_original_errors() {
    let compile = |source: &str| {
        let ast = matchbox_compiler::parser::parse(source, None).unwrap();
        matchbox_compiler::compiler::Compiler::new("provider-cleanup.bxs").compile(&ast, source).unwrap()
    };
    for phase in ["duplicate", "startup", "application", "unload"] {
        let mut vm = VM::new();
        unsafe { matchbox_vm::plugins::load(&mut vm, library()) }.unwrap();
        let module = |failing: bool| {
            let duplicate = if failing && phase == "duplicate" { ",{name:'java:org.matchbox.fixture.NativeCounter',provider:fixtureClass}" } else { "" };
            let on_load = if failing && phase == "startup" { "throw 'original startup error';" } else { "" };
            let on_unload = if failing { "throw 'secondary unload error';" } else { "" };
            matchbox_vm::bundle::Module {
                parent_module:None,nested_modules:vec![],
                name:"module".into(), path:"/module".into(), dependencies:vec![], instance:"config".into(),
                descriptor:compile(&format!(r#"
class Config {{
    function configure() {{ classProviders = [{{name:'java:org.matchbox.fixture.NativeCounter',provider:fixtureClass}}{duplicate}]; }}
    function onLoad() {{ {on_load} }}
    function onUnload() {{ assert createObject('java', 'org.matchbox.fixture.NativeCounter').answer() == 42; {on_unload} }}
}}
config = new Config();
"#)),
                bifs:compile(""), bif_registrations:vec![], classes:vec![], components:vec![], settings:serde_json::json!({}), artifact:None,
            }
        };
        let cache = tempfile::tempdir().unwrap();
        let script = if phase == "application" { "throw 'original application error';" } else { "" };
        let app = matchbox_vm::bundle::Application { modules:vec![module(true)], chunk:compile(script) };
        let error = app.run(&mut vm, cache.path()).unwrap_err().to_string();
        assert!(error.contains(match phase { "duplicate" => "already registered", "startup" => "original startup error", "application" => "original application error", _ => "secondary unload error" }), "{phase}: {error}");
        // The same name can be registered again even after partial registration or failing unload.
        let app = matchbox_vm::bundle::Application { modules:vec![module(false)], chunk:compile("assert createObject('java','org.matchbox.fixture.NativeCounter').init(9).info().value == 9;") };
        app.run(&mut vm, cache.path()).unwrap();
    }
}

#[test]
fn class_provider_records_and_results_are_validated_at_execution_only() {
    let dir = tempfile::tempdir().unwrap();
    let module = package(dir.path());
    fs::write(dir.path().join("app.bxs"), "createObject('java','org.matchbox.fixture.Invalid');").unwrap();
    for (providers, factory, expected) in [
        ("{}", "return 0;", "classProviders must be an array"),
        ("[0]", "return 0;", "Class provider must be a struct"),
        ("[{name:1,provider:this.factory}]", "return 0;", "name must be a string"),
        ("[{name:'rust:Invalid',provider:this.factory}]", "return 0;", "exact java:"),
        ("[{name:'java:org.matchbox.fixture.Invalid',provider:0}]", "return 0;", "must be callable"),
        ("[{name:'java:org.matchbox.fixture.Invalid',provider:this.factory}]", "return 0;", "did not return a class reference"),
        ("[{name:'java:org.matchbox.fixture.Invalid',provider:this.factory}]", "throw 'original factory error';", "original factory error"),
    ] {
        fs::write(module.join("ModuleConfig.bx"), format!("class {{ function configure() {{ classProviders = {providers}; }} function factory() {{ {factory} }} }}")).unwrap();
        // Packaging must not run either the descriptor or provider, including invalid definitions.
        success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module"]));
        let output = cli(dir.path(), &["app.bxb"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected), "{}", String::from_utf8_lossy(&output.stderr));
    }
    fs::write(module.join("NullFacade.bx"), "class { function init() { } }").unwrap();
    fs::write(module.join("ModuleConfig.bx"), "import bxModules.module.NullFacade; class { function configure() { classProviders = [{name:'java:org.matchbox.fixture.Invalid', provider:this.factory}]; } function factory() { return NullFacade; } }").unwrap();
    fs::write(dir.path().join("app.bxs"), "createObject('java','org.matchbox.fixture.Invalid').init();").unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("init() returned null"));
}

#[test]
fn static_class_methods_share_named_binding_precedence_and_gc_roots() {
    let source = r#"
class StaticProbe {
    function instanceOnly() { return 0; }
    static function answer(numeric left=40, numeric right=2) { return left + right; }
}
staticReference = StaticProbe.answer;
assert StaticProbe.answer(right=3, left=4) == 7;
"#;
    let ast = matchbox_compiler::parser::parse(source, None).unwrap();
    let chunk = matchbox_compiler::compiler::Compiler::new("static-probe.bxs").compile(&ast, source).unwrap();
    let mut vm = VM::new();
    vm.register_callback("genericAnswer", matchbox_vm::types::NativeCallback {
        parameters: vec!["receiver".into()],
        callback: std::rc::Rc::new(|_, _| Ok(BxValue::new_int(-1))),
    }).unwrap();
    vm.register_member_method(vm.get_global("genericAnswer").unwrap(), &matchbox_vm::bundle::BifMember {
        name:"answer".into(), type_name:"any".into(), object_argument:None,
    }).unwrap();
    vm.interpret(chunk).unwrap();
    let class = vm.get_global("StaticProbe").unwrap();
    assert_eq!(vm.call_method_value(class, "answer", vec![]).unwrap().as_number(), 42.0);
    assert_eq!(vm.call_method_value_named(class, "answer", vec![BxValue::new_int(3), BxValue::new_int(4)], vec!["right".into(), "left".into()]).unwrap().as_number(), 7.0);
    assert!(vm.call_method_value(class, "instanceOnly", vec![]).unwrap_err().to_string().contains("requires an instance"));
    vm.insert_global("StaticProbe".into(), BxValue::new_null());
    for _ in 0..16 { vm.collect_garbage_now(); }
    let reference = vm.get_global("staticReference").unwrap();
    assert_eq!(vm.call_function_value(reference, vec![], None).unwrap().as_number(), 42.0);
    // The captured class itself, not only its compiled method, must still exist.
    assert_eq!(vm.type_name_from_value(class).as_deref(), Some("StaticProbe"));
}

#[test]
fn module_classes_exclude_build_outputs_but_keep_nested_source_directories() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir(&module).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    // Non-class scripts are not module classes; strings/comments are not declarations.
    fs::write(module.join("Script.bxs"),
        "// class NotADeclaration {}\nvar text = 'class Fake {}'; var kind = types.Class; var keys = [Class:'kind']; var unfinished = [:").unwrap();
    let mut source = String::new();
    for name in ["build", "dist", "target"] {
        fs::create_dir_all(module.join(name).join("fixtures")).unwrap();
        fs::write(module.join(name).join("fixtures/Incomplete.bx"), "class { function broken( }").unwrap();
        fs::write(module.join(name).join("Incomplete.bxs"), "class { function broken( }").unwrap();
        // These names are valid below a source directory, not globally reserved.
        fs::create_dir_all(module.join("models").join(name)).unwrap();
        fs::write(module.join("models").join(name).join("Kept.bx"),
            "class { function value() { return 42; } }").unwrap();
        source.push_str(&format!("assert new bxModules.module.models.{name}.Kept().value() == 42;\n"));
    }
    source.push_str("println('module source boundaries OK');");
    fs::write(dir.path().join("app.bxs"), source).unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    assert_eq!(output.stdout, b"module source boundaries OK\n");
    fs::write(module.join("Broken.bxs"), "class { function broken( }").unwrap();
    let invalid = cli(dir.path(), &["app.bxs", "--module", "module"]);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("Broken.bxs"));
    fs::remove_file(module.join("Broken.bxs")).unwrap();
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module", "--output", "app.bxb"]));
    success(&cli(dir.path(), &["--target", "native", "app.bxs", "--module", "module", "--output", "application"]));
    fs::remove_dir_all(module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    for output in [cli(dir.path(), &["app.bxb"]), Command::new(dir.path().join("application"))
        .env("MATCHBOX_NATIVE_CACHE", dir.path().join("fresh-cache"))
        .current_dir(dir.path()).output().unwrap()] {
        success(&output);
        assert_eq!(output.stdout, b"module source boundaries OK\n");
    }
}

#[test]
fn module_names_mappings_dependencies_and_disabled_descriptors() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["consumer-dir", "provider-dir", "disabled"] {
        fs::create_dir(dir.path().join(name)).unwrap();
    }
    fs::write(
        dir.path().join("consumer-dir/box.json"),
        r#"{"boxlang":{"moduleName":"consumer"}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("provider-dir/box.json"),
        r#"{"boxlang":{"moduleName":"provider"}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("consumer-dir/ModuleConfig.bx"),
        r#"
class {
    this.mapping = "consumerMapping";
    this.dependencies = ["provider"];
    function configure() { settings = {}; }
    function onLoad() { assert getModuleSettings("provider").ready; println("consumer loaded"); }
    function onUnload() { println("consumer unloaded"); }
}
"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("provider-dir/ModuleConfig.bx"),
        r#"
class {
    function configure() { settings = {ready:true}; }
    function onLoad() { println("provider loaded"); }
    function onUnload() { println("provider unloaded"); }
}
"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("disabled/ModuleConfig.bx"),
        "class { this.enabled = false; throw 'disabled module executed'; }",
    )
    .unwrap();
    fs::create_dir(dir.path().join("consumer-dir/models")).unwrap();
    fs::write(
        dir.path().join("consumer-dir/models/Value.bx"),
        "class { function get() { return 7; } }",
    )
    .unwrap();
    fs::write(dir.path().join("app.bxs"), "import bxModules.consumerMapping.models.Value as Value; instance = new Value(); assert instance.get() == 7;").unwrap();
    let args = [
        "app.bxs",
        "--module",
        "consumer-dir",
        "--module",
        "provider-dir",
        "--module",
        "disabled",
    ];
    let output = cli(dir.path(), &args);
    success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "provider loaded\nconsumer loaded\nconsumer unloaded\nprovider unloaded"
    );
    fs::write(
        dir.path().join("provider-dir/ModuleConfig.bx"),
        "class { this.dependencies = ['consumer']; }",
    )
    .unwrap();
    let output = cli(dir.path(), &args);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Circular module dependency"));
    fs::remove_dir_all(dir.path().join("provider-dir")).unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "consumer-dir"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires missing module provider"));
}

#[test]
fn shared_bif_annotations_bind_members_independently_of_global_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("boxlang_modules/annotations");
    fs::create_dir_all(module.join("bifs")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(
        module.join("bifs/StringDecorate.bx"),
        r#"
@BoxBIF( ["decorate", "decorateAlias"] )
@BoxMember( {string: {name: "decorate", objectArgument: "text"}} )
class {
    function invoke(required string prefix, required string text, string suffix = "!") {
        return prefix & text & suffix;
    }
}
"#,
    )
    .unwrap();
    fs::write(
        module.join("bifs/ArrayLabel.bx"),
        r#"
@BoxBIF( "onlyArrayLabel" )
@BoxMember( "array" )
class { function invoke(required array value) { return "array-" & arrayLen(value); } }
"#,
    )
    .unwrap();
    fs::write(
        module.join("bifs/StructLabel.bx"),
        r#"
class boxBIF="onlyStructLabel" boxMember={struct: {}} {
    function invoke(required struct value) { return "struct-" & structCount(value); }
}
"#,
    )
    .unwrap();
    fs::write(
        module.join("bifs/IntegerLabel.bx"),
        r#"
@BoxBIF("signedLabel", "signedAlias")
@BoxMember("integer")
class { function invoke(required integer value) { return "integer-" & value; } }
"#,
    )
    .unwrap();
    fs::write(dir.path().join("app.bxs"), r#"
function check(required boolean condition) { if (!condition) { throw "annotation assertion failed"; } }
check(stringDecorate("[", "value") == "[value!");
check(decorateAlias(TEXT="value", PREFIX="[", SUFFIX="]") == "[value]");
check("value".decorate("[") == "[value!");
check("value".decorate(SUFFIX="]", PREFIX="[") == "[value]");
check([1,2].label() == "array-2");
check({}.label() == "struct-0");
check(onlyArrayLabel([1]) == "array-1");
check(onlyStructLabel({a:1}) == "struct-1");
check(integerLabel(3) == "integer-3");
check(signedAlias(3) == "integer-3");
check(signedLabel(3) == "integer-3");
check((3).label() == "integer-3");
check("3".label() == "integer-3");
check((42).decorate("[") == "[42!");
decorate = function() { return "replaced"; };
decorateAlias = decorate;
stringDecorate = decorate;
check("value".decorate("[") == "[value!");
check(!isDefined("arrayLabel"));
check(!isDefined("structLabel"));
println("annotations OK");
"#).unwrap();
    let output = cli(dir.path(), &["app.bxs"]);
    success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "annotations OK"
    );
    success(&cli(
        dir.path(),
        &["--build", "app.bxs", "--output", "app.bxb"],
    ));
    fs::remove_dir_all(dir.path().join("boxlang_modules")).unwrap();
    let output = cli(dir.path(), &["app.bxb"]);
    success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "annotations OK"
    );
}

#[test]
fn module_bifs_preserve_arguments_and_execution_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("bifs")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(
        module.join("bifs/Inspect.bx"),
        r#"
@BoxBIF(["inspectCall","inspectAlias"])
@BoxMember({string:{name:"inspectMember",objectArgument:"target"}})
class { function invoke(string prefix="default") { return arguments; } }
"#,
    )
    .unwrap();
    fs::write(
        module.join("bifs/Bare.bx"),
        r#"
@BoxMember({string:{name:"bareArguments"}})
class { function invoke() { return arguments; } }
"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("app.bxs"),
        r#"
a = inspectAlias(prefix="named", extra={kept:42});
assert a.prefix == "named" && a.extra.kept == 42;
assert a.__functionName == "inspectAlias" && !a.__isMemberExecution;
a = inspectCall("first", "second");
assert a.prefix == "first" && a[2] == "second";
a = "receiver".inspectMember(extra=42,target="ignored");
assert a.target == "receiver" && a.prefix == "default" && a.extra == 42;
assert a.__functionName == "inspectMember" && a.__isMemberExecution;
a = "receiver".inspectMember("prefix");
assert a.prefix == "prefix" && a.target == "receiver";
a = "receiver".bareArguments(3,4);
assert a[1] == "receiver" && a[2] == 3 && a[3] == 4;
assert a.__isMemberExecution;
function ordinary(string declared="default") { return arguments; }
a = ordinary(extra=9);
assert a.declared == "default" && a.extra == 9;
a = ordinary("declared", 2, 3);
assert a.declared == "declared" && a[2] == 2 && a[3] == 3;
class Extra { function inspect(string value="default") { return arguments; } }
a = new Extra().inspect(extra=8);
assert a.value == "default" && a.extra == 8;
println("BIF invocation parity OK");
"#,
    )
    .unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "BIF invocation parity OK"
    );
    success(&cli(
        dir.path(),
        &[
            "--build", "app.bxs", "--module", "module", "--output", "app.bxb",
        ],
    ));
    success(&cli(
        dir.path(),
        &[
            "--target",
            "native",
            "app.bxs",
            "--module",
            "module",
            "--output",
            "application",
        ],
    ));
    fs::remove_dir_all(&module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    success(&cli(dir.path(), &["app.bxb"]));
    let output = Command::new(dir.path().join("application"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    success(&output);
}

#[test]
fn interceptor_registration_roots_instances_until_unregister() {
    let mut vm = VM::new();
    let ast = matchbox_compiler::parser::parse(
        r#"
class Listener { function onGcProbe(data) { data.count += 1; } }
listener = new Listener();
"#,
        None,
    )
    .unwrap();
    let chunk = matchbox_compiler::compiler::Compiler::new("interceptor-gc.bxs")
        .compile(&ast, "")
        .unwrap();
    vm.interpret_sync(chunk).unwrap();
    let listener = vm.get_global("listener").unwrap();
    BxVM::append_interception_points(&mut vm, vec!["onGcProbe".into()]).unwrap();
    BxVM::register_interceptor(&mut vm, listener).unwrap();
    vm.insert_global("listener".into(), BxValue::new_null());
    for _ in 0..16 {
        vm.collect_garbage_now();
    }
    let data = BxVM::struct_new(&mut vm);
    BxVM::struct_set(&mut vm, data, "count", BxValue::new_number(0.0));
    BxVM::announce(&mut vm, "onGcProbe", BxValue::new_ptr(data)).unwrap();
    assert_eq!(BxVM::struct_get(&vm, data, "count").as_number(), 1.0);
    BxVM::unregister_interceptor(&mut vm, listener).unwrap();
    for _ in 0..16 {
        vm.collect_garbage_now();
    }
    assert!(vm.heap.get_opt(listener.as_gc_id().unwrap()).is_none());
}

#[test]
fn module_interceptors_receive_shared_data_and_short_circuit() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("bifs")).unwrap();
    fs::write(
        module.join("ModuleConfig.bx"),
        r#"
class {
    function configure() { customInterceptionPoints = ["onModuleProbe"]; settings = {step:2}; }
    function onModuleProbe(required struct data) { data.count += settings.step; }
    function onLoad() {
        assert boxRuntime.getInterceptorService() == interceptorService;
        assert interceptorService.hasInterceptionPoint("ONMODULEPROBE");
        data = {count:0};
        assert boxAnnounce(state="onModuleProbe",data=data);
        assert data.count == 2;
    }
}
"#,
    )
    .unwrap();
    fs::write(
        module.join("bifs/InterceptorBus.bx"),
        "class { function invoke() { return interceptorService; } }",
    )
    .unwrap();
    fs::write(
        dir.path().join("app.bxs"),
        r#"
class Stop { function onModuleProbe(data) { data.count += 10; return true; } }
class Never { function onModuleProbe(data) { throw "chain did not stop"; } }
i = interceptorBus();
stopTarget = new Stop();
neverTarget = new Never();
i.register(stopTarget);
i.register(neverTarget);
data = {count:0};
i.announce("ONMODULEPROBE",data);
assert data.count == 12;
data.count = 0;
assert boxAnnounce("onModuleProbe",data);
assert data.count == 12;
function probeCaller() {
    boxAnnounce("onModuleProbe",{count:0});
    assert getFunctionCalledName() == "probeCaller";
}
probeCaller();
println("interceptors OK");
"#,
    )
    .unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    success(&cli(
        dir.path(),
        &[
            "--build", "app.bxs", "--module", "module", "--output", "app.bxb",
        ],
    ));
    success(&cli(
        dir.path(),
        &[
            "--target",
            "native",
            "app.bxs",
            "--module",
            "module",
            "--output",
            "application",
        ],
    ));
    fs::remove_dir_all(&module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    success(&cli(dir.path(), &["app.bxb"]));
    success(
        &Command::new(dir.path().join("application"))
            .current_dir(dir.path())
            .output()
            .unwrap(),
    );
}

#[test]
fn annotated_interceptors_discover_points_without_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("bifs")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), r#"
class {
    @interceptionPoint(false)
    function onAnnotatedProbe(data) { data.count += 2; }
    function onLoad() {
        assert interceptorService.hasInterceptionPoint("ONANNOTATEDPROBE");
        data = {count:0};
        boxAnnounce("onAnnotatedProbe",data);
        assert data.count == 2;
    }
}
"#).unwrap();
    fs::write(module.join("bifs/InterceptorBus.bx"), "class { function invoke() { return interceptorService; } }").unwrap();
    fs::write(dir.path().join("app.bxs"), r#"
class ParentListener {
    @interceptionPoint
    function onInheritedProbe(data) { data.count += 3; }
}
class Listener extends="ParentListener" {
    @interceptionPoint
    function onAnnotatedProbe(data) { data.count += 5; }
    /** @interceptionPoint */
    function documentationOnly(data) { throw "Documentation registered a point"; }
}
bus = interceptorBus();
target = new Listener();
bus.register(target);
assert !bus.hasInterceptionPoint("documentationOnly");
data = {count:0};
bus.announce("ONANNOTATEDPROBE",data);
assert data.count == 7;
assert !bus.hasInterceptionPoint("onInheritedProbe");
bus.announce("onInheritedProbe",data);
assert data.count == 7;
parentTarget = new ParentListener();
bus.register(parentTarget);
bus.announce("onInheritedProbe",data);
assert data.count == 10;
println("annotated interceptors OK");
"#).unwrap();
    success(&cli(dir.path(), &["app.bxs", "--module", "module"]));
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module", "--output", "app.bxb"]));
    success(&cli(dir.path(), &["--target", "native", "app.bxs", "--module", "module", "--output", "application"]));
    fs::remove_dir_all(module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    success(&cli(dir.path(), &["app.bxb"]));
    success(&Command::new(dir.path().join("application")).current_dir(dir.path()).output().unwrap());
}

#[test]
fn declared_interceptors_are_configured_injected_and_embedded() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("interceptors")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), r#"
class {
    function configure() {
        interceptors = [
            {class:"interceptors.Probe", properties:{step:3}},
            {class:"bxModules.module.interceptors.Probe", name:"second", properties:{step:4}}
        ];
    }
    function onLoad() {
        assert arrayLen(moduleRecord.interceptors) == 2;
        assert moduleRecord.interceptors[1].name == "bxModules.module.interceptors.Probe@module";
        data = {count:0};
        boxAnnounce("onDeclaredProbe",data);
        assert data.count == 9;
        assert moduleRecord.interceptors[1].properties.step == 4;
    }
    function onUnload() {
        data = {count:0};
        boxAnnounce("onDeclaredProbe",data);
        assert data.count == 9;
        println("declared interceptors unloaded");
    }
}
"#).unwrap();
    fs::write(module.join("interceptors/Probe.bx"), r#"
class {
    function init() { variables.constructed = true; return this; }
    function configure() {
        assert variables.constructed;
        assert moduleRecord.name == "module";
        assert boxRuntime.getInterceptorService() == interceptorService;
        assert len(name) > 0;
        properties.step += 1;
    }
    @interceptionPoint
    function onDeclaredProbe(data) {
        assert interceptorService.hasInterceptionPoint("onDeclaredProbe");
        data.count += properties.step;
    }
}
"#).unwrap();
    fs::write(dir.path().join("app.bxs"), "data = {count:0}; boxAnnounce(\"onDeclaredProbe\",data); assert data.count == 9; println(\"declared interceptors OK\");").unwrap();
    success(&cli(dir.path(), &["app.bxs", "--module", "module"]));
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module", "--output", "app.bxb"]));
    success(&cli(dir.path(), &["--target", "native", "app.bxs", "--module", "module", "--output", "application"]));
    fs::remove_dir_all(module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    success(&cli(dir.path(), &["app.bxb"]));
    success(&Command::new(dir.path().join("application")).current_dir(dir.path()).output().unwrap());
}

#[test]
fn interceptor_service_constructs_classes_and_allows_reentrant_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("bifs")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(module.join("bifs/InterceptorBus.bx"), "class { function invoke() { return interceptorService; } }").unwrap();
    fs::write(module.join("Probe.bx"), r#"
class {
    function configure() { assert interceptorService.getInterceptionPoints().len() >= 0; }
    @interceptionPoint
    function outerProbe(data) { interceptorService.announce("innerProbe",data); }
    @interceptionPoint
    function innerProbe(data) { data.count += properties.step; }
}
"#).unwrap();
    fs::write(dir.path().join("app.bxs"), r#"
bus = interceptorBus();
target = bus.newAndRegister("bxModules.module.Probe",{step:3},"dynamic",null);
data = {count:0};
bus.announce("outerProbe",data);
assert data.count == 3;
bus.unregister(target);
boxAnnounce("outerProbe",data);
assert data.count == 3;
println("reentrant interceptor service OK");
"#).unwrap();
    success(&cli(dir.path(), &["app.bxs", "--module", "module"]));
}

#[test]
fn declared_interceptors_unregister_after_partial_startup_and_application_errors() {
    for startup_failure in [false, true] {
        let mut vm = VM::new();
        let compile = |source: &str| {
            let ast = matchbox_compiler::parser::parse(source, None).unwrap();
            matchbox_compiler::compiler::Compiler::new("cleanup.bxs").compile(&ast, source).unwrap()
        };
        vm.interpret(compile("class Probe { @interceptionPoint function cleanupProbe(data) { data.count += 1; } } ")).unwrap();
        vm.insert_global("bxModules.module.Probe".into(), vm.get_global("Probe").unwrap());
        let additional = if startup_failure { ", {class:\"Missing\"}" } else { "" };
        let descriptor = compile(&format!(r#"
class Config {{
    function configure() {{ interceptors = [{{class:"Probe"}}{additional}]; }}
    function onUnload() {{ throw "secondary unload error"; }}
}}
config = new Config();
"#));
        let module = matchbox_vm::bundle::Module {
            parent_module:None,nested_modules:vec![],
            name:"module".into(), path:"/module".into(), dependencies:vec![],
            instance:"config".into(), descriptor, bifs:compile(""), bif_registrations:vec![], classes:vec![], components:vec![],
            settings:serde_json::json!({}), artifact:None,
        };
        let app = matchbox_vm::bundle::Application { chunk:compile("throw \"original application error\";"), modules:vec![module] };
        let cache = tempfile::tempdir().unwrap();
        let error = app.run(&mut vm,cache.path()).unwrap_err().to_string();
        assert!(error.contains(if startup_failure { "Missing" } else { "original application error" }), "{error}");
        let data = vm.struct_new();
        vm.struct_set(data,"count",BxValue::new_number(0.0));
        BxVM::announce(&mut vm,"cleanupProbe",BxValue::new_ptr(data)).unwrap();
        assert_eq!(vm.struct_get(data,"count").as_number(),0.0);
    }
}

#[test]
fn module_components_dispatch_template_aliases_with_live_services() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("components")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class { function configure() { settings = {suffix:\"!\"}; } }").unwrap();
    fs::write(module.join("components/Echo.bx"), r#"
@BoxComponent(["Speak", "Say"])
class {
    function invoke(required struct attributes, required any context, any body, struct executionState) {
        assert moduleRecord.name == "module";
        assert componentService.hasComponent("SPEAK");
        assert boxRuntime.getComponentService() == componentService;
        assert arrayFindNoCase(moduleRecord.components,"Echo") > 0;
        assert findNoCase("Echo",getName()) > 0;
        assert !isNull(body);
        writeOutput(attributes.message & moduleRecord.settings.suffix);
    }
}
"#).unwrap();
    fs::write(dir.path().join("app.bxm"), "<bx:speak message=\"Hello\" /><bx:Say message=\"World\" /><bx:Echo message=\"Again\" />").unwrap();
    let output = cli(dir.path(), &["app.bxm", "--module", "module"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout),"Hello!World!Again!");
    success(&cli(dir.path(), &["--build", "app.bxm", "--module", "module", "--output", "app.bxb"]));
    success(&cli(dir.path(), &["--target", "native", "app.bxm", "--module", "module", "--output", "application"]));
    fs::remove_dir_all(module).unwrap();
    fs::remove_file(dir.path().join("app.bxm")).unwrap();
    let output = cli(dir.path(), &["app.bxb"]); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout),"Hello!World!Again!");
    let output = Command::new(dir.path().join("application")).current_dir(dir.path()).output().unwrap(); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout),"Hello!World!Again!");
}

#[test]
fn module_callbacks_scope_reentrant_interception_and_bodies_without_sources() {
    let dir = tempfile::tempdir().unwrap();
    let module = package(dir.path());
    fs::create_dir_all(module.join("components")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), r#"class {
        function configure() {
            customInterceptionPoints = ["scopeProbe"];
            interceptors = [{class:"Probe"}];
        }
    }"#).unwrap();
    fs::write(module.join("Probe.bx"), r#"class {
        function scopeProbe(data) {
            stamp = data.depth;
            if (stamp == 0) { data.depth = 1; interceptorService.announce("scopeProbe", data); }
            arrayAppend(data.trace, stamp);
            arrayAppend(data.values, data.callback());
        }
    }"#).unwrap();
    fs::write(module.join("bifs/ScopeCounter.bx"), r#"class {
        function invoke(label = "seed") {
            count = 0;
            return function(step = 1) { count += step; return label & ":" & count; };
        }
    }"#).unwrap();
    fs::write(module.join("components/ScopeWrap.bx"), r#"@RequiresBody(true) class {
        function invoke(context, attributes, body) {
            buffer = newBuffer();
            buffer.append("pre:");
            result = processBody(context, body, buffer);
            writeOutput("[" & buffer.toString() & "]");
            return result;
        }
    }"#).unwrap();
    let source = format!(r#"{}
alpha = scopeCounter("alpha"); beta = scopeCounter("beta");
if (alpha() != "alpha:1" || beta() != "beta:1" || alpha() != "alpha:2") {{ throw "Module BIF scopes were shared across calls"; }}
data = {{depth:0, trace:[], values:[], callback:scopeCounter("event")}};
boxAnnounce("scopeProbe", data);
if (data.trace[1] != 1 || data.trace[2] != 0 || data.values[1] != "event:1" || data.values[2] != "event:2") {{ throw "Reentrant interceptor scopes were overwritten"; }}
function render(prefix) {{
    tail = "!";
    bx:scopeWrap {{ writeOutput(prefix); bx:scopeWrap {{ writeOutput("inner"); }} writeOutput(tail); }}
}}
render("outer");
"#, include_str!("scripts/vm_lexical_closures.bxs"));
    fs::write(dir.path().join("app.bxs"), source).unwrap();
    let expected = "lexical closures OK\n[pre:outer[pre:inner]!]";
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module", "--output", "app.bxb"]));
    success(&cli(dir.path(), &["--target", "native", "app.bxs", "--module", "module", "--output", "application"]));
    fs::remove_dir_all(module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    fs::remove_dir_all(dir.path().join("cache")).unwrap();
    let output = cli(dir.path(), &["app.bxb"]); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    let output = Command::new(dir.path().join("application")).current_dir(dir.path())
        .env("MATCHBOX_NATIVE_CACHE", dir.path().join("native-cache")).output().unwrap(); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn component_bodies_capture_nested_output_and_can_be_processed_repeatedly() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("components")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(module.join("components/Wrap.bx"), r#"
@AllowsBody(true)
@RequiresBody(true)
class {
    function invoke(context, attributes, body, executionState) {
        buffer = newBuffer();
        buffer.append("pre:");
        result = processBody(context,body,buffer);
        assert !result.isEarlyExit();
        writeOutput("[" & buffer.toString() & "]");
        return result;
    }
}
"#).unwrap();
    fs::write(module.join("components/Twice.bx"), r#"
@AllowsBody(true)
class {
    function invoke(context,attributes,body,executionState) {
        var buffer = newBuffer();
        processBody(context,body,buffer);
        var result = processBody(context,body,buffer);
        writeOutput(buffer.toString());
        return result;
    }
}
"#).unwrap();
    fs::write(dir.path().join("app.bxm"), "<bx:script>word = \"outer\";</bx:script><bx:wrap><bx:output>#word#</bx:output><bx:Wrap>inner</bx:Wrap>tail</bx:wrap><bx:twice>!</bx:twice>").unwrap();
    let expected = "[pre:outer[pre:inner]tail]!!";
    let output = cli(dir.path(), &["app.bxm", "--module", "module"]); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout),expected);
    success(&cli(dir.path(), &["--build", "app.bxm", "--module", "module", "--output", "app.bxb"]));
    success(&cli(dir.path(), &["--target", "native", "app.bxm", "--module", "module", "--output", "application"]));
    fs::remove_dir_all(module).unwrap();
    fs::remove_file(dir.path().join("app.bxm")).unwrap();
    let output = cli(dir.path(), &["app.bxb"]); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout),expected);
    let output = Command::new(dir.path().join("application")).current_dir(dir.path()).output().unwrap(); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout),expected);
}

#[test]
fn script_components_use_builder_fallback_and_validate_body_requirements() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("components")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(module.join("components/Frame.bx"), r#"
@RequiresBody(true)
class {
    function invoke(context,attributes,body,executionState) {
        var builder = newBuilder();
        builder.append("suffix");
        var result = processBody(context,body,builder);
        writeOutput(builder.toString());
        return result;
    }
}
"#).unwrap();
    fs::write(module.join("components/Leaf.bx"), "@BoxComponent(\"leafAlias\") class { function invoke(attributes) { writeOutput(attributes.value); } }").unwrap();
    fs::write(dir.path().join("app.bxs"), "bx:frame { writeOutput(\"body\"); } bx:leafAlias value=\"!\";").unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]); success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "bodysuffix!");
    fs::write(dir.path().join("missing.bxm"), "<bx:frame />").unwrap();
    let output = cli(dir.path(), &["missing.bxm", "--module", "module"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires a body"));
    fs::write(dir.path().join("unexpected.bxm"), "<bx:leafAlias>not allowed</bx:leafAlias>").unwrap();
    let output = cli(dir.path(), &["unexpected.bxm", "--module", "module"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not allow a body"));
    fs::write(dir.path().join("unknown.bxm"), "<bx:unknown />").unwrap();
    let output = cli(dir.path(), &["unknown.bxm", "--module", "module"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Component not registered"));
}

#[test]
fn component_body_returns_reach_the_declaring_function() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("components")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(module.join("components/FlowPass.bx"), r#"@RequiresBody(true) class {
        function invoke(context, attributes, body) {
            var buffer = newBuffer();
            var result = processBody(context, body, buffer);
            if (!result.isReturn() || result.resultType() != 1 || result.returnValue() != 42) { throw "Invalid body return result"; }
            writeOutput(buffer.toString());
            return result;
        }
    }"#).unwrap();
    fs::write(dir.path().join("app.bxs"), r#"function probe() {
        bx:flowPass { writeOutput("before:"); return 42; writeOutput("unreachable"); }
        throw "Component return did not reach its declaring function";
    }
    if (probe() != 42) { throw "Component body return value was lost"; }
    println("return OK");"#).unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "before:return OK\n");
}

#[test]
fn component_completion_preserves_declaring_context_and_finally_without_sources() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("components")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(module.join("components/FlowPass.bx"), r#"@RequiresBody(true) class {
        function invoke(context, attributes, body) { return processBody(context, body, newBuffer()); }
    }"#).unwrap();
    let source = format!("{}\nif (!result.state.innerFinally || !result.state.outerFinally) {{ throw \"Finally did not unwind\"; }}", include_str!("scripts/module_component_flow.bxs"));
    fs::write(dir.path().join("app.bxs"), source).unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "nested OK\ndeclarations OK\n");
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module", "--output", "app.bxb"]));
    success(&cli(dir.path(), &["--target", "native", "app.bxs", "--module", "module", "--output", "standalone"]));
    fs::remove_dir_all(&module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    for output in [cli(dir.path(), &["app.bxb"]), Command::new(dir.path().join("standalone")).current_dir(dir.path()).output().unwrap()] {
        success(&output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), "nested OK\ndeclarations OK\n");
    }
}

#[test]
fn component_break_continue_results_can_be_inspected_and_forwarded() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("components")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(module.join("components/Control.bx"), r#"@RequiresBody(true) class {
        function invoke(context, attributes, body) {
            var result = processBody(context, body, newBuffer());
            if (attributes.kind == 2 && (!result.isBreak() || !result.isBreak("unused"))) { throw "Break result missing"; }
            if (attributes.kind == 3 && (!result.isContinue() || !result.isContinue("unused"))) { throw "Continue result missing"; }
            if (!result.isEarlyExit() || result.isReturn() || result.resultType() != attributes.kind || !isNull(result.label()) || !isNull(result.returnValue())) { throw "Invalid control result"; }
            if (attributes.consume) { return; }
            return result;
        }
    }"#).unwrap();
    fs::write(dir.path().join("app.bxs"), r#"
function breakProbe() { bx:control kind=2 consume=false { break; } return "unreachable"; }
function continueProbe() { bx:control kind=3 consume=false { continue; } return "unreachable"; }
if (!isNull(breakProbe()) || !isNull(continueProbe())) { throw "Non-local control result did not reach the function boundary"; }
bx:control kind=2 consume=true { break; }
bx:control kind=3 consume=true { continue; }
println("control results OK");
"#).unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "control results OK\n");
}

#[test]
fn component_body_results_trace_return_values_across_collections() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("components")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(module.join("components/FlowPass.bx"), r#"@RequiresBody(true) class {
        function invoke(context, attributes, body) { this.saved = processBody(context, body, newBuffer()); return this.saved; }
    }"#).unwrap();
    fs::write(dir.path().join("app.bxs"), r#"function probe() {
        bx:flowPass { return {text:"retained body result"}; }
    }
    probe();"#).unwrap();
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module"]));
    let app = matchbox_vm::bundle::Application::from_executable(&fs::read(dir.path().join("app.bxb")).unwrap()).unwrap();
    let mut vm = VM::new();
    app.run(&mut vm, &dir.path().join("cache")).unwrap();
    for _ in 0..16 { vm.collect_garbage_now(); }
    let registry = vm.get_global("__mb_components").unwrap().as_gc_id().unwrap();
    let record = vm.struct_get(registry, "flowPass").as_gc_id().unwrap();
    let target = vm.struct_get(record, "target");
    let saved = vm.instance_property(target, "saved").unwrap();
    let payload = vm.call_method_value(saved, "returnValue", vec![]).unwrap();
    assert_eq!(vm.to_string(vm.struct_get(payload.as_gc_id().unwrap(), "text")), "retained body result");
}

#[test]
fn component_and_interceptor_contexts_expose_live_caller_scopes_and_active_states() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("components")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), r#"class {
        function configure() { customInterceptionPoints = ["contextProbe"]; }
        function contextProbe(data, context) {
            if (context.getScopeNearby("arguments").seed != data.seed) { throw "Announce context is not the caller"; }
            context.getScopeNearby("local").seen = true;
        }
    }"#).unwrap();
    fs::write(module.join("components/ContextFrame.bx"), r#"@RequiresBody(true) class {
        function invoke(context, attributes, body, executionState) {
            var states = context.getComponents();
            if (listLen(getBaseTagList()) != attributes.depth) { throw "Component ancestry BIF returned an empty inventory"; }
            if (context.findClosestComponent("ContextFrame") != executionState || (attributes.depth == 2 && context.findClosestComponent("ContextFrame", 1) != states[2])) { throw "Closest component did not find ancestors"; }
            if (arrayLen(states) != attributes.depth || states[1] != executionState || states[1].name != "ContextFrame") { throw "Component states are not live or ordered"; }
            context.writeToBuffer("<");
            var result = processBody(context, body, newBuffer());
            context.writeToBuffer(">");
            return result;
        }
    }"#).unwrap();
    fs::write(dir.path().join("app.bxs"), r#"function probe(seed = 7) {
        var context = getBoxContext();
        var seen = false;
        boxAnnounce("contextProbe", {seed:seed});
        if (!seen) { throw "Caller local was not updated"; }
        bx:contextFrame depth=1 {
            if (getBoxContext() != context) { throw "Body did not retain declaring context"; }
            bx:contextFrame depth=2 { if (getBoxContext() != context) { throw "Nested body context changed"; } }
        }
        if (arrayLen(context.getComponents()) != 0) { throw "Component context leaked"; }
        try { bx:contextFrame depth=1 { throw "body failure"; } } catch(any error) {}
        if (arrayLen(context.getComponents()) != 0) { throw "Component state leaked on failure"; }
    }
    probe();
    println("component contexts OK");"#).unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "<><component contexts OK\n");
}

#[test]
fn retained_execution_contexts_keep_live_scopes_and_callbacks_across_gc() {
    let mut vm = VM::new();
    let source = include_str!("scripts/vm_execution_contexts.bxs");
    let ast = matchbox_compiler::parser::parse(source, None).unwrap();
    let chunk = matchbox_compiler::compiler::Compiler::new("context-gc.bxs").compile(&ast, source).unwrap();
    vm.interpret_sync(chunk).unwrap();
    vm.insert_global("readerReference".into(), BxValue::new_null());
    vm.insert_global("keepContext".into(), BxValue::new_null());
    for _ in 0..16 { vm.collect_garbage_now(); }
    let context = vm.get_global("savedContext").unwrap();
    let local = BxValue::new_ptr(vm.string_new("local".into()));
    let scope = vm.call_method_value(context, "getScopeNearby", vec![local]).unwrap().as_gc_id().unwrap();
    let callback = vm.struct_get(scope, "reader");
    let result = vm.call_method_value(context, "invokeFunction", vec![callback]).unwrap();
    assert_eq!(result.as_number(), 16.0);
    assert_eq!(vm.struct_get(scope, "count").as_number(), 16.0);
    vm.insert_global("savedContext".into(), BxValue::new_null());
    for _ in 0..16 { vm.collect_garbage_now(); }
    assert!(vm.heap.get_opt(context.as_gc_id().unwrap()).is_none());
    assert!(vm.heap.get_opt(scope).is_none());
}

#[test]
fn function_event_listener_errors_unwind_into_callers_without_poisoning_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(&module).unwrap();
    fs::write(module.join("ModuleConfig.bx"), r#"class {
        function preFunctionInvoke(data) { if (data.name == "preBroken") { throw "pre listener failure"; } }
        function postFunctionInvoke(data) { if (data.name == "postBroken") { throw "post listener failure"; } }
        function onFunctionException(data) { if (data.name == "errorBroken") { throw "exception listener failure"; } }
    }"#).unwrap();
    fs::write(dir.path().join("app.bxs"), r#"
        function preBroken() { throw "Unreachable pre body"; }
        function postBroken() { return 1; }
        function errorBroken() { throw "original function failure"; }
        function stillWorks() { return 42; }
        function caller(callable) {
            try { callable(); } catch(any error) { return error.message; }
            return "Not caught";
        }
        for (callable in [preBroken, postBroken, errorBroken]) {
            if (!find("listener failure", caller(callable))) { throw "Function event error bypassed its caller"; }
        }
        if (stillWorks() != 42) { throw "Function event state remained poisoned"; }
        println("event failures OK");"#).unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "event failures OK\n");
}

#[test]
fn component_output_policy_flags_fail_explicitly_without_running_descriptors() {
    for flag in ["ignoreEnableOutputOnly", "autoEvaluateBodyExpressions"] {
        let dir = tempfile::tempdir().unwrap();
        let module = dir.path().join("module");
        fs::create_dir_all(module.join("components")).unwrap();
        fs::write(module.join("ModuleConfig.bx"), "class { println(\"Descriptor must not run\"); }").unwrap();
        fs::write(module.join("components/Policy.bx"), format!("@BoxComponent class {flag}=true {{ function invoke(context, attributes, body, executionState) {{ return processBody(context, body, newBuffer()); }} }}")).unwrap();
        fs::write(dir.path().join("app.bxs"), "bx:policy;").unwrap();
        let output = cli(dir.path(), &["--build", "app.bxs", "--module", "module", "--output", "app.bxb"]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(&format!("Module component {flag} is not yet supported")), "{}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn unsupported_module_reflection_and_hot_reload_fail_instead_of_returning_placeholders() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("app.bxs"), r#"
        for (operation in [() => getFunctionList(), () => getComponentList(), () => boxModuleReload()]) {
            rejected = false;
            try { operation(); } catch(any error) { rejected = find("not supported", error.message) > 0; }
            if (!rejected) { throw "Unsupported module operation returned a placeholder"; }
        }
        println("module boundaries OK");"#).unwrap();
    let output = cli(dir.path(), &["app.bxs"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "module boundaries OK\n");
}

#[test]
fn module_inventory_bifs_expose_loaded_records_and_live_settings() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(&module).unwrap();
    fs::write(module.join("ModuleConfig.bx"), r#"class { function configure() { settings.value = 7; } }"#).unwrap();
    fs::write(dir.path().join("app.bxs"), r#"
        modules = getModuleList();
        record = getModuleInfo(module="MoDuLe");
        if (!structKeyExists(modules, "module") || modules.module.name != "module" || !record.activated || record.settings.value != 7) { throw "Loaded module inventory is missing"; }
        record.settings.value = 42;
        if (getModuleInfo("module").settings.value != 42 || !structIsEmpty(getModuleInfo("absent"))) { throw "Module inventory did not preserve live settings or absent records"; }
        println("module inventory OK");"#).unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "module inventory OK\n");
}

#[test]
fn module_lifecycle_interception_points_emit_live_records_in_dependency_order() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["watcher", "worker"] { fs::create_dir(dir.path().join(name)).unwrap(); }
    fs::write(dir.path().join("watcher/ModuleConfig.bx"), r#"class {
        this.events = [];
        function onLoad() { if (!interceptorService.hasInterceptionPoint("preModuleLoad") || !interceptorService.hasInterceptionPoint("postModuleUnload")) { throw "Built-in lifecycle points missing"; } }
        function preModuleLoad(data) { this.events.append("pre:" & data.moduleName & ":" & data.moduleRecord.isActivated()); }
        function postModuleLoad(data) { this.events.append("post:" & data.moduleName & ":" & data.moduleRecord.isActivated()); }
        function preModuleUnload(data) { this.events.append("unload:" & data.moduleName & ":" & data.moduleRecord.isActivated()); }
        function postModuleUnload(data) { this.events.append("unloaded:" & data.moduleName & ":" & data.moduleRecord.isActivated()); }
        function onUnload() { println(arrayToList(this.events)); }
    }"#).unwrap();
    fs::write(dir.path().join("worker/ModuleConfig.bx"), r#"class { this.dependencies = ["watcher"]; }"#).unwrap();
    fs::write(dir.path().join("app.bxs"), "println(\"application\");").unwrap();
    let output = cli(dir.path(), &["app.bxs", "--module", "worker", "--module", "watcher"]);
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "application\npost:watcher:true,pre:worker:false,post:worker:true,unload:worker:true,unloaded:worker:false,unload:watcher:true\n");
}

#[test]
fn function_interception_mutates_arguments_results_and_observes_errors() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(&module).unwrap();
    fs::write(module.join("ModuleConfig.bx"), r#"class {
        this.events = [];
        function preFunctionInvoke(data) {
            if (data.name == "probe" || data.name == "probeLoop") {
                if (data.context.getScopeNearby("arguments") != data.arguments || !isCustomFunction(data.function)) { throw "Function event context missing"; }
                data.arguments.value += 1;
                this.events.append("pre");
            }
        }
        function postFunctionInvoke(data) {
            if (data.name == "probe" || data.name == "probeLoop") {
                if (data.name == "probeLoop" && data.context.getScopeNearby("local").counter != 41) { throw "JIT lost declaring scope"; }
                this.events.append("post"); data.result += 1;
            }
        }
        function onFunctionException(data) { if (data.name == "probeFailure") { this.events.append("error:" & data.exception.message); } }
        function onUnload() { println(arrayToList(this.events)); }
    }"#).unwrap();
    let source = format!("{}\n{}", r#"function probe(value = 40) { return value; }
        function probeLoop(value = 40) { var counter=0; while (counter < value) { counter=counter+1; } return value; }
        function probeFailure() { throw "observed failure"; }
        if (probe(value=40) != 42 || probe() != 42) { throw "Function interception did not change default/named arguments and results"; }
        for (var call=0; call<200; call++) { if (probe() != 42 || probeLoop() != 42) { throw "Hot function skipped interception"; } }
        try { probeFailure(); } catch(any error) { if (!find("observed failure", error.message)) { throw error; } }
        println("function events OK");"#, include_str!("scripts/vm_execution_contexts.bxs"));
    fs::write(dir.path().join("app.bxs"), source).unwrap();
    let expected = format!("function events OK\ncontext scopes OK\n{}error:observed failure\n", "pre,post,".repeat(402));
    let output = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&output);
    #[cfg(feature = "jit")]
    {
        let trace = String::from_utf8_lossy(&output.stderr);
        assert!(trace.contains("[JIT] compiled hot fn name=probe "), "{trace}");
        assert!(trace.contains("[JIT] compiled hot loop fn name=probeLoop "), "{trace}");
    }
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    success(&cli(dir.path(), &["--build", "app.bxs", "--module", "module", "--output", "app.bxb"]));
    success(&cli(dir.path(), &["--target", "native", "app.bxs", "--module", "module", "--output", "application"]));
    fs::remove_dir_all(&module).unwrap();
    fs::remove_file(dir.path().join("app.bxs")).unwrap();
    for output in [cli(dir.path(), &["app.bxb"]), Command::new(dir.path().join("application")).current_dir(dir.path()).output().unwrap()] {
        success(&output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}

#[test]
fn component_body_errors_restore_output_and_root_return_stops_execution() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("components")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(module.join("components/Capture.bx"), "@RequiresBody(true) class { function invoke(context,attributes,body) { var buffer = newBuffer(); return processBody(context,body,buffer); } }").unwrap();
    fs::write(dir.path().join("app.bxm"), "<bx:capture><bx:script>writeOutput(\"partial\"); throw \"body failure\";</bx:script></bx:capture>").unwrap();
    let output = cli(dir.path(), &["app.bxm", "--module", "module"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("body failure"));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "partial");
    fs::write(dir.path().join("early.bxs"), "bx:capture { return; }").unwrap();
    let output = cli(dir.path(), &["early.bxs", "--module", "module"]);
    success(&output);
    assert!(output.stdout.is_empty());
}

#[test]
fn parameterless_members_match_positional_and_named_jvm_behavior() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module");
    fs::create_dir_all(module.join("bifs")).unwrap();
    fs::write(module.join("ModuleConfig.bx"), "class {}").unwrap();
    fs::write(
        module.join("bifs/NoParameters.bx"),
        r#"
@BoxMember({string:{name:"noParameters"}})
class { function invoke() { return "no parameters"; } }
"#,
    )
    .unwrap();
    fs::write(
        module.join("bifs/DocOnly.bx"),
        r#"
/**
 * @BoxBIF DocAlias
 * @BoxMember string
 */
class { function invoke(required string value) { return "doc:" & value; } }
"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("app.bxs"),
        r#"
assert noParameters() == "no parameters";
assert "value".noParameters() == "no parameters";
assert "value".noParameters(3, 4) == "no parameters";
assert (3).noParameters() == "no parameters";
assert docOnly("value") == "doc:value";
failed = false;
try { docAlias("value"); } catch (any e) { failed = true; }
assert failed;
failed = false;
try { "value".noParameters(extra=3); } catch (any e) {
    assert find("does not accept", e.message) > 0;
    failed = true;
}
assert failed;
noParameters = function() { return "replaced"; };
assert "value".noParameters() == "no parameters";
println("parameterless members OK");
"#,
    )
    .unwrap();
    let direct = cli(dir.path(), &["app.bxs", "--module", "module"]);
    success(&direct);
    assert_eq!(
        String::from_utf8_lossy(&direct.stdout).trim(),
        "parameterless members OK"
    );
    success(&cli(
        dir.path(),
        &[
            "--build", "app.bxs", "--module", "module", "--output", "app.bxb",
        ],
    ));
    success(&cli(
        dir.path(),
        &[
            "--target",
            "native",
            "app.bxs",
            "--module",
            "module",
            "--output",
            "application",
        ],
    ));
    fs::remove_dir_all(module).unwrap();
    success(&cli(dir.path(), &["app.bxb"]));
    let standalone = Command::new(dir.path().join("application"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    success(&standalone);
    assert_eq!(standalone.stdout, direct.stdout);
}

#[test]
fn startup_failures_unwind_partial_modules_and_preserve_the_original_error() {
    for phase in ["configure", "onLoad", "application"] {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a", "b"] {
            let module = dir.path().join("boxlang_modules").join(name);
            fs::create_dir_all(&module).unwrap();
            let dependency = if name == "b" {
                "this.dependencies = ['a'];"
            } else {
                ""
            };
            let fail_configure = if name == "b" && phase == "configure" {
                "throw 'startup failure';"
            } else {
                ""
            };
            let fail_load = if name == "b" && phase == "onLoad" {
                "throw 'startup failure';"
            } else {
                ""
            };
            let fail_unload = if name == "b" {
                "throw 'shutdown failure';"
            } else {
                ""
            };
            fs::write(
                module.join("ModuleConfig.bx"),
                format!(
                    r#"
class {{
    {dependency}
    function configure() {{ settings = {{}}; {fail_configure} }}
    function onLoad() {{ assert !moduleRecord.isActivated(); {fail_load} println("load-{name}"); }}
    function onUnload() {{
        println("unload-{name}-" & moduleRecord.isActivated());
        {fail_unload}
    }}
}}
"#
                ),
            )
            .unwrap();
        }
        fs::write(dir.path().join("app.bxs"), "throw 'application failure';").unwrap();
        let output = cli(dir.path(), &["app.bxs"]);
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(if phase == "application" {
                "application failure"
            } else {
                "startup failure"
            }),
            "{stderr}"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let expected = match phase {
            "configure" => "unload-b-false\nunload-a-false",
            "onLoad" => "load-a\nunload-b-false\nunload-a-true",
            _ => "load-a\nload-b\nunload-b-true\nunload-a-true",
        };
        assert_eq!(stdout.trim(), expected);
    }
}

#[test]
fn runtime_home_modules_are_discovered_and_project_packages_take_precedence() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let global = home.join("modules/location");
    let local = dir.path().join("boxlang_modules/location");
    fs::create_dir_all(&global).unwrap();
    fs::write(
        global.join("ModuleConfig.bx"),
        "class { function configure() { settings = {source:'home'}; } }",
    )
    .unwrap();
    fs::write(
        dir.path().join("app.bxs"),
        "println(getModuleSettings('location').source);",
    )
    .unwrap();
    let execute = || {
        Command::new(env!("CARGO_BIN_EXE_matchbox"))
            .current_dir(dir.path())
            .env("BOXLANG_HOME", &home)
            .args(["app.bxs"])
            .output()
            .unwrap()
    };
    let output = execute();
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "home");
    fs::create_dir_all(&local).unwrap();
    fs::write(
        local.join("ModuleConfig.bx"),
        "class { function configure() { settings = {source:'project'}; } }",
    )
    .unwrap();
    let output = execute();
    success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "project");
}

#[test]
fn source_native_modules_still_build_through_fusion() {
    let dir = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = cli(
        dir.path(),
        &[
            "--target",
            "native",
            root.join("tests/scripts/vm_module_native_fusion.bxs")
                .to_str()
                .unwrap(),
            "--module",
            root.join("tests/modules/native-math").to_str().unwrap(),
            "--output",
            "fusion-app",
        ],
    );
    success(&output);
    let output = Command::new(dir.path().join("fusion-app"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    success(&output);
}
