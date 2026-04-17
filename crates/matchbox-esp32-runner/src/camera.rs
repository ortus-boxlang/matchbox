#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
use esp_idf_sys::{self, camera};
#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
use std::collections::HashMap;
#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
use std::sync::{Mutex as StdMutex, OnceLock};

#[derive(Clone, Debug)]
pub struct Esp32CameraPins {
    pub pin_pwdn: i32,
    pub pin_reset: i32,
    pub pin_xclk: i32,
    pub pin_siod: i32,
    pub pin_sioc: i32,
    pub pin_d0: i32,
    pub pin_d1: i32,
    pub pin_d2: i32,
    pub pin_d3: i32,
    pub pin_d4: i32,
    pub pin_d5: i32,
    pub pin_d6: i32,
    pub pin_d7: i32,
    pub pin_vsync: i32,
    pub pin_href: i32,
    pub pin_pclk: i32,
}

#[derive(Clone, Debug)]
pub struct Esp32CameraOptions {
    pub pins: Esp32CameraPins,
    pub frame_size: &'static str,
    pub pixel_format: &'static str,
    pub jpeg_quality: i32,
    pub fb_count: i32,
    pub xclk_frequency: u32,
}

#[derive(Clone, Debug)]
pub struct Esp32Capture {
    pub width: u32,
    pub height: u32,
    pub format: &'static str,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
struct StoredCapture {
    id: u64,
    version: u64,
    width: u32,
    height: u32,
    format: String,
    frame_ptr: usize,
    bytes_len: usize,
}

#[derive(Clone, Debug)]
pub struct StoredCaptureMeta {
    pub id: u64,
    pub version: u64,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub bytes_len: usize,
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
unsafe fn frame_ptr_as_ref<'a>(ptr: usize) -> &'a camera::camera_fb_t {
    &*(ptr as *const camera::camera_fb_t)
}

pub fn default_xiao_esp32s3_sense_camera() -> Esp32CameraOptions {
    Esp32CameraOptions {
        pins: Esp32CameraPins {
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
        },
        frame_size: "qvga",
        pixel_format: "jpeg",
        jpeg_quality: 12,
        fb_count: 1,
        xclk_frequency: 20_000_000,
    }
}

pub fn low_memory_xiao_esp32s3_sense_camera() -> Esp32CameraOptions {
    Esp32CameraOptions {
        pins: Esp32CameraPins {
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
        },
        frame_size: "qqvga",
        pixel_format: "jpeg",
        jpeg_quality: 16,
        fb_count: 1,
        xclk_frequency: 20_000_000,
    }
}

pub fn low_memory_xiao_esp32s3_sense_print_camera() -> Esp32CameraOptions {
    Esp32CameraOptions {
        pins: Esp32CameraPins {
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
        },
        frame_size: "128x128",
        pixel_format: "grayscale",
        jpeg_quality: 16,
        fb_count: 1,
        xclk_frequency: 10_000_000,
    }
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn ensure_print_camera_ready() -> Result<(), String> {
    let options = low_memory_xiao_esp32s3_sense_print_camera();
    let frame_size = parse_frame_size(options.frame_size)?;
    let pixel_format = parse_pixel_format(options.pixel_format)?;
    ensure_camera_initialized(&options, frame_size, pixel_format)?;
    println!("[matchbox] print camera profile initialized");
    Ok(())
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn shutdown_camera() -> Result<(), String> {
    Err(
        "ESP32 camera component is not enabled in this ESP-IDF build. Clean the runner target so esp-idf-sys regenerates with espressif/esp32-camera enabled."
            .to_string(),
    )
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn shutdown_camera() -> Result<(), String> {
    let state = camera_state();
    let mut state = state
        .lock()
        .map_err(|_| "camera state lock poisoned".to_string())?;

    free_photo(None)?;

    if state.initialized {
        unsafe {
            esp_ok(camera::esp_camera_deinit(), "esp_camera_deinit")?;
        }
        state.initialized = false;
        state.current_signature.clear();
        println!("[matchbox] camera shutdown complete");
    }

    Ok(())
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
fn esp_ok(code: i32, action: &str) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("{} failed with esp_err_t={}", action, code))
    }
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
fn parse_frame_size(input: &str) -> Result<camera::framesize_t, String> {
    let normalized = input.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "96x96" => Ok(camera::framesize_t_FRAMESIZE_96X96),
        "qqvga" => Ok(camera::framesize_t_FRAMESIZE_QQVGA),
        "128x128" => Ok(camera::framesize_t_FRAMESIZE_128X128),
        "qcif" => Ok(camera::framesize_t_FRAMESIZE_QCIF),
        "hqvga" => Ok(camera::framesize_t_FRAMESIZE_HQVGA),
        "240x240" => Ok(camera::framesize_t_FRAMESIZE_240X240),
        "qvga" => Ok(camera::framesize_t_FRAMESIZE_QVGA),
        "320x320" => Ok(camera::framesize_t_FRAMESIZE_320X320),
        "cif" => Ok(camera::framesize_t_FRAMESIZE_CIF),
        "hvga" => Ok(camera::framesize_t_FRAMESIZE_HVGA),
        "vga" => Ok(camera::framesize_t_FRAMESIZE_VGA),
        "svga" => Ok(camera::framesize_t_FRAMESIZE_SVGA),
        "xga" => Ok(camera::framesize_t_FRAMESIZE_XGA),
        "hd" => Ok(camera::framesize_t_FRAMESIZE_HD),
        "sxga" => Ok(camera::framesize_t_FRAMESIZE_SXGA),
        "uxga" => Ok(camera::framesize_t_FRAMESIZE_UXGA),
        _ => Err(format!("Unsupported frameSize '{}'", input)),
    }
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
fn frame_size_dimensions(input: &str) -> (u32, u32) {
    match input.trim().to_ascii_lowercase().as_str() {
        "96x96" => (96, 96),
        "qqvga" => (160, 120),
        "128x128" => (128, 128),
        "qcif" => (176, 144),
        "hqvga" => (240, 176),
        "240x240" => (240, 240),
        "qvga" => (320, 240),
        "320x320" => (320, 320),
        "cif" => (400, 296),
        "hvga" => (480, 320),
        "vga" => (640, 480),
        "svga" => (800, 600),
        "xga" => (1024, 768),
        "hd" => (1280, 720),
        "sxga" => (1280, 1024),
        "uxga" => (1600, 1200),
        _ => (0, 0),
    }
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
fn parse_pixel_format(input: &str) -> Result<camera::pixformat_t, String> {
    if input.eq_ignore_ascii_case("jpeg") {
        Ok(camera::pixformat_t_PIXFORMAT_JPEG)
    } else if input.eq_ignore_ascii_case("grayscale") {
        Ok(camera::pixformat_t_PIXFORMAT_GRAYSCALE)
    } else {
        Err(format!(
            "Unsupported pixelFormat '{}'; embedded runner supports jpeg and grayscale only",
            input
        ))
    }
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
#[derive(Clone, Debug)]
struct CameraRuntimeState {
    initialized: bool,
    current_signature: String,
    next_photo_id: u64,
    next_capture_version: u64,
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
fn camera_state() -> &'static StdMutex<CameraRuntimeState> {
    static CAMERA_STATE: OnceLock<StdMutex<CameraRuntimeState>> = OnceLock::new();
    CAMERA_STATE.get_or_init(|| {
        StdMutex::new(CameraRuntimeState {
            initialized: false,
            current_signature: String::new(),
            next_photo_id: 1,
            next_capture_version: 1,
        })
    })
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
fn photo_store() -> &'static StdMutex<HashMap<u64, StoredCapture>> {
    static PHOTO_STORE: OnceLock<StdMutex<HashMap<u64, StoredCapture>>> = OnceLock::new();
    PHOTO_STORE.get_or_init(|| StdMutex::new(HashMap::new()))
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
fn options_signature(options: &Esp32CameraOptions) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        options.frame_size,
        options.pixel_format,
        options.jpeg_quality,
        options.fb_count,
        options.xclk_frequency,
        options.pins.pin_pwdn,
        options.pins.pin_reset,
        options.pins.pin_xclk,
        options.pins.pin_siod,
        options.pins.pin_sioc,
        options.pins.pin_d0,
        options.pins.pin_d1,
        options.pins.pin_d2,
        options.pins.pin_d3,
        options.pins.pin_d4,
        options.pins.pin_d5,
        options.pins.pin_d6,
        options.pins.pin_d7,
        options.pins.pin_vsync,
        options.pins.pin_href,
        options.pins.pin_pclk,
        options.jpeg_quality
    )
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
fn build_camera_config(
    options: &Esp32CameraOptions,
    frame_size: camera::framesize_t,
    pixel_format: camera::pixformat_t,
) -> camera::camera_config_t {
    let pins = &options.pins;
    let mut config: camera::camera_config_t = unsafe { std::mem::zeroed() };
    config.pin_pwdn = pins.pin_pwdn;
    config.pin_reset = pins.pin_reset;
    config.pin_xclk = pins.pin_xclk;
    config.__bindgen_anon_1.pin_sccb_sda = pins.pin_siod;
    config.__bindgen_anon_2.pin_sccb_scl = pins.pin_sioc;
    config.pin_d0 = pins.pin_d0;
    config.pin_d1 = pins.pin_d1;
    config.pin_d2 = pins.pin_d2;
    config.pin_d3 = pins.pin_d3;
    config.pin_d4 = pins.pin_d4;
    config.pin_d5 = pins.pin_d5;
    config.pin_d6 = pins.pin_d6;
    config.pin_d7 = pins.pin_d7;
    config.pin_vsync = pins.pin_vsync;
    config.pin_href = pins.pin_href;
    config.pin_pclk = pins.pin_pclk;
    config.xclk_freq_hz = options.xclk_frequency as i32;
    config.ledc_timer = camera::ledc_timer_t_LEDC_TIMER_0;
    config.ledc_channel = camera::ledc_channel_t_LEDC_CHANNEL_0;
    config.pixel_format = pixel_format;
    config.frame_size = frame_size;
    config.jpeg_quality = options.jpeg_quality;
    config.fb_count = options.fb_count.max(1) as usize;
    config.grab_mode = camera::camera_grab_mode_t_CAMERA_GRAB_LATEST;
    if cfg!(feature = "psram") {
        config.fb_location = camera::camera_fb_location_t_CAMERA_FB_IN_PSRAM;
    } else {
        config.fb_location = camera::camera_fb_location_t_CAMERA_FB_IN_DRAM;
    }
    config
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
fn ensure_camera_initialized(
    options: &Esp32CameraOptions,
    frame_size: camera::framesize_t,
    pixel_format: camera::pixformat_t,
) -> Result<(), String> {
    let signature = options_signature(options);
    let state = camera_state();
    let mut state = state.lock().map_err(|_| "camera state lock poisoned".to_string())?;

    unsafe {
        esp_idf_sys::link_patches();

        if state.initialized {
            if state.current_signature == signature {
                return Ok(());
            }

            esp_ok(camera::esp_camera_deinit(), "esp_camera_deinit")?;
            state.initialized = false;
            state.current_signature.clear();
        }

        let config = build_camera_config(options, frame_size, pixel_format);
        esp_ok(camera::esp_camera_init(&config), "esp_camera_init")?;
        state.initialized = true;
        state.current_signature = signature;
    }

    Ok(())
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn store_latest_capture(capture: &Esp32Capture) -> Result<(), String> {
    let owned = Esp32Capture {
        width: capture.width,
        height: capture.height,
        format: capture.format,
        bytes: capture.bytes.clone(),
    };
    store_latest_capture_owned(owned).map(|_| ())
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn store_latest_capture_owned(capture: Esp32Capture) -> Result<StoredCaptureMeta, String> {
    let state = camera_state();
    let mut state = state
        .lock()
        .map_err(|_| "camera state lock poisoned".to_string())?;
    let id = state.next_photo_id;
    state.next_photo_id = state.next_photo_id.saturating_add(1);
    let version = state.next_capture_version;
    state.next_capture_version = state.next_capture_version.saturating_add(1);
    drop(state);

    let bytes_len = capture.bytes.len();
    let format = capture.format.to_string();
    let width = capture.width;
    let height = capture.height;
    let bytes = capture.bytes;
    let store = photo_store();
    let mut store = store
        .lock()
        .map_err(|_| "photo store lock poisoned".to_string())?;
    let frame = Box::new(bytes);
    let frame_ptr = Box::into_raw(frame) as usize;
    store.insert(id, StoredCapture {
        id,
        version,
        width,
        height,
        format: format.clone(),
        frame_ptr,
        bytes_len,
    });
    Ok(StoredCaptureMeta { id, version, width, height, format, bytes_len })
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn capture_photo_handle(options: &Esp32CameraOptions) -> Result<StoredCaptureMeta, String> {
    let frame_size = parse_frame_size(options.frame_size)?;
    let pixel_format = parse_pixel_format(options.pixel_format)?;
    let (width_hint, height_hint) = frame_size_dimensions(options.frame_size);
    ensure_camera_initialized(options, frame_size, pixel_format)?;

    let state = camera_state();
    let mut state = state
        .lock()
        .map_err(|_| "camera state lock poisoned".to_string())?;
    let id = state.next_photo_id;
    state.next_photo_id = state.next_photo_id.saturating_add(1);
    let version = state.next_capture_version;
    state.next_capture_version = state.next_capture_version.saturating_add(1);
    drop(state);

    unsafe {
        let frame = camera::esp_camera_fb_get();
        if frame.is_null() {
            return Err("esp_camera_fb_get() returned null".to_string());
        }

        let width = if (*frame).width > 0 {
            (*frame).width as u32
        } else {
            width_hint
        };
        let height = if (*frame).height > 0 {
            (*frame).height as u32
        } else {
            height_hint
        };
        let bytes_len = (*frame).len as usize;
        let format = "jpeg".to_string();

        let store = photo_store();
        let mut store = store
            .lock()
            .map_err(|_| "photo store lock poisoned".to_string())?;
        store.insert(id, StoredCapture {
            id,
            version,
            width,
            height,
            format: format.clone(),
            frame_ptr: frame as usize,
            bytes_len,
        });

        Ok(StoredCaptureMeta {
            id,
            version,
            width,
            height,
            format,
            bytes_len,
        })
    }
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn latest_capture() -> Result<Option<Esp32Capture>, String> {
    let store = photo_store();
    let store = store
        .lock()
        .map_err(|_| "photo store lock poisoned".to_string())?;
    let capture = store.values().max_by_key(|capture| capture.version);
    Ok(capture.map(|capture| unsafe {
        let frame = frame_ptr_as_ref(capture.frame_ptr);
        let bytes =
            std::slice::from_raw_parts(frame.buf, capture.bytes_len).to_vec();
        Esp32Capture {
            width: capture.width,
            height: capture.height,
            format: if capture.format.eq_ignore_ascii_case("jpeg") {
                "jpeg"
            } else {
                "jpeg"
            },
            bytes,
        }
    }))
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn latest_capture_version() -> Result<Option<u64>, String> {
    let store = photo_store();
    let store = store
        .lock()
        .map_err(|_| "photo store lock poisoned".to_string())?;
    Ok(store.values().map(|capture| capture.version).max())
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn clear_latest_capture() -> Result<(), String> {
    free_photo(None)
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn with_latest_capture<R>(
    f: impl FnOnce(Option<(&str, &[u8], u64, u32, u32)>) -> R,
) -> Result<R, String> {
    let store = photo_store();
    let store = store
        .lock()
        .map_err(|_| "photo store lock poisoned".to_string())?;
    let capture = store.values().max_by_key(|capture| capture.version);
    Ok(match capture {
        Some(capture) => f(Some((
            capture.format.as_str(),
            unsafe {
                let frame = frame_ptr_as_ref(capture.frame_ptr);
                std::slice::from_raw_parts(frame.buf, capture.bytes_len)
            },
            capture.version,
            capture.width,
            capture.height,
        ))),
        None => f(None),
    })
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn photo_info(id: u64) -> Result<Option<StoredCaptureMeta>, String> {
    let store = photo_store();
    let store = store
        .lock()
        .map_err(|_| "photo store lock poisoned".to_string())?;
    Ok(store.get(&id).map(|capture| StoredCaptureMeta {
        id: capture.id,
        version: capture.version,
        width: capture.width,
        height: capture.height,
        format: capture.format.clone(),
        bytes_len: capture.bytes_len,
    }))
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn free_photo(id: Option<u64>) -> Result<(), String> {
    let store = photo_store();
    let mut store = store
        .lock()
        .map_err(|_| "photo store lock poisoned".to_string())?;
    match id {
        Some(id) => {
            if let Some(capture) = store.remove(&id) {
                unsafe { camera::esp_camera_fb_return(capture.frame_ptr as *mut camera::camera_fb_t) };
            }
        }
        None => {
            for (_, capture) in store.drain() {
                unsafe { camera::esp_camera_fb_return(capture.frame_ptr as *mut camera::camera_fb_t) };
            }
        }
    }
    Ok(())
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn with_photo<R>(
    id: u64,
    f: impl FnOnce(Option<(&str, &[u8], u64, u32, u32)>) -> R,
) -> Result<R, String> {
    let store = photo_store();
    let store = store
        .lock()
        .map_err(|_| "photo store lock poisoned".to_string())?;
    Ok(match store.get(&id) {
        Some(capture) => f(Some((
            capture.format.as_str(),
            unsafe {
                let frame = frame_ptr_as_ref(capture.frame_ptr);
                std::slice::from_raw_parts(frame.buf, capture.bytes_len)
            },
            capture.version,
            capture.width,
            capture.height,
        ))),
        None => f(None),
    })
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
// This is intentionally runner-owned. The embedded runtime should call direct
// ESP32 camera primitives here instead of going back through the general
// MatchBox module object model.
pub fn capture_frame(options: &Esp32CameraOptions) -> Result<Esp32Capture, String> {
    let frame_size = parse_frame_size(options.frame_size)?;
    let pixel_format = parse_pixel_format(options.pixel_format)?;
    let (width_hint, height_hint) = frame_size_dimensions(options.frame_size);
    ensure_camera_initialized(options, frame_size, pixel_format)?;

    unsafe {
        let frame = camera::esp_camera_fb_get();
        if frame.is_null() {
            return Err("esp_camera_fb_get() returned null".to_string());
        }

        let bytes = std::slice::from_raw_parts((*frame).buf, (*frame).len as usize).to_vec();
        let width = if (*frame).width > 0 {
            (*frame).width as u32
        } else {
            width_hint
        };
        let height = if (*frame).height > 0 {
            (*frame).height as u32
        } else {
            height_hint
        };

        camera::esp_camera_fb_return(frame);

        Ok(Esp32Capture {
            bytes,
            format: if options.pixel_format.eq_ignore_ascii_case("grayscale") {
                "grayscale"
            } else {
                "jpeg"
            },
            width,
            height,
        })
    }
}

#[cfg(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
))]
pub fn capture_jpeg(options: &Esp32CameraOptions) -> Result<Esp32Capture, String> {
    let capture = capture_frame(options)?;
    if !capture.format.eq_ignore_ascii_case("jpeg") {
        return Err(format!(
            "camera returned '{}' but JPEG was required",
            capture.format
        ));
    }
    Ok(capture)
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn capture_jpeg(_options: &Esp32CameraOptions) -> Result<Esp32Capture, String> {
    Err(
        "ESP32 camera component is not enabled in this ESP-IDF build. Clean the runner target so esp-idf-sys regenerates with espressif/esp32-camera enabled."
            .to_string(),
    )
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn ensure_print_camera_ready() -> Result<(), String> {
    Err(
        "ESP32 camera component is not enabled in this ESP-IDF build. Clean the runner target so esp-idf-sys regenerates with espressif/esp32-camera enabled."
            .to_string(),
    )
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn capture_frame(_options: &Esp32CameraOptions) -> Result<Esp32Capture, String> {
    Err(
        "ESP32 camera component is not enabled in this ESP-IDF build. Clean the runner target so esp-idf-sys regenerates with espressif/esp32-camera enabled."
            .to_string(),
    )
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn store_latest_capture(_capture: &Esp32Capture) -> Result<(), String> {
    Err(
        "ESP32 camera component is not enabled in this ESP-IDF build."
            .to_string(),
    )
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn store_latest_capture_owned(_capture: Esp32Capture) -> Result<StoredCaptureMeta, String> {
    Err(
        "ESP32 camera component is not enabled in this ESP-IDF build."
            .to_string(),
    )
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn capture_photo_handle(_options: &Esp32CameraOptions) -> Result<StoredCaptureMeta, String> {
    Err(
        "ESP32 camera component is not enabled in this ESP-IDF build."
            .to_string(),
    )
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn latest_capture() -> Result<Option<Esp32Capture>, String> {
    Ok(None)
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn latest_capture_version() -> Result<Option<u64>, String> {
    Ok(None)
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn clear_latest_capture() -> Result<(), String> {
    Ok(())
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn with_latest_capture<R>(
    f: impl FnOnce(Option<(&str, &[u8], u64, u32, u32)>) -> R,
) -> Result<R, String> {
    Ok(f(None))
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn photo_info(_id: u64) -> Result<Option<StoredCaptureMeta>, String> {
    Ok(None)
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn free_photo(_id: Option<u64>) -> Result<(), String> {
    Ok(())
}

#[cfg(not(all(
    matchbox_camera_supported,
    any(
        esp_idf_comp_esp32_camera_enabled,
        esp_idf_comp_espressif__esp32_camera_enabled
    )
)))]
pub fn with_photo<R>(
    _id: u64,
    f: impl FnOnce(Option<(&str, &[u8], u64, u32, u32)>) -> R,
) -> Result<R, String> {
    Ok(f(None))
}
