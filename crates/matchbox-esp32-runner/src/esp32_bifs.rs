use bx_bluetooth_native::esp32_print_bytes;
use bx_camera_native::{esp32_capture_jpeg, Esp32CameraPins, Esp32OpenOptions};
use matchbox_vm::types::{BxNativeFunction, BxVM, BxValue};
use printer_bitmap_native::jpeg_to_monochrome_bitmap_bytes;
use std::collections::HashMap;

fn string_value(vm: &mut dyn BxVM, value: impl Into<String>) -> BxValue {
    BxValue::new_ptr(vm.string_new(value.into()))
}

fn struct_value(vm: &mut dyn BxVM) -> usize {
    vm.struct_new()
}

fn bytes_value(vm: &mut dyn BxVM, bytes: Vec<u8>) -> BxValue {
    BxValue::new_ptr(vm.bytes_new(bytes))
}

fn capture_result_value(vm: &mut dyn BxVM, capture: &bx_camera_native::Esp32CaptureData) -> BxValue {
    let id = struct_value(vm);
    vm.struct_set(id, "bytes", bytes_value(vm, capture.bytes.clone()));
    vm.struct_set(id, "format", string_value(vm, capture.format.clone()));
    vm.struct_set(id, "width", BxValue::new_number(capture.width as f64));
    vm.struct_set(id, "height", BxValue::new_number(capture.height as f64));
    BxValue::new_ptr(id)
}

fn bitmap_result_value(vm: &mut dyn BxVM, width: usize, height: usize, bytes_per_row: usize, bytes: Vec<u8>) -> BxValue {
    let id = struct_value(vm);
    vm.struct_set(id, "width", BxValue::new_number(width as f64));
    vm.struct_set(id, "height", BxValue::new_number(height as f64));
    vm.struct_set(id, "bytesPerRow", BxValue::new_number(bytes_per_row as f64));
    vm.struct_set(id, "bytes", bytes_value(vm, bytes));
    BxValue::new_ptr(id)
}

fn minimal_capture_value(
    vm: &mut dyn BxVM,
    width: u32,
    height: u32,
    format: &str,
    bytes_len: usize,
) -> BxValue {
    let id = struct_value(vm);
    vm.struct_set(id, "width", BxValue::new_number(width as f64));
    vm.struct_set(id, "height", BxValue::new_number(height as f64));
    vm.struct_set(id, "format", string_value(vm, format.to_string()));
    vm.struct_set(id, "bytes", BxValue::new_number(bytes_len as f64));
    BxValue::new_ptr(id)
}

fn minimal_bitmap_value(
    vm: &mut dyn BxVM,
    width: usize,
    height: usize,
    bytes_per_row: usize,
    bytes_len: usize,
) -> BxValue {
    let id = struct_value(vm);
    vm.struct_set(id, "width", BxValue::new_number(width as f64));
    vm.struct_set(id, "height", BxValue::new_number(height as f64));
    vm.struct_set(id, "bytesPerRow", BxValue::new_number(bytes_per_row as f64));
    vm.struct_set(id, "bytes", BxValue::new_number(bytes_len as f64));
    BxValue::new_ptr(id)
}

fn build_default_camera_options() -> Esp32OpenOptions {
    Esp32OpenOptions {
        pins: Some(Esp32CameraPins {
            pin_pwdn: -1,
            pin_reset: -1,
            pin_xclk: 10,
            pin_siod: 40,
            pin_sioc: 39,
            pin_d0: 15,
            pin_d1: 17,
            pin_d2: 18,
            pin_d3: 16,
            pin_d4: 14,
            pin_d5: 12,
            pin_d6: 11,
            pin_d7: 48,
            pin_vsync: 38,
            pin_href: 47,
            pin_pclk: 13,
        }),
        frame_size: "qvga".to_string(),
        pixel_format: "jpeg".to_string(),
        jpeg_quality: 12,
        fb_count: 1,
        xclk_frequency: 20_000_000,
        brightness: 0,
        contrast: 0,
        saturation: 0,
        hmirror: false,
        vflip: false,
        width: None,
        height: None,
    }
}

fn esp32_camera_capture(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    println!("[esp32-bif] camera capture start");
    let capture = esp32_capture_jpeg(&build_default_camera_options())?;
    println!(
        "[esp32-bif] camera capture done width={} height={} bytes={}",
        capture.width,
        capture.height,
        capture.bytes.len()
    );
    Ok(capture_result_value(vm, &capture))
}

