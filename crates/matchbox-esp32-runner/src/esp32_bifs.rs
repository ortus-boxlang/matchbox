use crate::camera::{
    capture_frame, capture_jpeg, capture_photo_handle, default_xiao_esp32s3_sense_camera,
    free_photo, low_memory_xiao_esp32s3_sense_camera, low_memory_xiao_esp32s3_sense_print_camera,
    photo_info, shutdown_camera, Esp32CameraOptions, Esp32Capture, StoredCaptureMeta,
};
use crate::imaging::{grayscale_to_monochrome_bitmap, jpeg_to_monochrome_bitmap};
use crate::printer::{
    connect_printer, disconnect_handle, ensure_ble_ready, print_bytes, print_bytes_to_address,
    print_hello_boxlang, shutdown_ble, write_connected, PrinterConnectionInfo,
};
use matchbox_vm::types::{BxNativeFunction, BxVM, BxValue};
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

fn print_result_value(vm: &mut dyn BxVM, print: crate::printer::PrintResult, message: &str) -> BxValue {
    let result = struct_value(vm);
    let message = string_value(vm, message);
    let device_name = string_value(vm, print.device_name);
    let device_id = string_value(vm, print.device_id);
    let write_mode = string_value(vm, print.write_mode);
    vm.struct_set(result, "ok", BxValue::new_bool(true));
    vm.struct_set(result, "message", message);
    vm.struct_set(result, "deviceName", device_name);
    vm.struct_set(result, "deviceId", device_id);
    vm.struct_set(result, "writeMode", write_mode);
    vm.struct_set(
        result,
        "payloadBytes",
        BxValue::new_number(print.payload_bytes as f64),
    );
    BxValue::new_ptr(result)
}

fn printer_connection_value(vm: &mut dyn BxVM, connection: PrinterConnectionInfo) -> BxValue {
    let result = struct_value(vm);
    let device_name = string_value(vm, connection.device_name);
    let device_id = string_value(vm, connection.device_id);
    let write_mode = string_value(vm, connection.write_mode);
    let characteristic_uuid = string_value(vm, connection.characteristic_uuid);
    vm.struct_set(result, "handleId", BxValue::new_number(connection.handle_id as f64));
    vm.struct_set(result, "deviceName", device_name);
    vm.struct_set(result, "deviceId", device_id);
    vm.struct_set(result, "writeMode", write_mode);
    vm.struct_set(result, "characteristicUuid", characteristic_uuid);
    BxValue::new_ptr(result)
}

fn capture_result_value(vm: &mut dyn BxVM, capture: &StoredCaptureMeta) -> BxValue {
    let id = struct_value(vm);
    let format = string_value(vm, capture.format.to_string());
    let image_url = string_value(
        vm,
        format!("/__matchbox/photo/{}?v={}", capture.id, capture.version),
    );
    vm.struct_set(id, "id", BxValue::new_number(capture.id as f64));
    vm.struct_set(id, "format", format);
    vm.struct_set(id, "imageUrl", image_url);
    vm.struct_set(id, "bytes", BxValue::new_number(capture.bytes_len as f64));
    vm.struct_set(id, "width", BxValue::new_number(capture.width as f64));
    vm.struct_set(id, "height", BxValue::new_number(capture.height as f64));
    vm.struct_set(id, "version", BxValue::new_number(capture.version as f64));
    BxValue::new_ptr(id)
}

fn parse_photo_handle_id(vm: &mut dyn BxVM, value: BxValue) -> Result<Option<u64>, String> {
    if let Some(id) = value.as_gc_id() {
        if vm.struct_key_exists(id, "id") {
            return Ok(Some(vm.struct_get(id, "id").as_number() as u64));
        }
    }

    if value.is_null() {
        return Ok(None);
    }

    let as_number = value.as_number();
    if as_number <= 0.0 {
        Ok(None)
    } else {
        Ok(Some(as_number as u64))
    }
}

