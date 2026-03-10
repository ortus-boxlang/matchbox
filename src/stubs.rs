use std::collections::HashMap;

pub fn get_stub(target: &str) -> Option<&'static [u8]> {
    let mut stubs: HashMap<&str, &[u8]> = HashMap::new();
    stubs.insert("wasi", include_bytes!("../stubs/runner_stub_wasip1.wasm"));
    stubs.insert("wasm32-wasip1", include_bytes!("../stubs/runner_stub_wasip1.wasm"));
    stubs.insert("host", include_bytes!("../stubs/runner_stub_aarch64-apple-darwin"));
    stubs.insert("aarch64-apple-darwin", include_bytes!("../stubs/runner_stub_aarch64-apple-darwin"));
    stubs.get(target).copied()
}
