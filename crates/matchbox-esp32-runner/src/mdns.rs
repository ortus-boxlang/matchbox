use crate::profile::StrictProfile;
use anyhow::Result;

#[cfg(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled))]
use esp_idf_svc::mdns::EspMdns;

#[cfg(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled))]
pub fn try_start(profile: &StrictProfile, port: u16) -> Result<EspMdns> {
    let mut mdns = EspMdns::take()?;
    mdns.set_hostname(profile.wifi_hostname)?;
    mdns.set_instance_name(profile.wifi_hostname)?;
    mdns.add_service(
        Some(profile.wifi_hostname),
        "_http",
        "_tcp",
        port,
        &[("board", "matchbox-esp32"), ("hostname", profile.wifi_hostname)],
    )?;
    println!(
        "[matchbox] mDNS ready: http://{}.local:{}",
        profile.wifi_hostname, port
    );
    Ok(mdns)
}

#[cfg(not(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled)))]
pub fn try_start(profile: &StrictProfile, port: u16) -> Result<()> {
    println!(
        "[matchbox] mDNS component is not enabled in this ESP-IDF build; hostname '{}' will only be available by IP on port {}",
        profile.wifi_hostname, port
    );
    Ok(())
}
