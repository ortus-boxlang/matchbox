use crate::features::BundledFeatures;
use crate::profile::StrictProfile;
use crate::printer;
use crate::{mdns, web, wifi};
use anyhow::Result;

#[derive(Clone, Copy, Debug)]
pub struct PlatformServices {
    features: BundledFeatures,
}

impl PlatformServices {
    pub fn new(features: BundledFeatures) -> Self {
        Self { features }
    }

    pub fn log_startup_summary(&self) {
        if self.features.psram {
            println!("[matchbox] PSRAM-enabled build requested");
        }
        if self.features.web {
            println!("[matchbox] bundled web routing enabled");
        }
        if self.features.mdns {
            println!("[matchbox] bundled mDNS enabled");
        }
        if self.features.camera {
            println!("[matchbox] bundled camera access enabled");
        }
        if self.features.bluetooth {
            println!("[matchbox] bundled bluetooth enabled");
        }
        if self.features.pins {
            println!("[matchbox] bundled pins enabled");
        }
        if self.features.sdcard {
            println!("[matchbox] bundled sdcard enabled");
        }
        if self.features.printer {
            println!("[matchbox] bundled printer helpers enabled");
        }
    }

    pub fn run_forever(&self, profile: &StrictProfile) -> Result<()> {
        if self.features.psram {
            unsafe {
                let total = esp_idf_sys::esp_psram_get_size();
                let free = esp_idf_sys::heap_caps_get_free_size(esp_idf_sys::MALLOC_CAP_SPIRAM);
                let total_heap =
                    esp_idf_sys::heap_caps_get_total_size(esp_idf_sys::MALLOC_CAP_SPIRAM);
                println!(
                    "[matchbox] PSRAM runtime total={} free={} heap_total={}",
                    total, free, total_heap
                );
            }
        }
        let wifi_state = wifi::connect(profile)?;

        #[cfg(feature = "platform-web")]
        if self.features.web {
            web::serve(profile, self.features, &wifi_state)?;
        }

        #[cfg(feature = "platform-mdns")]
        let _mdns = if self.features.mdns {
            Some(mdns::try_start(profile, profile.web_port)?)
        } else {
            None
        };

        println!("[matchbox] Platform services are running");
        loop {
            unsafe { esp_idf_sys::vTaskDelay(1000) };
        }
    }
}