fn bitmap_result_value(vm: &mut dyn BxVM, width: usize, height: usize, bytes_per_row: usize, bytes: Vec<u8>) -> BxValue {
    let id = struct_value(vm);
    let bytes = bytes_value(vm, bytes);
    vm.struct_set(id, "width", BxValue::new_number(width as f64));
    vm.struct_set(id, "height", BxValue::new_number(height as f64));
    vm.struct_set(id, "bytesPerRow", BxValue::new_number(bytes_per_row as f64));
    vm.struct_set(id, "bytes", bytes);
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
    let format = string_value(vm, format.to_string());
    vm.struct_set(id, "width", BxValue::new_number(width as f64));
    vm.struct_set(id, "height", BxValue::new_number(height as f64));
    vm.struct_set(id, "format", format);
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

fn frame_to_monochrome_bitmap(capture: &Esp32Capture) -> Result<crate::imaging::MonochromeBitmap, String> {
    if capture.format.eq_ignore_ascii_case("grayscale") {
        grayscale_to_monochrome_bitmap(
            capture.width as usize,
            capture.height as usize,
            &capture.bytes,
        )
    } else {
        jpeg_to_monochrome_bitmap(&capture.bytes)
    }
}

fn build_default_camera_options() -> Esp32CameraOptions {
    default_xiao_esp32s3_sense_camera()
}

fn esp32_camera_capture(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    println!("[esp32-bif] camera capture start");
    let stored = capture_photo_handle(&build_default_camera_options())?;
    println!(
        "[esp32-bif] camera capture done width={} height={} bytes={}",
        stored.width,
        stored.height,
        stored.bytes_len
    );
    Ok(capture_result_value(vm, &stored))
}

fn esp32_camera_capture_bitmap(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    println!("[esp32-bif] camera capture-bitmap start");
    let capture = capture_frame(&low_memory_xiao_esp32s3_sense_print_camera())?;
    let bitmap = frame_to_monochrome_bitmap(&capture)?;
    Ok(bitmap_result_value(
        vm,
        bitmap.width,
        bitmap.height,
        bitmap.bytes_per_row,
        bitmap.bytes,
    ))
}

fn esp32_photo_info(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("esp32PhotoInfo requires a photo handle".to_string());
    }
    let Some(handle_id) = parse_photo_handle_id(vm, args[0])? else {
        return Err("esp32PhotoInfo requires a valid photo handle".to_string());
    };
    match photo_info(handle_id)? {
        Some(info) => Ok(capture_result_value(vm, &info)),
        None => Err(format!("photo handle {} was not found", handle_id)),
    }
}

fn esp32_photo_url(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("esp32PhotoUrl requires a photo handle".to_string());
    }
    let Some(handle_id) = parse_photo_handle_id(vm, args[0])? else {
        return Err("esp32PhotoUrl requires a valid photo handle".to_string());
    };
    match photo_info(handle_id)? {
        Some(info) => Ok(string_value(
            vm,
            format!("/__matchbox/photo/{}?v={}", info.id, info.version),
        )),
        None => Err(format!("photo handle {} was not found", handle_id)),
    }
}

fn esp32_photo_free(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let id = if args.is_empty() {
        None
    } else {
        parse_photo_handle_id(vm, args[0])?
    };
    free_photo(id)?;
    Ok(BxValue::new_null())
}

