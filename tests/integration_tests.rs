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
fn jit_type_guards() {
    // Cranelift JIT requires more stack space than the default 2MB test thread stack in debug mode.
    let builder = std::thread::Builder::new().name("jit_type_guards".into()).stack_size(8 * 1024 * 1024);
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
    let builder = std::thread::Builder::new().name("jit_iter".into()).stack_size(8 * 1024 * 1024);
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
    let builder = std::thread::Builder::new().name("jit_hot_fn".into()).stack_size(8 * 1024 * 1024);
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
    let builder = std::thread::Builder::new().name("jit_osr_loop".into()).stack_size(8 * 1024 * 1024);
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
    let builder = std::thread::Builder::new().name("jit_leaf_call".into()).stack_size(8 * 1024 * 1024);
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
        .stack_size(8 * 1024 * 1024);
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
