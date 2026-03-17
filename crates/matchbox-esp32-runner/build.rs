use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // If we're building from the matchbox CLI, it will set BOXLANG_BYTECODE_PATH.
    // Otherwise, we'll use an empty dummy to allow 'cargo build' to work.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dest_path = out_dir.join("bytecode.bxb");

    if let Ok(bytecode_path) = env::var("BOXLANG_BYTECODE_PATH") {
        if !bytecode_path.is_empty() {
            fs::copy(bytecode_path, &dest_path).expect("Failed to copy bytecode");
        } else {
            fs::write(&dest_path, b"").expect("Failed to write dummy bytecode");
        }
    } else {
        // Create an empty chunk as a dummy
        let empty: Vec<u8> = vec![];
        fs::write(&dest_path, empty).expect("Failed to write dummy bytecode");
    }

    // Standard ESP-IDF build script hook
    embuild::espidf::sysenv::output();
}
