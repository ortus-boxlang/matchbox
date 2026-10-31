use crate::features::BundledFeatures;
#[cfg(feature = "platform-mdns")]
use crate::mdns;
use crate::profile::StrictProfile;
use crate::{web, wifi};
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
        self.log_psram_runtime();

        #[cfg(feature = "platform-web")]
        let (route_table, app_config) = if self.features.web {
            let route_table = web::load_executable_route_table();
            let app_config = web::run_application_start(&route_table)?;
            println!(
                "[matchbox] Application config: wifi_ssid={} web_port={}",
                app_config.wifi_ssid, app_config.web_port,
            );
            (Some(route_table), app_config)
        } else {
            (None, web::Esp32AppConfig::default())
        };

        let fallback_wifi_state = if wifi::active_ip().is_some() {
            None
        } else if profile.has_wifi_credentials() {
            Some(wifi::connect(profile)?)
        } else {
            println!("{}", crate::profile::NO_WIFI_CREDENTIALS_MESSAGE);
            None
        };
        let ip = wifi::active_ip()
            .or_else(|| fallback_wifi_state.as_ref().map(|state| state.ip.clone()))
            .unwrap_or_else(|| "0.0.0.0".to_string());

        #[cfg(feature = "platform-web")]
        if self.features.web {
            if let Some(route_table) = route_table {
                web::serve_with_route_table(profile, self.features, &ip, route_table)?;
            }
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

#[cfg(feature = "psram")]
impl PlatformServices {
    fn log_psram_runtime(&self) {
        if !self.features.psram {
            return;
        }

        unsafe {
            let total = esp_idf_sys::esp_psram_get_size();
            let free = esp_idf_sys::heap_caps_get_free_size(esp_idf_sys::MALLOC_CAP_SPIRAM);
            let total_heap = esp_idf_sys::heap_caps_get_total_size(esp_idf_sys::MALLOC_CAP_SPIRAM);
            println!(
                "[matchbox] PSRAM runtime total={} free={} heap_total={}",
                total, free, total_heap
            );
        }
    }
}

#[cfg(not(feature = "psram"))]
impl PlatformServices {
    fn log_psram_runtime(&self) {}
}
