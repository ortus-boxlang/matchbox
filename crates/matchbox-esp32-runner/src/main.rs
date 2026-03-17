use matchbox_vm::{vm::VM, Chunk};
use anyhow::{Result, bail};
use esp_idf_sys as _; 
use esp_idf_svc::log::EspLogger;
use postcard;

// Fallback embedded bytecode (compiled at build time)
static EMBEDDED_BYTECODE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bytecode.bxb"));

const MAGIC_FOOTER: &[u8; 8] = b"BOXLANG\x01";

fn load_from_flash() -> Result<Chunk> {
    println!("[matchbox] Attempting to load bytecode from 'storage' partition...");
    
    unsafe {
        let label = std::ffi::CString::new("storage").unwrap();
        let partition = esp_idf_sys::esp_partition_find_first(
            esp_idf_sys::esp_partition_type_t_ESP_PARTITION_TYPE_DATA,
            esp_idf_sys::esp_partition_subtype_t_ESP_PARTITION_SUBTYPE_ANY,
            label.as_ptr(),
        );

        if partition.is_null() {
            bail!("Storage partition not found");
        }

        println!("[matchbox] Found 'storage' partition at offset 0x{:x} (size: {} bytes)", (*partition).address, (*partition).size);

        let mut map_handle: esp_idf_sys::esp_partition_mmap_handle_t = 0;
        let mut map_ptr: *const std::ffi::c_void = std::ptr::null();

        // Map the partition into memory for direct reading
        let err = esp_idf_sys::esp_partition_mmap(
            partition,
            0,
            (*partition).size as usize,
            esp_idf_sys::esp_partition_mmap_memory_t_ESP_PARTITION_MMAP_DATA,
            &mut map_ptr,
            &mut map_handle,
        );

        if err != 0 {
            bail!("Failed to mmap storage partition (error {})", err);
        }

        // The first 4 bytes of our partition will store the length of the bytecode.
        let data_ptr = map_ptr as *const u8;
        let len = u32::from_le_bytes([*data_ptr, *data_ptr.add(1), *data_ptr.add(2), *data_ptr.add(3)]) as usize;

        if len == 0 || len > ((*partition).size as usize - 4) {
             esp_idf_sys::esp_partition_munmap(map_handle);
             bail!("Storage partition is empty or invalid (len: {})", len);
        }

        println!("[matchbox] Found bytecode in flash! ({} bytes)", len);
        let bytecode = std::slice::from_raw_parts(data_ptr.add(4), len);
        let mut chunk: Chunk = postcard::from_bytes(bytecode)?;
        chunk.reconstruct_functions();
        
        esp_idf_sys::esp_partition_munmap(map_handle);
        Ok(chunk)
    }
}

fn load_appended_bytecode() -> Result<Chunk> {
    unsafe {
        let partition = esp_idf_sys::esp_partition_find_first(
            esp_idf_sys::esp_partition_type_t_ESP_PARTITION_TYPE_APP,
            esp_idf_sys::esp_partition_subtype_t_ESP_PARTITION_SUBTYPE_APP_FACTORY,
            std::ptr::null(),
        );
        if partition.is_null() { bail!("Factory app partition not found"); }

        let mut map_handle: esp_idf_sys::esp_partition_mmap_handle_t = 0;
        let mut map_ptr: *const std::ffi::c_void = std::ptr::null();
        let err = esp_idf_sys::esp_partition_mmap(
            partition,
            0,
            (*partition).size as usize,
            esp_idf_sys::esp_partition_mmap_memory_t_ESP_PARTITION_MMAP_DATA,
            &mut map_ptr,
            &mut map_handle,
        );
        if err != 0 { bail!("Failed to mmap app partition"); }

        let data = std::slice::from_raw_parts(map_ptr as *const u8, (*partition).size as usize);
        
        if data.len() < 16 { 
            esp_idf_sys::esp_partition_munmap(map_handle);
            bail!("App partition too small"); 
        }

        let footer_pos = data.windows(8).rposition(|w| w == MAGIC_FOOTER);
        
        if let Some(pos) = footer_pos {
            let len_start = pos - 8;
            let len = u64::from_le_bytes([
                data[len_start], data[len_start+1], data[len_start+2], data[len_start+3],
                data[len_start+4], data[len_start+5], data[len_start+6], data[len_start+7]
            ]) as usize;
            
            let chunk_start = len_start - len;
            println!("[matchbox] Found appended bytecode at 0x{:x} (len: {})", chunk_start, len);
            
            let mut chunk: Chunk = postcard::from_bytes(&data[chunk_start..len_start])?;
            chunk.reconstruct_functions();
            esp_idf_sys::esp_partition_munmap(map_handle);
            Ok(chunk)
        } else {
            esp_idf_sys::esp_partition_munmap(map_handle);
            bail!("No appended bytecode footer found in app partition");
        }
    }
}

fn run_vm() -> Result<()> {
    let chunk = load_from_flash()
        .or_else(|e| {
            println!("[matchbox] Partition load skipped: {}. Trying appended...", e);
            load_appended_bytecode()
        })
        .or_else(|e| {
            println!("[matchbox] Appended load skipped: {}. Falling back to embedded.", e);
            let mut c: Chunk = postcard::from_bytes(EMBEDDED_BYTECODE)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize embedded bytecode: {}", e))?;
            c.reconstruct_functions();
            Result::<Chunk>::Ok(c)
        })?;

    let mut vm = VM::new();
    println!("[matchbox] Executing BoxLang Script...");
    
    match vm.interpret(chunk) {
        Ok(val) => {
            println!("[matchbox] Script finished. Result: {}", val);
        }
        Err(e) => {
            eprintln!("[matchbox] Runtime Error: {}", e);
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    esp_idf_sys::link_patches();
    EspLogger::initialize_default();

    println!("[matchbox] ESP32 MatchBox Runner Starting...");

    // We use a high-priority task with a LARGE stack (48KB) to avoid stack overflow.
    const STACK_SIZE: u32 = 48 * 1024;
    
    extern "C" fn task_wrapper(_: *mut std::ffi::c_void) {
        if let Err(e) = run_vm() {
            eprintln!("[matchbox] VM Task failed: {}", e);
        }
        println!("[matchbox] VM Task finished. Standing by...");
        loop {
            unsafe { esp_idf_sys::vTaskDelay(100); }
        }
    }

    unsafe {
        let name = std::ffi::CString::new("matchbox_vm").unwrap();
        let res = esp_idf_sys::xTaskCreatePinnedToCore(
            Some(task_wrapper),
            name.as_ptr(),
            STACK_SIZE,
            std::ptr::null_mut(),
            5, // Priority
            std::ptr::null_mut(),
            0, // Core
        );

        if res != 1 {
            bail!("Failed to create VM task (error {})", res);
        }
    }

    println!("[matchbox] Main task standing by...");
    loop {
        unsafe { esp_idf_sys::vTaskDelay(1000); }
    }
}
