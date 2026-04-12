pub mod boot;
pub mod camera;
pub mod esp32_bifs;
pub mod features;
pub mod imaging;
pub mod mdns;
pub mod platform;
pub mod printer;
pub mod profile;
pub mod web;
pub mod wifi;

use anyhow::Result;
use boot::BootRuntime;
use features::BundledFeatures;
use profile::StrictProfile;

pub fn main_entry() -> Result<()> {
    esp_idf_sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let profile = StrictProfile::from_env();
    let features = BundledFeatures::from_compiled_features();
    let runtime = BootRuntime::new(profile, features);

    runtime.start()
}
