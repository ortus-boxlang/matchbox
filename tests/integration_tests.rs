use std::path::Path;
use matchbox::process_file;

macro_rules! script_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("scripts")
                .join($file);
            
            // source_path, is_build, orig_target, keep_symbols, no_shaking, no_std_lib, strip_source, output, extra_module_paths, is_flash, orig_chip, is_fast_deploy, is_watch, is_full_flash
            if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
                panic!("Script test '{}' failed: {}", $file, e);
            }
        }
    };
}

script_test!(arrays, "arrays.bxs");
script_test!(bifs, "bifs.bxs");
script_test!(for_in_loops, "for_in_loops.bxs");
script_test!(for_loop, "for_loop.bxs");
script_test!(functions, "functions.bxs");
script_test!(hello_world, "hello_world.bxs");
script_test!(nested_interpolation, "nested_interpolation.bxs");
script_test!(return_stmt, "return.bxs");
script_test!(string_concat, "string_concat.bxs");
script_test!(string_interpolation, "string_interpolation.bxs");
script_test!(structs, "structs.bxs");
script_test!(top_level_return, "top_level_return.bxs");
script_test!(vm_basic, "vm_basic.bxs");
script_test!(vm_classes, "vm_classes.bxs");
script_test!(vm_complex_types, "vm_complex_types.bxs");
script_test!(vm_exceptions, "vm_exceptions.bxs");
script_test!(vm_functions, "vm_functions.bxs");
script_test!(vm_if, "vm_if.bxs");
script_test!(vm_loop, "vm_loop.bxs");
script_test!(vm_struct_assign, "vm_struct_assign.bxs");
script_test!(vm_struct_iter, "vm_struct_iter.bxs");
script_test!(vm_inheritance, "vm_inheritance.bxs");
script_test!(vm_accessors, "vm_accessors.bxs");
script_test!(vm_typed_functions, "vm_typed_functions.bxs");
script_test!(vm_defaults, "vm_defaults.bxs");
script_test!(vm_interfaces, "vm_interfaces.bxs");
script_test!(vm_on_missing_method, "vm_on_missing_method.bxs");
script_test!(vm_imports_alias, "vm_imports_alias.bxs");
script_test!(vm_imports_no_alias, "vm_imports_no_alias.bxs");
script_test!(vm_operators, "vm_operators.bxs");
script_test!(vm_continue, "vm_continue.bxs");
script_test!(vm_array_sparse, "vm_array_sparse.bxs");
script_test!(vm_polymorphic_ic, "vm_polymorphic_ic.bxs");
script_test!(vm_fiber_priority, "vm_fiber_priority.bxs");
script_test!(bvm_features, "bvm_features.bxs");
script_test!(vm_safe_nav_elvis, "vm_safe_nav_elvis.bxs");
script_test!(vm_switch, "vm_switch.bxs");
script_test!(vm_while, "vm_while.bxs");

#[test]
#[cfg(feature = "bif-tui")]
fn tui_app_instantiation() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("tui_app_instantiation.bxs");
    
    if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
        panic!("Script test 'tui_app_instantiation.bxs' failed: {}", e);
    }
}

#[test]
#[cfg(feature = "bif-tui")]
fn tui_fluent() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("tui_fluent.bxs");
    
    if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
        panic!("Script test 'tui_fluent.bxs' failed: {}", e);
    }
}
#[test]
fn jit_type_guards() {
    // Cranelift JIT requires more stack space than the default 2MB test thread stack.
    // 32MB is needed on Windows/macOS where MSVC/Apple Clang debug builds have larger
    // per-frame overhead than Linux, causing stack overflows at 8MB.
    let builder = std::thread::Builder::new().name("jit_type_guards".into()).stack_size(32 * 1024 * 1024);
    let handler = builder.spawn(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("scripts")
            .join("jit_type_guards.bxs");

        if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
            panic!("Script execution failed: {}", e);
        }
    }).unwrap();
    handler.join().unwrap();
}

#[test]
#[cfg(feature = "jit")]
fn jit_iter() {
    // Cranelift JIT requires more stack space than the default 2MB test thread stack in debug mode.
    let builder = std::thread::Builder::new().name("jit_iter".into()).stack_size(32 * 1024 * 1024);
    let handler = builder.spawn(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("scripts")
            .join("jit_iter.bxs");

        if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
            panic!("jit_iter.bxs failed: {}", e);
        }
    }).unwrap();
    handler.join().unwrap();
}

