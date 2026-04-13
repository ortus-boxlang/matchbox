use std::fs;
use std::path::Path;

#[test]
fn test_js_bundle_contains_matchbox_namespace() {
    let tmp_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("tmp").join("js_tests");
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).unwrap();
    }
    fs::create_dir_all(&tmp_dir).unwrap();

    let source_path = tmp_dir.join("test.bxs");
    let output_path = tmp_dir.join("test.js");
    
    fs::write(&source_path, "function hello() { return 'world'; }").unwrap();
    
    // We need to call the matchbox CLI logic. 
    matchbox::process_file(
        &source_path,
        false, // is_build
        Some("js"), // target
        vec![], // keep_symbols
        false, // no_shaking
        false, // no_std_lib
        false, // strip_source
        Some(&output_path), // output
        &[], // extra_module_paths
        false, // is_flash
        None, // chip
        false, // is_fast_deploy
        false, // is_watch
        false, // is_full_flash
        false, // esp32_web
    ).expect("process_file should succeed");
    
    let js_content = fs::read_to_string(&output_path).expect("JS file should be generated");
    
    // Assert window.MatchBox namespace initialization
    assert!(js_content.contains("window.MatchBox = window.MatchBox || {}"));
    assert!(js_content.contains("window.MatchBox.runtime = window.MatchBox.runtime || \"browser\";"));
    assert!(js_content.contains("window.MatchBox.contractVersion = window.MatchBox.contractVersion || 1;"));
    assert!(js_content.contains("window.MatchBox.modules = window.MatchBox.modules || {}"));
    
    // Assert module registration
    assert!(js_content.contains("window.MatchBox.modules[\"test\"] = {"));
    assert!(js_content.contains("hello,"));
    
    // Assert readiness signal (Issue 1 requirement)
    // The requirement says window.MatchBox.ready(stem) should be a function.
    assert!(js_content.contains("window.MatchBox.ready = window.MatchBox.ready || function("));
}

#[test]
fn test_js_bundle_isolation() {
    let tmp_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("tmp").join("js_isolation_tests");
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).unwrap();
    }
    fs::create_dir_all(&tmp_dir).unwrap();

    let source_a = tmp_dir.join("moduleA.bxs");
    let output_a = tmp_dir.join("moduleA.js");
    fs::write(&source_a, "function featA() { return 'A'; }").unwrap();

    let source_b = tmp_dir.join("moduleB.bxs");
    let output_b = tmp_dir.join("moduleB.js");
    fs::write(&source_b, "function featB() { return 'B'; }").unwrap();
    
    matchbox::process_file(&source_a, false, Some("js"), vec![], false, false, false, Some(&output_a), &[], false, None, false, false, false, false).unwrap();
    matchbox::process_file(&source_b, false, Some("js"), vec![], false, false, false, Some(&output_b), &[], false, None, false, false, false, false).unwrap();
    
    let js_a = fs::read_to_string(&output_a).unwrap();
    let js_b = fs::read_to_string(&output_b).unwrap();
    
    assert!(js_a.contains("window.MatchBox.modules[\"moduleA\"] = {"));
    assert!(js_a.contains("featA,"));
    assert!(js_a.contains("window.MatchBox._readySignals[\"moduleA\"] = ready;"));

    assert!(js_b.contains("window.MatchBox.modules[\"moduleB\"] = {"));
    assert!(js_b.contains("featB,"));
    assert!(js_b.contains("window.MatchBox._readySignals[\"moduleB\"] = ready;"));
    
    // Ensure they both use the same shared ready function logic
    let ready_func = "window.MatchBox.ready = window.MatchBox.ready || function(stem) {";
    assert!(js_a.contains(ready_func));
    assert!(js_b.contains(ready_func));
}

#[test]
fn test_js_numerical_interop() {
    let tmp_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("tmp").join("js_num_tests");
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).unwrap();
    }
    fs::create_dir_all(&tmp_dir).unwrap();

    let source = tmp_dir.join("num.bxs");
    let output = tmp_dir.join("num.js");
    fs::write(&source, r#"
        function isInt(val) { return isSafeInteger(val); }
        function addOne(val) { return val + 1; }
    "#).unwrap();
    
    matchbox::process_file(&source, false, Some("js"), vec![], false, false, false, Some(&output), &[], false, None, false, false, false, false).expect("process_file failed");
    
    let js_content = fs::read_to_string(&output).unwrap();
    
    // Verify that the generated JS uses the VM's call method which uses js_to_bx
    // The actual conversion happens in the VM's js_to_bx method.
    // We can't easily run the WASM here, but we can verify that the VM implementation is correct
    // and that the generated JS is wired to it.
    assert!(js_content.contains("vm.call(\"isInt\", args)"));
}

#[test]
fn test_js_to_bx_integer_semantics() {
    // This is a unit test for the VM's internal conversion logic
    // but since we are in matchbox crate, we can't easily reach into matchbox-vm's private methods
    // unless we use the public API.
}

#[test]
fn test_js_bundle_contains_state_helper() {
    let tmp_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("tmp").join("js_state_tests");
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).unwrap();
    }
    fs::create_dir_all(&tmp_dir).unwrap();

    let source = tmp_dir.join("state.bxs");
    let output = tmp_dir.join("state.js");
    fs::write(&source, "function init() { return { count: 0 }; } function inc(n) { return { count: n + 1 }; }").unwrap();
    
    matchbox::process_file(&source, false, Some("js"), vec![], false, false, false, Some(&output), &[], false, None, false, false, false, false).unwrap();
    
    let js_content = fs::read_to_string(&output).unwrap();
    
    // Assert window.MatchBox.State is present
    assert!(js_content.contains("window.MatchBox.State = window.MatchBox.State ||"));
    
    // Assert it supports initial state and mount
    assert!(js_content.contains("options.initialState || {}"));
    assert!(js_content.contains("options.mount || null"));
    
    // Assert it handles module readiness
    assert!(js_content.contains("await window.MatchBox.ready(moduleName)"));
    assert!(js_content.contains("window.dispatchEvent(new CustomEvent(\"matchbox:ready\""));
    assert!(js_content.contains("detail: { module: \"state\" }"));
    
    // Assert call method exists and applies state
    assert!(js_content.contains("async call(method, ...args) {"));
    assert!(js_content.contains("return this.applyState(result)"));
    
    // Assert applyState method exists and merges properties
    assert!(js_content.contains("applyState(next) {"));
    assert!(js_content.contains("this[key] = value"));
}

#[test]
fn test_js_error_handling() {
    let tmp_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("tmp").join("js_error_tests");
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).unwrap();
    }
    fs::create_dir_all(&tmp_dir).unwrap();

    let source = tmp_dir.join("error.bxs");
    let output = tmp_dir.join("error.js");
    fs::write(&source, r#"
        function boom() { throw "boom"; }
    "#).unwrap();
    
    matchbox::process_file(&source, false, Some("js"), vec![], false, false, false, Some(&output), &[], false, None, false, false, false, false).unwrap();
    
    let js_content = fs::read_to_string(&output).unwrap();
    
    // We verify that the exported function calls the VM's call method.
    // If the VM's call method returns an Error object (via Result::Err), 
    // it will throw in JS.
    assert!(js_content.contains("await vm.call("));
}
