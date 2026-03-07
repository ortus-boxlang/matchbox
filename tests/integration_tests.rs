use std::path::PathBuf;
use matchbox::process_file;

macro_rules! script_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("tests/scripts");
            path.push($file);
            
            if let Err(e) = process_file(&path, false, None, Vec::new(), false, false) {
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

#[test]
#[should_panic(expected = "must implement abstract method f")]
fn vm_interface_fail() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/scripts/vm_interface_fail.bxs");
    process_file(&path, false, None, Vec::new(), false, false).unwrap();
}

#[test]
fn test_native_fusion_build() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/native_fusion/script.bxs");
    
    // 1. Build the native binary
    if let Err(e) = process_file(&path, false, Some("native"), Vec::new(), false, false) {
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
    
    // 4. Cleanup
    let _ = std::fs::remove_file(&out_path);
}
