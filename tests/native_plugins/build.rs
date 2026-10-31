use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var_os("CARGO_FEATURE_BUNDLED_LIBRARIES").is_none() {
        return;
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").unwrap(),
        "linux",
        "The dependent-library fixture currently targets Linux"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(
        out.join("leaf.c"),
        "int matchbox_fixture_leaf(void) { return 40; }\n",
    )
    .unwrap();
    fs::write(out.join("codec.c"), "extern int matchbox_fixture_leaf(void);\nint matchbox_fixture_codec(void) { return matchbox_fixture_leaf() + 2; }\n").unwrap();
    for (name, dependencies) in [("leaf", vec![]), ("codec", vec!["-lmatchbox_fixture_leaf"])] {
        let status = Command::new("cc")
            .current_dir(&out)
            .args(["-shared", "-fPIC", "-Wl,-rpath,$ORIGIN"])
            .arg(format!("-Wl,-soname,libmatchbox_fixture_{name}.so"))
            .arg(format!("{name}.c"))
            .args(["-L", "."])
            .args(dependencies)
            .arg("-o")
            .arg(format!("libmatchbox_fixture_{name}.so"))
            .status()
            .unwrap();
        assert!(
            status.success(),
            "Failed to build the {name} shared library"
        );
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=dylib=matchbox_fixture_codec");
    println!("cargo:rustc-cdylib-link-arg=-Wl,-rpath,$ORIGIN");
}
