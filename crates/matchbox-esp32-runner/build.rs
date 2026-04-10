fn main() {
    println!("cargo:rerun-if-env-changed=MATCHBOX_EMBEDDED_ROUTE_TABLE");
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dest_path = out_dir.join("embedded-route-table.json");
    let sdkconfig_dest_path = out_dir.join("sdkconfig.defaults.generated");

    if let Ok(route_table_path) = std::env::var("MATCHBOX_EMBEDDED_ROUTE_TABLE") {
        if !route_table_path.is_empty() {
            println!("cargo:rerun-if-changed={route_table_path}");
            std::fs::copy(route_table_path, dest_path)
                .expect("Failed to copy embedded route table");
        }
    } else {
        std::fs::write(dest_path, [])
            .expect("Failed to write default embedded route table");
    }

    if std::env::var_os("CARGO_FEATURE_PSRAM").is_some() {
        let psram_defaults = std::path::PathBuf::from("sdkconfig.defaults.psram");
        println!("cargo:rerun-if-changed={}", psram_defaults.display());
        std::fs::copy(&psram_defaults, &sdkconfig_dest_path)
            .expect("Failed to copy PSRAM sdkconfig defaults");
        println!(
            "cargo:rustc-env=MATCHBOX_PSRAM_SDKCONFIG_DEFAULTS={}",
            sdkconfig_dest_path.display()
        );
    }

    embuild::espidf::sysenv::output();
}
