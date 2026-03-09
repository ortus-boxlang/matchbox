use matchbox_vm::{Chunk, vm::VM};
use anyhow::Result;
use std::env as std_env;
use std::fs;

const MAGIC_FOOTER: &[u8; 8] = b"BOXLANG\x01";

// ---------------------------------------------------------------------------
// WASI embed blob
//
// Layout: [0..8]  = magic sentinel  "BXLG\x00\x00\x00\x01"
//         [8..12] = bytecode length (u32, little-endian), zero until patched
//         [12..]  = bytecode bytes,  zero until patched
//
// `matchbox --target wasi` locates this sentinel in the pre-built stub binary
// and patches the length + bytecode in-place — no cargo invocation needed.
//
// EMBED_CAPACITY is the total size of this region. Data capacity is
// EMBED_CAPACITY - 12 bytes. Scripts that exceed this limit should use
// `--target native` or increase EMBED_CAPACITY and rebuild the stub.
// ---------------------------------------------------------------------------
#[cfg(target_arch = "wasm32")]
const EMBED_CAPACITY: usize = 1024 * 1024; // 1 MB total (≈1 MB - 12 B usable)

#[cfg(target_arch = "wasm32")]
#[used]
#[no_mangle]
static mut BOXLANG_EMBED: [u8; EMBED_CAPACITY] = {
    let mut arr = [0u8; EMBED_CAPACITY];
    // Magic sentinel — must match SENTINEL in produce_wasi_binary
    arr[0] = b'B'; arr[1] = b'X'; arr[2] = b'L'; arr[3] = b'G';
    arr[4] = 0;    arr[5] = 0;    arr[6] = 0;    arr[7] = 1;
    // bytes 8-11: length (u32 LE) — zero → no bytecode embedded
    arr
};

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<()> {
    let bytes = fs::read(std_env::current_exe()?)?;
    let chunk = load_embedded_bytecode(&bytes)?;
    let mut vm = VM::new();
    vm.interpret(chunk)?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() -> Result<()> {
    // Safety: single-threaded WASM, no concurrent access.
    let embed: &[u8] = unsafe {
        std::slice::from_raw_parts((&raw const BOXLANG_EMBED) as *const u8, EMBED_CAPACITY)
    };

    // Verify sentinel
    if embed[0..8] != [b'B', b'X', b'L', b'G', 0, 0, 0, 1] {
        anyhow::bail!("BOXLANG_EMBED sentinel missing — stub may be corrupt");
    }

    let len = u32::from_le_bytes([embed[8], embed[9], embed[10], embed[11]]) as usize;
    if len == 0 {
        // Unpatched stub — nothing to run.
        return Ok(());
    }

    if 12 + len > EMBED_CAPACITY {
        anyhow::bail!("Embedded bytecode length {} exceeds EMBED_CAPACITY", len);
    }

    let bytecode = &embed[12..12 + len];
    let chunk: Chunk = bincode::deserialize(bytecode)?;
    let mut vm = VM::new();
    vm.interpret(chunk)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn load_embedded_bytecode(bytes: &[u8]) -> Result<Chunk> {
    if bytes.len() < 16 { anyhow::bail!("Too small"); }
    let footer_start = bytes.len() - 8;
    if &bytes[footer_start..] != MAGIC_FOOTER { anyhow::bail!("No embedded bytecode found"); }
    let len_start = bytes.len() - 16;
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&bytes[len_start..footer_start]);
    let len = u64::from_le_bytes(len_bytes) as usize;
    let chunk_start = len_start - len;
    let chunk_bytes = &bytes[chunk_start..len_start];
    let chunk: Chunk = bincode::deserialize(chunk_bytes)?;
    Ok(chunk)
}

#[cfg(target_arch = "wasm32")]
mod wasm_interface {
    use super::*;
    use std::cell::RefCell;
    use serde_json::Value as JsonValue;

    thread_local! {
        static VM_INSTANCE: RefCell<VM> = RefCell::new(VM::new());
        static LAST_RESULT: RefCell<String> = RefCell::new(String::new());
    }

    #[no_mangle]
    pub extern "C" fn boxlang_load_bytecode(ptr: *const u8, len: usize) -> i32 {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        let chunk: Chunk = match bincode::deserialize(bytes) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[matchbox] bytecode deserialize error: {}", e);
                LAST_RESULT.with(|lr| *lr.borrow_mut() = format!("{{\"error\":\"Deserialize: {}\"}}", e));
                return -1;
            }
        };
        
        VM_INSTANCE.with(|vm| {
            match vm.borrow_mut().interpret(chunk) {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("[matchbox] interpret error: {}", e);
                    LAST_RESULT.with(|lr| *lr.borrow_mut() = format!("{{\"error\":\"Runtime: {}\"}}", e));
                    -2
                }
            }
        })
    }

    #[no_mangle]
    pub extern "C" fn boxlang_call(name_ptr: *const u8, name_len: usize, args_json_ptr: *const u8, args_json_len: usize) -> *const u8 {
        let name = unsafe {
            let slice = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8_unchecked(slice)
        };
        let args_json = unsafe {
            let slice = std::slice::from_raw_parts(args_json_ptr, args_json_len);
            std::str::from_utf8_unchecked(slice)
        };

        let args_val: JsonValue = serde_json::from_str(args_json).unwrap_or(JsonValue::Array(vec![]));
        
        VM_INSTANCE.with(|vm_ref| {
            let mut vm = vm_ref.borrow_mut();
            let mut bx_args = Vec::new();
            if let JsonValue::Array(arr) = args_val {
                for v in arr {
                    bx_args.push(vm.json_to_bx(v));
                }
            }

            let result = vm.call_function(name, bx_args);
            let json_res = match result {
                Ok(val) => vm.bx_to_json(&val),
                Err(e) => JsonValue::Object(serde_json::Map::from_iter(vec![
                    ("error".to_string(), JsonValue::String(e.to_string()))
                ])),
            };

            let res_str = serde_json::to_string(&json_res).unwrap_or_else(|_| "null".to_string());
            LAST_RESULT.with(|lr| {
                let mut s = lr.borrow_mut();
                *s = res_str;
                s.as_ptr()
            })
        })
    }

    #[no_mangle]
    pub extern "C" fn boxlang_get_last_result_len() -> usize {
        LAST_RESULT.with(|lr| lr.borrow().len())
    }
    
    #[no_mangle]
    pub extern "C" fn boxlang_alloc(len: usize) -> *mut u8 {
        let mut buf = Vec::with_capacity(len);
        let ptr = buf.as_mut_ptr();
        std::mem::forget(buf);
        ptr
    }
}
