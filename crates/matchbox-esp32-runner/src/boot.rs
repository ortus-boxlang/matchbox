use crate::features::BundledFeatures;
use crate::platform::PlatformServices;
use crate::profile::StrictProfile;
use anyhow::Result;
use std::ffi::c_void;

pub struct BootRuntime {
    profile: StrictProfile,
    features: BundledFeatures,
}

struct RunnerTaskContext {
    profile: StrictProfile,
    features: BundledFeatures,
}

extern "C" fn runner_task_main(arg: *mut c_void) {
    let ctx = unsafe { Box::from_raw(arg as *mut RunnerTaskContext) };
    let services = PlatformServices::new(ctx.features);
    if let Err(error) = services.run_forever(&ctx.profile) {
        eprintln!("[matchbox] Platform services failed: {}", error);
    }
    unsafe {
        esp_idf_sys::vTaskDelete(std::ptr::null_mut());
    }
}

impl BootRuntime {
    pub fn new(profile: StrictProfile, features: BundledFeatures) -> Self {
        Self { profile, features }
    }

    pub fn start(&self) -> Result<()> {
        println!("[matchbox] ESP32 bundled runner starting");
        println!(
            "[matchbox] strict profile = {}, tree-shake target = {}",
            self.profile.name, self.profile.tree_shake_target
        );
        println!("[matchbox] bundled features = {}", self.features.describe());

        let services = PlatformServices::new(self.features);
        services.log_startup_summary();

        let ctx = Box::new(RunnerTaskContext {
            profile: self.profile.clone(),
            features: self.features,
        });
        let ctx_ptr = Box::into_raw(ctx) as *mut c_void;

        let task_name = std::ffi::CString::new("matchbox-runner").unwrap();
        let created = unsafe {
            esp_idf_sys::xTaskCreatePinnedToCore(
                Some(runner_task_main),
                task_name.as_ptr(),
                24576,
                ctx_ptr,
                5,
                std::ptr::null_mut(),
                0,
            )
        };

        if created != 1 {
            unsafe {
                drop(Box::from_raw(ctx_ptr as *mut RunnerTaskContext));
            }
            anyhow::bail!("[matchbox] Failed to create runner task");
        }

        loop {
            unsafe { esp_idf_sys::vTaskDelay(1000) };
        }
    }
}
