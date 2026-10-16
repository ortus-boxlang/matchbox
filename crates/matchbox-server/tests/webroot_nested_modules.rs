#![cfg(not(target_family = "wasm"))]
use matchbox_server::webroot_core::{
    FileSystemAssetStore, WebrootConfig, WebrootEngine, WebrootRequest, WebrootRuntime,
};
use std::{fs, path::Path};
fn write(root: &Path, path: &str, source: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, source).unwrap();
}
#[test]
fn nested_modules_keep_parent_class_mappings_and_public_mapping_snapshots() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write(
        root,
        "boxlang_modules/parent/ModuleConfig.bx",
        "class {this.modules={child:{settings:{value:'parent'}}};}",
    );
    write(
        root,
        "boxlang_modules/parent/models/Base.bx",
        "class {function answer(){return 42;}}",
    );
    write(
        root,
        "boxlang_modules/parent/modules/child/ModuleConfig.bx",
        "class {this.publicMapping={name:'/assets/child',usePrefix:false,path:'web'};function configure(){variables.parentRecord=boxRuntime.getModuleService().getModuleRecord('parent');settings={value:'default'};}function parentLive(){return variables.parentRecord.isActivated();}}",
    );
    write(
        root,
        "boxlang_modules/parent/modules/child/models/Derived.bx",
        "class extends='bxModules.parent.models.Base' {}",
    );
    write(
        root,
        "index.bxs",
        "x=new bxModules.child.models.Derived();writeOutput(x.answer() & '|' & getModuleInfo('child').parentModule & '|' & getModuleSettings('child').value & '|' & getModuleInfo('child').moduleConfig.parentLive());",
    );
    let runtime = WebrootRuntime::from_project(root.into(), Some(root.join("home")), None).unwrap();
    let maps = runtime.module_mappings().unwrap();
    assert!(maps.iter().any(|map| map.name == "/assets/child/"
        && map.external
        && map.path == root.join("boxlang_modules/parent/modules/child/web")));
    let engine = WebrootEngine::with_runtime(
        FileSystemAssetStore::new(root.into()),
        WebrootConfig::default(),
        runtime,
    );
    assert_eq!(
        engine
            .handle(WebrootRequest {
                method: "GET".into(),
                path: "/index.bxs".into(),
                ..Default::default()
            })
            .unwrap()
            .body,
        b"42|parent|parent|true"
    );
}
#[cfg(unix)]
#[test]
fn recursive_physical_module_paths_fail_without_unbounded_discovery() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write(root, "boxlang_modules/parent/ModuleConfig.bx", "class {}");
    fs::create_dir(root.join("boxlang_modules/parent/modules")).unwrap();
    std::os::unix::fs::symlink(
        root.join("boxlang_modules/parent"),
        root.join("boxlang_modules/parent/modules/loop"),
    )
    .unwrap();
    let error = WebrootRuntime::from_project(root.into(), Some(root.join("home")), None)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("Circular nested module path"), "{error}");
}
#[test]
fn nonliteral_parent_overrides_are_rejected_not_silently_evaluated_during_discovery() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write(
        root,
        "boxlang_modules/parent/ModuleConfig.bx",
        "class {this.modules=makeOverrides();}",
    );
    let error = WebrootRuntime::from_project(root.into(), Some(root.join("home")), None)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("must be literals"), "{error}");
}
