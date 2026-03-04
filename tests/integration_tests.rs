use std::path::PathBuf;
use matchbox::process_file;

macro_rules! script_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("tests/scripts");
            path.push($file);
            
            if let Err(e) = process_file(&path, false, None) {
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
