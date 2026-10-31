use matchbox_server::webroot_core::{
    EmbeddedAssetStore, FileSystemAssetStore, WebrootConfig, WebrootEngine, WebrootRequest,
};
use std::collections::HashMap;

fn request(n: usize) -> WebrootRequest {
    WebrootRequest {
        method: "GET".into(),
        path: "/index.bxs".into(),
        query: HashMap::from([("n".into(), n.to_string())]),
        ..Default::default()
    }
}

#[test]
fn cached_source_changes_even_when_size_and_mtime_do_not() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("index.bxs");
    std::fs::write(&path, "writeOutput('A' & url.n);").unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let engine = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().into()),
        WebrootConfig::default(),
    );
    for i in 0..5 {
        assert_eq!(
            engine.handle(request(i)).unwrap().body,
            format!("A{i}").as_bytes()
        );
    }
    std::fs::write(&path, "writeOutput('B' & url.n);").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(engine.handle(request(7)).unwrap().body, b"B7");
    std::fs::write(&path, "function (").unwrap();
    assert_eq!(engine.handle(request(1)).unwrap().status, 500);
    std::fs::write(&path, "writeOutput('C' & url.n);").unwrap();
    assert_eq!(engine.handle(request(8)).unwrap().body, b"C8");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(engine.handle(request(1)).unwrap().status, 404);
    std::fs::write(&path, "writeOutput('D' & url.n);").unwrap();
    assert_eq!(engine.handle(request(9)).unwrap().body, b"D9");
}

#[test]
fn nested_cached_includes_execute_fresh_and_invalidate_independently() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("index.bxs"), "include 'part.bxs';").unwrap();
    std::fs::write(root.path().join("part.bxs"), "include 'leaf.bxm';").unwrap();
    std::fs::write(
        root.path().join("leaf.bxm"),
        "<bx:output>A#url.n#</bx:output>",
    )
    .unwrap();
    let engine = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().into()),
        WebrootConfig::default(),
    );
    for i in 0..4 {
        assert_eq!(
            engine.handle(request(i)).unwrap().body,
            format!("A{i}").as_bytes()
        );
    }
    std::fs::write(
        root.path().join("leaf.bxm"),
        "<bx:output>B#url.n#</bx:output>",
    )
    .unwrap();
    assert_eq!(engine.handle(request(5)).unwrap().body, b"B5");
    std::fs::write(root.path().join("index.bxs"), "p='part.bxs'; include p;").unwrap();
    assert_eq!(engine.handle(request(6)).unwrap().body, b"B6");
    std::fs::write(
        root.path().join("leaf.bxm"),
        "<bx:output>C#url.n#</bx:output>",
    )
    .unwrap();
    assert_eq!(engine.handle(request(7)).unwrap().body, b"C7");
}

#[test]
fn cached_templates_do_not_cross_mounts_or_class_contexts() {
    use matchbox_server::webroot_core::WebrootRuntime;
    let runtime = WebrootRuntime::default();
    let first = WebrootEngine::with_runtime(
        EmbeddedAssetStore::new(HashMap::from([(
            "index.bxs".into(),
            b"writeOutput('first-' & url.n);".to_vec(),
        )])),
        WebrootConfig::default(),
        runtime.clone(),
    );
    let second = WebrootEngine::with_runtime(
        EmbeddedAssetStore::new(HashMap::from([(
            "index.bxs".into(),
            b"writeOutput('second-' & url.n);".to_vec(),
        )])),
        WebrootConfig::default(),
        runtime,
    );
    for i in 0..5 {
        assert_eq!(
            first.handle(request(i)).unwrap().body,
            format!("first-{i}").as_bytes()
        );
        assert_eq!(
            second.handle(request(i)).unwrap().body,
            format!("second-{i}").as_bytes()
        );
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("index.bxs"), "writeOutput(marker);").unwrap();
    let engine = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().into()),
        WebrootConfig::default(),
    );
    assert_eq!(engine.handle(request(0)).unwrap().status, 200);
    std::fs::write(
        root.path().join("Application.bx"),
        "class { variables.marker='listener'; }",
    )
    .unwrap();
    for i in 0..4 {
        assert_eq!(engine.handle(request(i)).unwrap().body, b"listener");
    }
}

#[test]
fn literal_dependencies_inside_functions_are_not_stale() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("index.bxs"),
        "function piece(){include 'piece.bxs';} piece();",
    )
    .unwrap();
    std::fs::write(root.path().join("piece.bxs"), "writeOutput('A');").unwrap();
    let engine = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().into()),
        WebrootConfig::default(),
    );
    assert_eq!(engine.handle(request(1)).unwrap().body, b"A");
    std::fs::write(root.path().join("piece.bxs"), "writeOutput('B');").unwrap();
    assert_eq!(engine.handle(request(2)).unwrap().body, b"B");
}

