fn main() {
    println!("cargo:rustc-env=MATCHBOX_TARGET={}", std::env::var("TARGET").unwrap());
}
