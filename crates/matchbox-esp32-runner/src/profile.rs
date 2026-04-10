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
            wifi_ssid: option_env!("MATCHBOX_ESP32_WIFI_SSID").unwrap_or("Pixel_174"),
            wifi_password: option_env!("MATCHBOX_ESP32_WIFI_PASSWORD").unwrap_or("myinternetpass"),
            wifi_hostname: option_env!("MATCHBOX_ESP32_WIFI_HOSTNAME").unwrap_or("roastatron3k"),
            web_port: option_env!("MATCHBOX_ESP32_WEB_PORT")
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(8080),
        }
    }
}