fn esp32_bitmap_from_jpeg(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("esp32BitmapFromJpeg requires JPEG bytes".to_string());
    }

    println!("[esp32-bif] bitmap conversion start");
    let jpeg_bytes = vm.to_bytes(args[0])?;
    let bitmap = jpeg_to_monochrome_bitmap(&jpeg_bytes)?;
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
    let mut payload = Vec::with_capacity(bitmap_bytes.len() + 256);
    payload.extend_from_slice(format!("SIZE {} dot,auto\r\n", bitmap_width).as_bytes());
    payload.extend_from_slice(b"GAP 0 dot,0 dot\r\n");
    payload.extend_from_slice(b"SPEED 3\r\n");
    payload.extend_from_slice(b"DENSITY 8\r\n");
    payload.extend_from_slice(b"CLS\r\n");
    payload.extend_from_slice(
        format!(
            "BITMAP 0,0,{},{},0,",
            bitmap_width.div_ceil(8),
            bitmap_height
        )
        .as_bytes(),
    );
    payload.extend_from_slice(bitmap_bytes);
    payload.extend_from_slice(b"\r\n");
    payload.extend_from_slice(
        format!("TEXT 16,{},\"2\",0,1,1,\"Roastatron 3K\"\r\n", bitmap_height + 24).as_bytes(),
    );
    payload.extend_from_slice(
        format!("TEXT 16,{},\"1\",0,1,1,\"{}\"\r\n", bitmap_height + 56, timestamp).as_bytes(),
    );
    payload.extend_from_slice(
        format!(
            "TEXT 16,{},\"1\",0,1,1,\"{}x{} JPEG\"\r\n",
            bitmap_height + 80,
            capture_width,
            capture_height
        )
        .as_bytes(),
    );
    payload.extend_from_slice(b"PRINT 1,1\r\n");
    payload
}

fn esp32_capture_and_print(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    println!("[esp32-bif] capture-and-print start");
    let capture = capture_jpeg(&build_default_camera_options())
        .map_err(|error| format!("camera: {}", error))?;
    let bitmap = jpeg_to_monochrome_bitmap(&capture.bytes)
        .map_err(|error| format!("bitmap: {}", error))?;
    let payload = build_tspl_payload(
        bitmap.width,
        bitmap.height,
        &bitmap.bytes,
        capture.width,
        capture.height,
    );
    let print = print_bytes(
        "KM",
        "00002af1-0000-1000-8000-00805f9b34fb",
        5000,
        &payload,
    )
    .map_err(|error| format!("printer: {}", error))?;

    let result = struct_value(vm);
    let capture_value = minimal_capture_value(
        vm,
        capture.width,
        capture.height,
        capture.format,
        capture.bytes.len(),
    );
    let bitmap_value = minimal_bitmap_value(
        vm,
        bitmap.width,
        bitmap.height,
        bitmap.bytes_per_row,
        bitmap.bytes.len(),
    );
    let device_name = string_value(vm, print.device_name);
    let device_id = string_value(vm, print.device_id);
    let write_mode = string_value(vm, print.write_mode);
    vm.struct_set(result, "ok", BxValue::new_bool(true));
    vm.struct_set(result, "capture", capture_value);
    vm.struct_set(result, "bitmap", bitmap_value);
    vm.struct_set(result, "deviceName", device_name);
    vm.struct_set(result, "deviceId", device_id);
    vm.struct_set(result, "writeMode", write_mode);
    vm.struct_set(result, "payloadBytes", BxValue::new_number(print.payload_bytes as f64));
    println!("[esp32-bif] capture-and-print done");
    Ok(BxValue::new_ptr(result))
}