#[test]
fn public_compiler_reports_source_interface_dependencies_for_cache_bypass() {
    use matchbox_compiler::{compiler::Compiler, parser};
    let root = tempfile::tempdir().unwrap();
    let filename = root.path().join("index.bxs").to_string_lossy().into_owned();
    let source = "class Greeter implements=\"Message\" {} g=new Greeter(); writeOutput(g.message());";
    let class = root.path().join("Message.bx");
    let compile = || {
        let ast = parser::parse(source, Some(&filename)).unwrap();
        let mut compiler = Compiler::new(&filename);
        let chunk = compiler.compile(&ast, source).unwrap();
        assert!(compiler.has_source_dependencies());
        serde_json::to_vec(&chunk).unwrap()
    };
    std::fs::write(&class, "interface Message { function message(){return 'A';} }").unwrap();
    let first = compile();
    std::fs::write(&class, "interface Message { function message(){return 'B';} }").unwrap();
    assert_ne!(first, compile());
}

#[test]
fn cached_bytecode_does_not_retain_request_closures() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("index.bxs"),
        "function readN(){return url.n;} f=()=>readN(); writeOutput(f());",
    )
    .unwrap();
    let engine = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().into()),
        WebrootConfig::default(),
    );
    for i in 0..40 {
        assert_eq!(
            engine.handle(request(i)).unwrap().body,
            i.to_string().as_bytes()
        );
    }
}

#[test]
fn a_cache_hit_still_checks_the_asset_store() {
    use matchbox_server::webroot_core::{AssetMetadata, AssetStore};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Gate {
        files: FileSystemAssetStore,
        allowed: Arc<AtomicBool>,
    }
    impl AssetStore for Gate {
        fn metadata(&self, path: &str) -> anyhow::Result<Option<AssetMetadata>> {
            self.files.metadata(path)
        }
        fn source_path(&self, path: &str) -> anyhow::Result<String> {
            self.files.source_path(path)
        }
        fn read(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
            if !self.allowed.load(Ordering::SeqCst) {
                return Ok(None);
            }
            self.files.read(path)
        }
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("index.bxs"), "writeOutput(url.n);").unwrap();
    let allowed = Arc::new(AtomicBool::new(true));
    let engine = WebrootEngine::new(
        Gate {
            files: FileSystemAssetStore::new(root.path().into()),
            allowed: allowed.clone(),
        },
        WebrootConfig::default(),
    );
    assert_eq!(engine.handle(request(1)).unwrap().body, b"1");
    allowed.store(false, Ordering::SeqCst);
    assert_eq!(engine.handle(request(2)).unwrap().status, 404);
    allowed.store(true, Ordering::SeqCst);
    assert_eq!(engine.handle(request(3)).unwrap().body, b"3");
}

#[test]
fn eviction_and_oversized_templates_preserve_execution() {
    let root = tempfile::tempdir().unwrap();
    let engine = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().into()),
        WebrootConfig::default(),
    );
    for i in 0..80 {
        std::fs::write(
            root.path().join(format!("{i}.bxs")),
            format!("writeOutput({i});"),
        )
        .unwrap();
        let mut req = request(i);
        req.path = format!("/{i}.bxs");
        assert_eq!(engine.handle(req).unwrap().body, i.to_string().as_bytes());
    }
    let source = " ".repeat(9 * 1024 * 1024) + "writeOutput(url.n);";
    std::fs::write(root.path().join("index.bxs"), source).unwrap();
    for i in 0..2 {
        assert_eq!(
            engine.handle(request(i)).unwrap().body,
            i.to_string().as_bytes()
        );
    }
    let mut req = request(0);
    req.path = "/0.bxs".into();
    assert_eq!(engine.handle(req).unwrap().body, b"0");
}

#[test]
#[ignore = "manual public-runtime cache performance check; run alone on a quiet host"]
fn warm_filesystem_execution_is_close_to_precompiled_execution() {
    let root = tempfile::tempdir().unwrap();
    let mut source = String::new();
    for i in 0..100 {
        source.push_str(&format!("function fn{i}(x){{return x+{i};}}\n"));
    }
    source.push_str("writeOutput(url.n);");
    std::fs::write(root.path().join("index.bxs"), &source).unwrap();
    let precompiled = WebrootEngine::new(
        EmbeddedAssetStore::from_directory(root.path()).unwrap(),
        WebrootConfig::default(),
    );
    let filesystem = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().to_path_buf()),
        WebrootConfig::default(),
    );
    fn measure<S: matchbox_server::webroot_core::AssetStore + Send + Sync + 'static>(
        engine: &WebrootEngine<S>,
    ) -> std::time::Duration {
        for i in 0..50 {
            let response = engine.handle(request(i)).unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(response.body, i.to_string().as_bytes());
        }
        let start = std::time::Instant::now();
        for i in 0..500 {
            let response = engine.handle(request(i)).unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(response.body, i.to_string().as_bytes());
        }
        start.elapsed()
    }
    let precompiled_time = measure(&precompiled);
    let filesystem_time = measure(&filesystem);
    eprintln!("precompiled={precompiled_time:?}, filesystem={filesystem_time:?}");
    assert!(
        filesystem_time.as_secs_f64() <= precompiled_time.as_secs_f64() * 1.6,
        "warm filesystem still pays compilation cost"
    );
}
