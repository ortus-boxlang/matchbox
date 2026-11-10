//! The Native Fusion SDK: the crates a generated fusion build compiles against, embedded into the
//! binary by `build.rs` so a prebuilt MatchBox needs no source checkout.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

include!(concat!(env!("OUT_DIR"), "/fusion_sdk.rs"));

/// Writes the SDK crates under `dest` as `dest/crates/<crate>/...`.
pub fn write_to(dest: &Path) -> Result<()> {
    for (rel, bytes) in SDK_FILES {
        let path = dest.join(rel);
        let parent = path.parent().context("SDK file path has no parent")?;
        fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
        fs::write(&path, bytes).with_context(|| format!("Failed to write {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The crate directory names (under `crates/`) that the embedded SDK contains.
    fn embedded_crates() -> BTreeSet<String> {
        SDK_FILES
            .iter()
            .filter_map(|(rel, _)| rel.strip_suffix("/Cargo.toml"))
            .filter_map(|dir| dir.strip_prefix("crates/"))
            .map(str::to_string)
            .collect()
    }

    /// Every `path = "..."` dependency declared in a crate manifest, across the dependency tables.
    fn path_dependencies(manifest: &toml::Value) -> Vec<String> {
        /// Collects the `path` of each dependency in one dependency table.
        fn from_table(table: Option<&toml::Value>, out: &mut Vec<String>) {
            if let Some(deps) = table.and_then(|t| t.as_table()) {
                for dep in deps.values() {
                    if let Some(path) = dep.get("path").and_then(|p| p.as_str()) {
                        out.push(path.to_string());
                    }
                }
            }
        }
        let mut out = Vec::new();
        from_table(manifest.get("dependencies"), &mut out);
        from_table(manifest.get("build-dependencies"), &mut out);
        if let Some(targets) = manifest.get("target").and_then(|t| t.as_table()) {
            for target in targets.values() {
                from_table(target.get("dependencies"), &mut out);
            }
        }
        out
    }

    #[test]
    fn embeds_the_vm_crate_and_the_lockfile() {
        let crates = embedded_crates();
        assert!(crates.contains("matchbox-vm"), "embedded crates: {crates:?}");
        assert!(!CARGO_LOCK.is_empty());
    }

    #[test]
    fn every_path_dependency_of_an_embedded_crate_is_embedded() {
        let crates = embedded_crates();
        for (rel, bytes) in SDK_FILES.iter().filter(|(rel, _)| rel.ends_with("/Cargo.toml")) {
            let manifest: toml::Value = toml::from_str(std::str::from_utf8(bytes).unwrap()).unwrap();
            for dep in path_dependencies(&manifest) {
                let name = dep.trim_start_matches("../").trim_end_matches('/');
                assert!(
                    crates.contains(name),
                    "{rel} depends on {dep}, which is not in FUSION_SDK_CRATES in build.rs"
                );
            }
        }
    }

    #[test]
    fn leaves_out_tests_and_build_output() {
        for (rel, _) in SDK_FILES {
            let parts: Vec<&str> = rel.split('/').collect();
            assert!(
                !parts.iter().any(|p| matches!(*p, "target" | "tests" | "benches" | "examples")),
                "unexpected embedded file {rel}"
            );
        }
    }

    #[test]
    fn writes_a_layout_cargo_can_resolve() {
        let dir = std::env::temp_dir().join(format!("matchbox-fusion-sdk-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        write_to(&dir).unwrap();
        for name in embedded_crates() {
            assert!(dir.join("crates").join(&name).join("Cargo.toml").exists(), "{name}");
        }
        assert!(dir.join("crates/matchbox-vm/src/lib.rs").exists());
        assert!(dir.join("crates/matchbox-vm/build.rs").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