fn esp32_printer_capture_bitmap(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("esp32PrinterCaptureBitmap requires a connection handle".to_string());
    }

    let handle_id = if let Some(id) = args[0].as_gc_id() {
        if vm.struct_key_exists(id, "handleId") {
            vm.struct_get(id, "handleId").as_number() as u32
        } else {
            args[0].as_number() as u32
        }
    } else {
        args[0].as_number() as u32
    };

    println!("[esp32-bif] printer-capture-bitmap start handleId={}", handle_id);
    let capture = capture_frame(&low_memory_xiao_esp32s3_sense_print_camera())
        .map_err(|error| format!("camera: {}", error))?;
    let bitmap = frame_to_monochrome_bitmap(&capture)
        .map_err(|error| format!("bitmap: {}", error))?;
    let capture_width = capture.width;
    let capture_height = capture.height;
    let capture_format = capture.format;
    let capture_bytes_len = capture.bytes.len();
    let bitmap_width = bitmap.width;
    let bitmap_height = bitmap.height;
    let bitmap_bytes_per_row = bitmap.bytes_per_row;
    let bitmap_bytes_len = bitmap.bytes.len();
    let payload = build_tspl_payload(
        bitmap_width,
        bitmap_height,
        &bitmap.bytes,
        capture_width,
        capture_height,
    );
    // Free camera and bitmap buffers before we touch the BLE stack.
    drop(bitmap);
    drop(capture);
    let print = write_connected(handle_id, &payload).map_err(|error| format!("printer: {}", error))?;

    let result = struct_value(vm);
    let capture_value = minimal_capture_value(
        vm,
        capture_width,
        capture_height,
        capture_format,
        capture_bytes_len,
    );
    let bitmap_value = minimal_bitmap_value(
        vm,
        bitmap_width,
        bitmap_height,
        bitmap_bytes_per_row,
        bitmap_bytes_len,
    );
    let device_name = string_value(vm, print.device_name);
    let device_id = string_value(vm, print.device_id);
    let write_mode = string_value(vm, print.write_mode);
    vm.struct_set(result, "ok", BxValue::new_bool(true));
    vm.struct_set(result, "capture", capture_value);
    vm.struct_set(result, "bitmap", bitmap_value);
    vm.struct_set(result, "deviceName", device_name);
    vm.struct_set(result, "deviceId", device_id);
    vm.struct_set(result, "writeMode", write_mode);
    vm.struct_set(result, "payloadBytes", BxValue::new_number(print.payload_bytes as f64));
    println!("[esp32-bif] printer-capture-bitmap done handleId={}", handle_id);
    Ok(BxValue::new_ptr(result))
}

fn esp32_capture_bitmap_and_print_to(
    vm: &mut dyn BxVM,
    args: &[BxValue],
) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("esp32CaptureBitmapAndPrintTo requires an address".to_string());
    }

    let address = vm.to_string(args[0]);
    let address_type = if args.len() > 1 && !args[1].is_null() {
        Some(vm.to_string(args[1]))
    } else {
        None
    };
    let characteristic_uuid = if args.len() > 2 && !args[2].is_null() {
        vm.to_string(args[2])
    } else {
        "00002af1-0000-1000-8000-00805f9b34fb".to_string()
    };

    println!(
        "[esp32-bif] capture-bitmap-and-print-to start address={} addressType={:?}",
        address,
        address_type
    );

    // Capture first so the camera init path gets the cleanest internal-RAM
    // state possible before BLE allocates its own controller/connection data.
    let capture = capture_frame(&low_memory_xiao_esp32s3_sense_print_camera())
        .map_err(|error| format!("camera: {}", error))?;
    let bitmap = frame_to_monochrome_bitmap(&capture)
        .map_err(|error| format!("bitmap: {}", error))?;
    let capture_width = capture.width;
    let capture_height = capture.height;
    let capture_format = capture.format;
    let capture_bytes_len = capture.bytes.len();
    let bitmap_width = bitmap.width;
    let bitmap_height = bitmap.height;
    let bitmap_bytes_per_row = bitmap.bytes_per_row;
    let bitmap_bytes_len = bitmap.bytes.len();
    let payload = build_tspl_payload(
        bitmap_width,
        bitmap_height,
        &bitmap.bytes,
        capture_width,
        capture_height,
    );
    // Free camera and bitmap buffers before BLE controller initialization.
    drop(bitmap);
    drop(capture);
    shutdown_camera().map_err(|error| format!("camera shutdown: {}", error))?;
    ensure_ble_ready().map_err(|error| format!("printer init: {}", error))?;
    let print = print_bytes_to_address(
        &address,
        &characteristic_uuid,
        &payload,
        address_type.as_deref(),
    )
    .map_err(|error| format!("printer: {}", error));
    let ble_shutdown_result =
        shutdown_ble().map_err(|error| format!("printer shutdown: {}", error));
    let print = print?;
    ble_shutdown_result?;

    let result = struct_value(vm);
    let capture_value = minimal_capture_value(
        vm,
        capture_width,
        capture_height,
        capture_format,
        capture_bytes_len,
    );
    let bitmap_value = minimal_bitmap_value(
        vm,
        bitmap_width,
        bitmap_height,
        bitmap_bytes_per_row,
        bitmap_bytes_len,
    );
    let device_name = string_value(vm, print.device_name);
    let device_id = string_value(vm, print.device_id);
    let write_mode = string_value(vm, print.write_mode);
    vm.struct_set(result, "ok", BxValue::new_bool(true));
    vm.struct_set(result, "capture", capture_value);
    vm.struct_set(result, "bitmap", bitmap_value);
    vm.struct_set(result, "deviceName", device_name);
    vm.struct_set(result, "deviceId", device_id);
    vm.struct_set(result, "writeMode", write_mode);
    vm.struct_set(
        result,
        "payloadBytes",
        BxValue::new_number(print.payload_bytes as f64),
    );
    println!("[esp32-bif] capture-bitmap-and-print-to done");
    Ok(BxValue::new_ptr(result))
}