fn esp32_bitmap_from_jpeg(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("esp32BitmapFromJpeg requires JPEG bytes".to_string());
    }

    println!("[esp32-bif] bitmap conversion start");
    let jpeg_bytes = vm.to_bytes(args[0])?;
    let bitmap = jpeg_to_monochrome_bitmap_bytes(&jpeg_bytes, false)?;
    println!(
        "[esp32-bif] bitmap conversion done width={} height={} packedBytes={}",
        bitmap.width,
        bitmap.height,
        bitmap.bytes.len()
    );
    Ok(bitmap_result_value(
        vm,
        bitmap.width,
        bitmap.height,
        bitmap.bytes_per_row,
        bitmap.bytes,
    ))
}

fn build_tspl_payload(
    bitmap_width: usize,
    bitmap_height: usize,
    bitmap_bytes: &[u8],
    capture_width: u32,
    capture_height: u32,
) -> Vec<u8> {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut payload = String::new();
    payload.push_str(&format!("SIZE {} dot,auto\r\n", bitmap_width));
    payload.push_str("GAP 0 dot,0 dot\r\n");
    payload.push_str("SPEED 3\r\n");
    payload.push_str("DENSITY 8\r\n");
    payload.push_str("CLS\r\n");
    payload.push_str(&format!(
        "BITMAP 0,0,{},{},0,{}\r\n",
        bitmap_width.div_ceil(8),
        bitmap_height,
        String::from_utf8_lossy(bitmap_bytes)
    ));
    payload.push_str(&format!(
        "TEXT 16,{},\"2\",0,1,1,\"Roastatron 3K\"\r\n",
        bitmap_height + 24
    ));
    payload.push_str(&format!(
        "TEXT 16,{},\"1\",0,1,1,\"{}\"\r\n",
        bitmap_height + 56,
        timestamp
    ));
    payload.push_str(&format!(
        "TEXT 16,{},\"1\",0,1,1,\"{}x{} JPEG\"\r\n",
        bitmap_height + 80,
        capture_width,
        capture_height
    ));
    payload.push_str("PRINT 1,1\r\n");
    payload.into_bytes()
}

fn esp32_capture_and_print(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    println!("[esp32-bif] capture-and-print start");
    let capture = esp32_capture_jpeg(&build_default_camera_options())
        .map_err(|error| format!("camera: {}", error))?;
    let bitmap = jpeg_to_monochrome_bitmap_bytes(&capture.bytes, false)
        .map_err(|error| format!("bitmap: {}", error))?;
    let payload = build_tspl_payload(
        bitmap.width,
        bitmap.height,
        &bitmap.bytes,
        capture.width,
        capture.height,
    );
    let print = esp32_print_bytes(
        "KM",
        "00002af1-0000-1000-8000-00805f9b34fb",
        5000,
        &payload,
    )
    .map_err(|error| format!("printer: {}", error))?;

    let result = struct_value(vm);
    vm.struct_set(result, "ok", BxValue::new_bool(true));
    vm.struct_set(
        result,
        "capture",
        minimal_capture_value(
            vm,
            capture.width,
            capture.height,
            &capture.format,
            capture.bytes.len(),
        ),
    );
    vm.struct_set(
        result,
        "bitmap",
        minimal_bitmap_value(
            vm,
            bitmap.width,
            bitmap.height,
            bitmap.bytes_per_row,
            bitmap.bytes.len(),
        ),
    );
    vm.struct_set(result, "deviceName", string_value(vm, print.device_name));
    vm.struct_set(result, "deviceId", string_value(vm, print.device_id));
    vm.struct_set(result, "writeMode", string_value(vm, print.write_mode));
    vm.struct_set(result, "payloadBytes", BxValue::new_number(print.payload_bytes as f64));
    println!("[esp32-bif] capture-and-print done");
    Ok(BxValue::new_ptr(result))
}

pub fn register_bifs() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("esp32cameracapture".to_string(), esp32_camera_capture as BxNativeFunction);
    map.insert("esp32CameraCapture".to_string(), esp32_camera_capture as BxNativeFunction);
    map.insert("esp32bitmapfromjpeg".to_string(), esp32_bitmap_from_jpeg as BxNativeFunction);
    map.insert("esp32BitmapFromJpeg".to_string(), esp32_bitmap_from_jpeg as BxNativeFunction);
    map.insert("esp32captureandprint".to_string(), esp32_capture_and_print as BxNativeFunction);
    map.insert("esp32CaptureAndPrint".to_string(), esp32_capture_and_print as BxNativeFunction);
    map
}
