#[derive(Clone, Copy, Debug)]
pub struct BundledFeatures {
    pub web: bool,
    pub mdns: bool,
    pub camera: bool,
    pub bluetooth: bool,
    pub pins: bool,
    pub sdcard: bool,
    pub printer: bool,
    pub psram: bool,
}

impl BundledFeatures {
    pub fn from_compiled_features() -> Self {
        Self {
            web: cfg!(feature = "platform-web"),
            mdns: cfg!(feature = "platform-mdns"),
            camera: cfg!(feature = "platform-camera"),
            bluetooth: cfg!(feature = "platform-bluetooth"),
            pins: cfg!(feature = "platform-pins"),
            sdcard: cfg!(feature = "platform-sdcard"),
            printer: cfg!(feature = "platform-printer"),
            psram: cfg!(feature = "psram"),
        }
    }

    pub fn describe(&self) -> String {
        let mut enabled = Vec::new();

        if self.web {
            enabled.push("web");
        }
        if self.mdns {
            enabled.push("mdns");
        }
        if self.camera {
            enabled.push("camera");
        }
        if self.bluetooth {
            enabled.push("bluetooth");
        }
        if self.pins {
            enabled.push("pins");
        }
        if self.sdcard {
            enabled.push("sdcard");
        }
        if self.printer {
            enabled.push("printer");
        }
        if self.psram {
            enabled.push("psram");
        }

        if enabled.is_empty() {
            "none".to_string()
        } else {
            enabled.join(", ")
        }
    }
}