fn esp32_print_hello_boxlang(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    println!("[esp32-bif] print-hello-boxlang start");
    let print = print_hello_boxlang().map_err(|error| format!("printer: {}", error))?;
    let result = print_result_value(vm, print, "Hello, BoxLang sent to printer");
    println!("[esp32-bif] print-hello-boxlang done");
    Ok(result)
}

fn esp32_print_tspl(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("esp32PrintTspl requires a TSPL string or bytes payload".to_string());
    }

    let payload = vm.to_string(args[0]).into_bytes();

    let printer_prefix = if args.len() > 1 && !args[1].is_null() {
        vm.to_string(args[1])
    } else {
        "KM".to_string()
    };

    println!(
        "[esp32-bif] print-tspl start prefix={} payloadBytes={}",
        printer_prefix,
        payload.len()
    );
    let print = print_bytes(
        &printer_prefix,
        "00002af1-0000-1000-8000-00805f9b34fb",
        5000,
        &payload,
    )
    .map_err(|error| format!("printer: {}", error))?;
    let result = print_result_value(vm, print, "TSPL payload sent to printer");
    println!("[esp32-bif] print-tspl done");
    Ok(result)
}

fn esp32_bluetooth_write(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err(
            "esp32BluetoothWrite requires address, characteristic UUID, and payload".to_string(),
        );
    }

    let address = vm.to_string(args[0]);
    let characteristic_uuid = vm.to_string(args[1]);
    let payload = match vm.to_bytes(args[2]) {
        Ok(bytes) => bytes,
        Err(_) => vm.to_string(args[2]).into_bytes(),
    };

    println!(
        "[esp32-bif] bluetooth-write start address={} characteristic={} payloadBytes={}",
        address,
        characteristic_uuid,
        payload.len()
    );
    let print = print_bytes_to_address(&address, &characteristic_uuid, &payload, None)
        .map_err(|error| format!("printer: {}", error))?;
    let result = print_result_value(vm, print, "Bluetooth payload sent");
    println!("[esp32-bif] bluetooth-write done");
    Ok(result)
}

fn esp32_print_tspl_to(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("esp32PrintTsplTo requires address and TSPL payload".to_string());
    }

    let address = vm.to_string(args[0]);
    let payload = vm.to_string(args[1]).into_bytes();
    let characteristic_uuid = if args.len() > 2 && !args[2].is_null() {
        vm.to_string(args[2])
    } else {
        "00002af1-0000-1000-8000-00805f9b34fb".to_string()
    };
    let address_type = if args.len() > 3 && !args[3].is_null() {
        Some(vm.to_string(args[3]))
    } else {
        None
    };

    println!(
        "[esp32-bif] print-tspl-to start address={} payloadBytes={} addressType={:?}",
        address,
        payload.len(),
        address_type
    );
    let print = print_bytes_to_address(
        &address,
        &characteristic_uuid,
        &payload,
        address_type.as_deref(),
    )
        .map_err(|error| format!("printer: {}", error))?;
    let result = print_result_value(vm, print, "TSPL payload sent to printer");
    println!("[esp32-bif] print-tspl-to done");
    Ok(result)
}

