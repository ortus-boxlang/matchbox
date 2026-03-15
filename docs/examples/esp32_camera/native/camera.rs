use matchbox_vm::{types::{BxValue, BxVM, BxNativeFunction}, Chunk};
use std::collections::HashMap;
use esp_idf_sys::{
    self as sys,
    uart_write_bytes,
    uart_port_t_UART_NUM_0,
};

pub fn register_bifs() -> HashMap<String, BxNativeFunction> {
    let mut bifs = HashMap::new();
    bifs.insert("cameraInit".to_string(), camera_init as BxNativeFunction);
    bifs.insert("cameraTake".to_string(), camera_take as BxNativeFunction);
    bifs.insert("serialWrite".to_string(), serial_write as BxNativeFunction);
    bifs
}

// Note: This is a high-level representation for an example.
// Real camera initialization would require setting up the esp32-cam sensor pins.
fn camera_init(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    println!("[native] Camera Initialized (Simulated)");
    Ok(BxValue::new_bool(true))
}

fn camera_take(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    // In a real app, this would call esp_camera_fb_get()
    // We'll simulate a small "image" buffer (10 bytes)
    let simulated_image = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44];
    
    let array_id = vm.array_new();
    for byte in simulated_image {
        vm.array_push(array_id, BxValue::new_number(byte as f64));
    }
    
    println!("[native] Captured simulated image ({} bytes)", 10);
    Ok(BxValue::new_ptr(array_id))
}

fn serial_write(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() { return Err("serialWrite() expects an array of bytes".to_string()); }
    
    if let Some(id) = args[0].as_gc_id() {
        let len = vm.get_len(id);
        let mut buffer = Vec::with_capacity(len);
        
        for i in 0..len {
            let val = vm.array_get(id, i);
            buffer.push(val.as_number() as u8);
        }

        unsafe {
            // Write to UART0 (usually the USB-Serial bridge)
            uart_write_bytes(uart_port_t_UART_NUM_0, buffer.as_ptr() as *const _, buffer.len());
        }
        
        Ok(BxValue::new_int(buffer.len() as i32))
    } else {
        Err("serialWrite() expects an array".to_string())
    }
}