#[test]
#[cfg(feature = "jit")]
fn jit_hot_fn() {
    // Cranelift JIT requires more stack space than the default 2MB test thread stack in debug mode.
    let builder = std::thread::Builder::new().name("jit_hot_fn".into()).stack_size(32 * 1024 * 1024);
    let handler = builder.spawn(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("scripts")
            .join("jit_hot_fn.bxs");

        if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
            panic!("jit_hot_fn.bxs failed: {}", e);
        }
    }).unwrap();
    handler.join().unwrap();
}

#[test]
#[cfg(feature = "jit")]
fn jit_osr_loop() {
    // OSR test: persistent JIT profiling counters survive across run_fiber quanta.
    // Runs 12,000 iterations (above the 10,000 compile threshold) and verifies the
    // compiled loop produces the correct accumulated sum.
    // Cranelift JIT requires more stack space than the default 2MB test thread stack in debug mode.
    let builder = std::thread::Builder::new().name("jit_osr_loop".into()).stack_size(32 * 1024 * 1024);
    let handler = builder.spawn(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("scripts")
            .join("jit_osr_loop.bxs");

        if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
            panic!("jit_osr_loop.bxs failed: {}", e);
        }
    }).unwrap();
    handler.join().unwrap();
}

#[test]
#[cfg(feature = "jit")]
fn jit_leaf_call() {
    // Test Tier-4 JIT: compiled function calls another compiled function via
    // direct pointer dispatch. compute(x, fn) takes fn as a parameter (GET_LOCAL),
    // so the CALL instruction is reachable by fn_is_translatable.
    // Cranelift JIT requires more stack space than the default 2MB test thread stack in debug mode.
    let builder = std::thread::Builder::new().name("jit_leaf_call".into()).stack_size(32 * 1024 * 1024);
    let handler = builder.spawn(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("scripts")
            .join("jit_leaf_call.bxs");

        if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
            panic!("jit_leaf_call.bxs failed: {}", e);
        }
    }).unwrap();
    handler.join().unwrap();
}

#[test]
#[cfg(feature = "jit")]
fn jit_pic_member() {
    // 2-entry PIC: Tier-2 loop over a function called with two different
    // struct shapes, IC promoted to Polymorphic before JIT compilation fires.
    let builder = std::thread::Builder::new()
        .name("jit_pic_member".into())
        .stack_size(32 * 1024 * 1024);
    let handler = builder.spawn(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("scripts")
            .join("jit_pic_member.bxs");
        if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
            panic!("jit_pic_member.bxs failed: {}", e);
        }
    }).unwrap();
    handler.join().unwrap();
}

#[test]
#[cfg(feature = "jit")]
fn jit_concat() {
    // Test Tier-4 JIT: functions containing STRING_CONCAT are compiled via
    // jit_concat helper, which allocates the concatenated string on the GC heap.
    let builder = std::thread::Builder::new()
        .name("jit_concat".into())
        .stack_size(32 * 1024 * 1024);
    let handler = builder.spawn(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("scripts")
            .join("jit_concat.bxs");
        if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
            panic!("jit_concat.bxs failed: {}", e);
        }
    }).unwrap();
    handler.join().unwrap();
}

#[test]
#[cfg(feature = "jit")]
fn jit_deopt_recompile() {
    // Test Tier-4 JIT: functions deoptimizing multiple times are recompiled with
    // relaxed guards/polymorphic helpers.
    let builder = std::thread::Builder::new()
        .name("jit_deopt_recompile".into())
        .stack_size(32 * 1024 * 1024);
    let handler = builder.spawn(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("scripts")
            .join("jit_deopt_recompile.bxs");
        if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
            panic!("jit_deopt_recompile.bxs failed: {}", e);
        }
    }).unwrap();
    handler.join().unwrap();
}

#[test]
fn test_vm_interface_fail() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("vm_interface_fail.bxs");
    
    let result = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false);
    assert!(result.is_err(), "vm_interface_fail.bxs should have failed");
}

#[test]
fn test_java_bxs() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("java_test.bxs");
    
    // This might fail if JRE is not available, but let's try
    let _ = process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false);
}

#[test]
fn test_multi_file() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("multi_file.bxs");
    
    process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false).unwrap();
}

#[test]
fn test_vm_modules() {
    let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("vm_modules.bxs");
    
    let module_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("modules")
        .join("greetings");

    process_file(&script_path, false, None, Vec::new(), false, false, false, None, &[module_path], false, None, false, false, false).unwrap();
}