fn esp32_bluetooth_connect(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() || args[0].is_null() {
        return Err("esp32BluetoothConnect requires an options struct".to_string());
    }

    let options_id = args[0]
        .as_gc_id()
        .ok_or_else(|| "esp32BluetoothConnect options must be a struct".to_string())?;

    let address = if vm.struct_key_exists(options_id, "address") {
        Some(vm.to_string(vm.struct_get(options_id, "address")))
    } else {
        None
    };
    let address_type = if vm.struct_key_exists(options_id, "addressType") {
        Some(vm.to_string(vm.struct_get(options_id, "addressType")))
    } else {
        None
    };
    let name_prefix = if vm.struct_key_exists(options_id, "namePrefix") {
        Some(vm.to_string(vm.struct_get(options_id, "namePrefix")))
    } else {
        None
    };
    let characteristic_uuid = if vm.struct_key_exists(options_id, "characteristicUuid") {
        vm.to_string(vm.struct_get(options_id, "characteristicUuid"))
    } else {
        "00002af1-0000-1000-8000-00805f9b34fb".to_string()
    };
    let timeout_ms = if vm.struct_key_exists(options_id, "timeoutMs") {
        vm.struct_get(options_id, "timeoutMs").as_number() as u64
    } else {
        5000
    };

    println!(
        "[esp32-bif] bluetooth-connect start address={:?} namePrefix={:?}",
        address, name_prefix
    );
    let connection = connect_printer(
        address.as_deref(),
        address_type.as_deref(),
        name_prefix.as_deref(),
        &characteristic_uuid,
        timeout_ms,
    )
    .map_err(|error| format!("printer: {}", error))?;
    let result = printer_connection_value(vm, connection);
    println!("[esp32-bif] bluetooth-connect done");
    Ok(result)
}

fn esp32_bluetooth_disconnect(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() || args[0].is_null() {
        return Err("esp32BluetoothDisconnect requires a connection handle".to_string());
    }

    let handle_id = if let Some(id) = args[0].as_gc_id() {
        if vm.struct_key_exists(id, "handleId") {
            vm.struct_get(id, "handleId").as_number() as u32
        } else {
            args[0].as_number() as u32
        }
    } else {
        args[0].as_number() as u32
    };

    println!("[esp32-bif] bluetooth-disconnect start handleId={}", handle_id);
    disconnect_handle(handle_id).map_err(|error| format!("printer: {}", error))?;
    println!("[esp32-bif] bluetooth-disconnect done");
    Ok(BxValue::new_bool(true))
}

fn esp32_printer_print_tspl(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("esp32PrinterPrintTspl requires a connection handle and TSPL payload".to_string());
    }

    let handle_id = if let Some(id) = args[0].as_gc_id() {
        if vm.struct_key_exists(id, "handleId") {
            vm.struct_get(id, "handleId").as_number() as u32
        } else {
            args[0].as_number() as u32
        }
    } else {
        args[0].as_number() as u32
    };
    let payload = vm.to_string(args[1]).into_bytes();

    println!(
        "[esp32-bif] printer-print-tspl start handleId={} payloadBytes={}",
        handle_id,
        payload.len()
    );
    let print = write_connected(handle_id, &payload).map_err(|error| format!("printer: {}", error))?;
    let result = print_result_value(vm, print, "TSPL payload sent to printer");
    println!("[esp32-bif] printer-print-tspl done");
    Ok(result)
}

