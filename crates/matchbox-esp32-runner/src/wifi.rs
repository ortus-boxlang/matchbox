use crate::profile::StrictProfile;
use anyhow::Result;
use core::convert::TryInto;
use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration as WifiConfiguration};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::handle::RawHandle;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use std::ffi::CString;

pub struct WifiState {
    pub ip: String,
    _wifi: BlockingWifi<EspWifi<'static>>,
}

pub fn connect(profile: &StrictProfile) -> Result<WifiState> {
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    let hostname = CString::new(profile.wifi_hostname)?;
    esp_idf_sys::esp!(unsafe {
        esp_idf_sys::esp_netif_set_hostname(
            wifi.wifi().sta_netif().handle(),
            hostname.as_ptr() as *const _,
        )
    })?;

    let configuration = WifiConfiguration::Client(ClientConfiguration {
        ssid: profile.wifi_ssid.try_into().unwrap(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: profile.wifi_password.try_into().unwrap(),
        channel: None,
        ..Default::default()
    });

    wifi.set_configuration(&configuration)?;
    wifi.start()?;
    println!("[matchbox] Wi-Fi started for SSID '{}'", profile.wifi_ssid);

    wifi.connect()?;
    println!("[matchbox] Wi-Fi connected");

    wifi.wait_netif_up()?;
    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
    let ip = ip_info.ip.to_string();
    println!(
        "[matchbox] Wi-Fi ready. hostname='{}' ip={}",
        profile.wifi_hostname, ip
    );

    Ok(WifiState { ip, _wifi: wifi })
}
