use std::path::PathBuf;
use matchbox::process_file;
use matchbox_vm::Chunk;

macro_rules! script_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("tests/scripts");
            path.push($file);
            
            if let Err(e) = process_file(&path, false, None, Vec::new(), false, false, false, None) {
                panic!("Script {} failed: {}", $file, e);
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
script_test!(java_test, "java_test.bxs");
script_test!(java_import, "java_import.bxs");
script_test!(multi_file, "multi_file.bxs");
script_test!(nested_interpolation, "nested_interpolation.bxs");
script_test!(return_statement, "return.bxs");
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

#[test]
fn test_strip_source() {
    let mut script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    script_path.push("tests/scripts/vm_functions.bxs");

    // Compile to a temp .bxb with --strip-source
    let out_path = std::env::temp_dir().join("matchbox_strip_source_test.bxb");
    if let Err(e) = process_file(&script_path, true, None, Vec::new(), false, false, true, Some(&out_path)) {
        panic!("strip-source compile failed: {}", e);
    }

    // Deserialize and verify source is gone but line info is preserved
    let bytes = std::fs::read(&out_path).expect("Could not read stripped .bxb");
    let chunk: Chunk = bincode::deserialize(&bytes).expect("Could not deserialize .bxb");

    assert!(chunk.source.is_empty(), "chunk.source should be empty after --strip-source, got {} bytes", chunk.source.len());
    assert!(!chunk.lines.is_empty(), "chunk.lines should be preserved after --strip-source");
    assert!(!chunk.filename.is_empty(), "chunk.filename should be preserved after --strip-source");

    // Verify stripped bytecode still executes without error
    if let Err(e) = process_file(&out_path, false, None, Vec::new(), false, false, false, None) {
        panic!("Executing stripped .bxb failed: {}", e);
    }

    let _ = std::fs::remove_file(&out_path);
}

#[test]
#[should_panic(expected = "must implement abstract method f")]
fn vm_interface_fail() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/scripts/vm_interface_fail.bxs");
    process_file(&path, false, None, Vec::new(), false, false, false, None).unwrap();
}

#[test]
fn test_native_fusion_build() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/native_fusion/script.bxs");
    
    // 1. Build the native binary
    if let Err(e) = process_file(&path, false, Some("native"), Vec::new(), false, false, false, None) {
        panic!("Native fusion build failed: {}", e);
    }
    
    // 2. Determine the output binary name
    let out_name = if cfg!(windows) {
        "script.exe"
    } else {
        "script"
    };
    let out_path = path.with_file_name(out_name);
    
    assert!(out_path.exists(), "Native binary was not produced");
    
    // 3. Execute the native binary
    let output = std::process::Command::new(&out_path)
        .output()
        .expect("Failed to execute native binary");
        
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("20"), "Expected output to contain 20, got: {}", stdout);
    assert!(stdout.contains("ENCRYPTED(data)_WITH(my-secret)"), "Expected output to contain ENCRYPTED(data)_WITH(my-secret), got: {}", stdout);
    assert!(stdout.contains("ENCRYPTED(more-data)_WITH(another-key)"), "Expected output to contain ENCRYPTED(more-data)_WITH(another-key), got: {}", stdout);
    assert!(stdout.contains("30"), "Expected output to contain 30, got: {}", stdout);
    assert!(stdout.contains("150"), "Expected output to contain 150, got: {}", stdout);
    assert!(stdout.contains("200"), "Expected output to contain 200, got: {}", stdout);
    
    // 4. Cleanup
    let _ = std::fs::remove_file(&out_path);
}