pub fn register_bifs() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("esp32cameracapture".to_string(), esp32_camera_capture as BxNativeFunction);
    map.insert("esp32CameraCapture".to_string(), esp32_camera_capture as BxNativeFunction);
    map.insert(
        "esp32cameracapturebitmap".to_string(),
        esp32_camera_capture_bitmap as BxNativeFunction,
    );
    map.insert(
        "esp32CameraCaptureBitmap".to_string(),
        esp32_camera_capture_bitmap as BxNativeFunction,
    );
    map.insert("esp32photoinfo".to_string(), esp32_photo_info as BxNativeFunction);
    map.insert("esp32PhotoInfo".to_string(), esp32_photo_info as BxNativeFunction);
    map.insert("esp32photourl".to_string(), esp32_photo_url as BxNativeFunction);
    map.insert("esp32PhotoUrl".to_string(), esp32_photo_url as BxNativeFunction);
    map.insert("esp32photofree".to_string(), esp32_photo_free as BxNativeFunction);
    map.insert("esp32PhotoFree".to_string(), esp32_photo_free as BxNativeFunction);
    map.insert("esp32bitmapfromjpeg".to_string(), esp32_bitmap_from_jpeg as BxNativeFunction);
    map.insert("esp32BitmapFromJpeg".to_string(), esp32_bitmap_from_jpeg as BxNativeFunction);
    map.insert("esp32captureandprint".to_string(), esp32_capture_and_print as BxNativeFunction);
    map.insert("esp32CaptureAndPrint".to_string(), esp32_capture_and_print as BxNativeFunction);
    map.insert(
        "esp32printercapturebitmap".to_string(),
        esp32_printer_capture_bitmap as BxNativeFunction,
    );
    map.insert(
        "esp32PrinterCaptureBitmap".to_string(),
        esp32_printer_capture_bitmap as BxNativeFunction,
    );
    map.insert(
        "esp32capturebitmapandprintto".to_string(),
        esp32_capture_bitmap_and_print_to as BxNativeFunction,
    );
    map.insert(
        "esp32CaptureBitmapAndPrintTo".to_string(),
        esp32_capture_bitmap_and_print_to as BxNativeFunction,
    );
    map.insert(
        "esp32printhelloboxlang".to_string(),
        esp32_print_hello_boxlang as BxNativeFunction,
    );
    map.insert(
        "esp32PrintHelloBoxLang".to_string(),
        esp32_print_hello_boxlang as BxNativeFunction,
    );
    map.insert("esp32printtspl".to_string(), esp32_print_tspl as BxNativeFunction);
    map.insert("esp32PrintTspl".to_string(), esp32_print_tspl as BxNativeFunction);
    map.insert(
        "esp32bluetoothwrite".to_string(),
        esp32_bluetooth_write as BxNativeFunction,
    );
    map.insert(
        "esp32BluetoothWrite".to_string(),
        esp32_bluetooth_write as BxNativeFunction,
    );
    map.insert(
        "esp32bluetoothconnect".to_string(),
        esp32_bluetooth_connect as BxNativeFunction,
    );
    map.insert(
        "esp32BluetoothConnect".to_string(),
        esp32_bluetooth_connect as BxNativeFunction,
    );
    map.insert(
        "esp32bluetoothdisconnect".to_string(),
        esp32_bluetooth_disconnect as BxNativeFunction,
    );
    map.insert(
        "esp32BluetoothDisconnect".to_string(),
        esp32_bluetooth_disconnect as BxNativeFunction,
    );
    map.insert(
        "esp32printtsplto".to_string(),
        esp32_print_tspl_to as BxNativeFunction,
    );
    map.insert(
        "esp32PrintTsplTo".to_string(),
        esp32_print_tspl_to as BxNativeFunction,
    );
    map.insert(
        "esp32printerprinttspl".to_string(),
        esp32_printer_print_tspl as BxNativeFunction,
    );
    map.insert(
        "esp32PrinterPrintTspl".to_string(),
        esp32_printer_print_tspl as BxNativeFunction,
    );
    map
}
