#[derive(Clone, Debug)]
pub struct StrictProfile {
    pub name: &'static str,
    pub tree_shake_target: &'static str,
    pub dynamic_web_features: bool,
    pub dynamic_module_loading: bool,
    pub reflection: bool,
    pub template_rendering: bool,
    pub wifi_ssid: &'static str,
    pub wifi_password: &'static str,
    pub wifi_hostname: &'static str,
    pub web_port: u16,
}

impl StrictProfile {
    pub fn from_env() -> Self {
        Self {
            name: "esp32-strict",
            tree_shake_target: "embedded",
            dynamic_web_features: false,
            dynamic_module_loading: false,
            reflection: false,
            template_rendering: false,
            wifi_ssid: option_env!("MATCHBOX_ESP32_WIFI_SSID").unwrap_or(""),
            wifi_password: option_env!("MATCHBOX_ESP32_WIFI_PASSWORD").unwrap_or(""),
            wifi_hostname: option_env!("MATCHBOX_ESP32_WIFI_HOSTNAME").unwrap_or("matchbox-esp32"),
            web_port: option_env!("MATCHBOX_ESP32_WEB_PORT")
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(80),
        }
    }

    /// Firmware carries no Wi-Fi credentials unless they are set at build time.
    pub fn has_wifi_credentials(&self) -> bool {
        !self.wifi_ssid.is_empty()
    }
}

pub const NO_WIFI_CREDENTIALS_MESSAGE: &str = "[matchbox] No Wi-Fi credentials configured: Wi-Fi is disabled. \
Set MATCHBOX_ESP32_WIFI_SSID and MATCHBOX_ESP32_WIFI_PASSWORD when building the firmware.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_has_no_baked_in_wifi_credentials() {
        let profile = StrictProfile::from_env();
        if option_env!("MATCHBOX_ESP32_WIFI_SSID").is_none() {
            assert_eq!(profile.wifi_ssid, "");
            assert!(!profile.has_wifi_credentials());
        }
        if option_env!("MATCHBOX_ESP32_WIFI_PASSWORD").is_none() {
            assert_eq!(profile.wifi_password, "");
        }
    }

    #[test]
    fn credentials_are_detected_from_the_ssid() {
        let mut profile = StrictProfile::from_env();
        profile.wifi_ssid = "home";
        assert!(profile.has_wifi_credentials());
        profile.wifi_ssid = "";
        assert!(!profile.has_wifi_credentials());
    }
}