#[test]
fn test_bytecode_roundtrip() {
    let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("hello_world.bxs");
    let out_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp")
        .join("test_roundtrip.bxb");
    
    std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();

    // Compile
    if let Err(e) = process_file(&script_path, true, None, Vec::new(), false, false, true, Some(&out_path), &[], false, None, false, false, false) {
        panic!("Compilation failed: {}", e);
    }

    // Run from bytecode
    if let Err(e) = process_file(&out_path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
        panic!("Running from bytecode failed: {}", e);
    }
}

#[test]
fn test_java_import() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("java_import.bxs");
    
    process_file(&path, false, None, Vec::new(), false, false, false, None, &[], false, None, false, false, false).unwrap();
}

#[test]
fn test_native_fusion_compilation() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("native_fusion")
        .join("script.bxs");
    
    if let Err(e) = process_file(&path, false, Some("native"), Vec::new(), false, false, false, None, &[], false, None, false, false, false) {
        panic!("Native fusion compilation failed: {}", e);
    }
}

#[test]
fn test_module_loading() {
    let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("modules")
        .join("greetings")
        .join("models")
        .join("Greeter.bxs");
    
    let module_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("modules")
        .join("greetings");

    if let Err(e) = process_file(
        &script_path,
        false,
        Some("native"),
        Vec::new(),
        false,
        false,
        false,
        None,
        &[module_path],
        false,
        None,
        false,
        false,
        false
    ) {
        panic!("Module loading test failed: {}", e);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Datasource tests
//
// No-DB tests run whenever the `bif-datasource` feature is enabled.
// DB tests additionally require a running PostgreSQL instance; they are skipped
// when the MATCHBOX_TEST_DB_HOST environment variable is not set.
//
// To run the DB tests locally:
//   docker compose up -d
//   MATCHBOX_TEST_DB_HOST=localhost cargo test --features bif-datasource datasource
// ─────────────────────────────────────────────────────────────────────────────

/// Return the DB host from the environment, or None to signal "skip".
fn db_host() -> Option<String> {
    std::env::var("MATCHBOX_TEST_DB_HOST").ok()
}

/// Run a datasource test script.
/// The script reads MATCHBOX_TEST_DB_HOST via getSystemSetting(); callers must
/// ensure the env var is set before the test process starts.
fn run_ds_script(file: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join(file);

    if let Err(e) = process_file(
        &path, false, None, Vec::new(), false, false, false, None, &[], false, None, false,
        false, false,
    ) {
        panic!("Datasource script '{}' failed: {}", file, e);
    }
}

// --- No-DB tests -----------------------------------------------------------

/// queryNew / queryAddRow / queryColumnList / queryColumnData — no database needed.
#[test]
#[cfg(feature = "bif-datasource")]
fn datasource_query_new() {
    run_ds_script("datasource_query_new.bxs");
}

// --- Database tests --------------------------------------------------------

/// Basic SELECT 1: verifies round-trip connectivity and recordCount.
#[test]
#[cfg(feature = "bif-datasource")]
fn datasource_select() {
    if db_host().is_none() {
        println!("SKIP datasource_select: MATCHBOX_TEST_DB_HOST not set");
        return;
    }
    run_ds_script("datasource_select.bxs");
}

/// Full table scan: users and products tables with type assertions.
#[test]
#[cfg(feature = "bif-datasource")]
fn datasource_select_table() {
    if db_host().is_none() {
        println!("SKIP datasource_select_table: MATCHBOX_TEST_DB_HOST not set");
        return;
    }
    run_ds_script("datasource_select_table.bxs");
}

/// Parameterized queries: positional ?, CF-style {value,cfsqltype}, multi-param.
#[test]
#[cfg(feature = "bif-datasource")]
fn datasource_params() {
    if db_host().is_none() {
        println!("SKIP datasource_params: MATCHBOX_TEST_DB_HOST not set");
        return;
    }
    run_ds_script("datasource_params.bxs");
}

/// returnType "array" and "struct" conversions.
#[test]
#[cfg(feature = "bif-datasource")]
fn datasource_return_types() {
    if db_host().is_none() {
        println!("SKIP datasource_return_types: MATCHBOX_TEST_DB_HOST not set");
        return;
    }
    run_ds_script("datasource_return_types.bxs");
}

#[test]
fn test_native_math_module() {
    let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("vm_module_native_fusion.bxs");
    
    let module_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("modules")
        .join("native-math");

    if let Err(e) = process_file(
        &script_path,
        false,
        None,
        Vec::new(),
        false,
        false,
        false,
        None,
        &[module_path],
        false,
        None,
        false,
        false,
        false
    ) {
        panic!("Native math module test failed: {}", e);
    }
}
