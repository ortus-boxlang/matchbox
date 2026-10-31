//! Firmware must not carry default Wi-Fi credentials: they are configured at build time or not at all.

use std::{fs, path::Path};

fn firmware_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("src/esp32_fusion_runner_template.rs.txt")];
    let runner_src = root.join("crates/matchbox-esp32-runner/src");
    for entry in fs::read_dir(&runner_src).expect("esp32 runner sources") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path).expect("readable source");
            (path.display().to_string(), text)
        })
        .collect()
}

#[test]
fn firmware_sources_have_no_baked_in_wifi_credentials() {
    for (path, text) in firmware_sources() {
        for needle in ["MATCHBOX_ESP32_WIFI_SSID", "MATCHBOX_ESP32_WIFI_PASSWORD"] {
            let mut rest = text.as_str();
            while let Some(at) = rest.find(needle) {
                let after = &rest[at + needle.len()..];
                let window: String = after.chars().take(120).collect();
                if let Some(default) = window.split(".unwrap_or(\"").nth(1) {
                    let value = default.split('"').next().unwrap_or("");
                    assert!(
                        value.is_empty(),
                        "{path}: {needle} has a non-empty default {value:?}"
                    );
                }
                rest = after;
            }
        }
        assert!(!text.contains("Pixel_174"), "{path} still contains a default SSID");
        assert!(!text.contains("myinternetpass"), "{path} still contains a default password");
    }
}
