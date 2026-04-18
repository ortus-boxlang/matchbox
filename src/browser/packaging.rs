use std::fs;
use std::path::{Path, PathBuf};
use std::env as std_env;
use anyhow::{Result, bail};
use postcard;
use matchbox_vm::Chunk;
use matchbox_compiler::ast;
use crate::stubs;
use crate::browser::bootstrap;
use matchbox_utility::try_log;

pub fn produce_js_bundle(chunk: &Chunk, source_path: &Path, ast: &[ast::Statement], output: Option<&Path>) -> Result<()> {
    try_log!("Producing standalone JS bundle...");
    let bytecode = postcard::to_stdvec(chunk)?;

    let wasm_bytes = stubs::get_stub("web").unwrap_or(&[]).to_vec();
    if wasm_bytes.is_empty() {
        bail!("Web runner stub is empty. The matchbox CLI must be rebuilt with the wasm32-unknown-unknown target installed.");
    }

    let out_path = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| source_path.with_extension("js"));
    try_log!("Output path determined: {}", out_path.display());

    let out_dir = out_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    try_log!("Output directory created: {}", out_dir.display());

    fs::create_dir_all(&out_dir)?;

    let stem: String = out_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("app")
        .to_string();

    try_log!("Using stem: {}", stem);

    let raw_wasm_path = out_dir.join(format!("{}.raw.wasm", stem));
    try_log!("Raw WASM path: {}", raw_wasm_path.display());
    fs::write(&raw_wasm_path, &wasm_bytes)?;

    try_log!( "Running wasm-bindgen to generate JS bindings...");
    let wasm_bindgen_bin = std_env::var("WASM_BINDGEN").unwrap_or_else(|_| "wasm-bindgen".to_string());
    match std::process::Command::new(wasm_bindgen_bin)
        .arg("--target").arg("web")
        .arg("--out-dir").arg(&out_dir)
        .arg("--out-name").arg(&stem)
        .arg(&raw_wasm_path)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => bail!("wasm-bindgen exited unsuccessfully: {status}"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            bail!("wasm-bindgen not found. Install `wasm-bindgen-cli` or set `WASM_BINDGEN`.");
        }
        Err(err) => return Err(err.into()),
    }

    let generated_js_path = out_dir.join(format!("{}.js", stem));
    try_log!("Generated JS path: {}", generated_js_path.display());
    let mut generated_js = fs::read_to_string(&generated_js_path)?;
    
    // Rename WASM file to {stem}.wasm
    let old_wasm_path = out_dir.join(format!("{}_bg.wasm", stem));
    let new_wasm_path = out_dir.join(format!("{}.wasm", stem));
    if old_wasm_path.exists() {
        fs::rename(&old_wasm_path, &new_wasm_path)?;
    }
    
    // Update JS to point to {stem}.wasm instead of {stem}_bg.wasm
    generated_js = generated_js.replace(&format!("{}_bg.wasm", stem), &format!("{}.wasm", stem));
    
    use base64::{Engine as _, engine::general_purpose};
    let b64_bytecode = general_purpose::STANDARD.encode(&bytecode);
    let bootstrap = bootstrap::render_stub_js_bootstrap(
        &bootstrap::exported_function_names(ast),
        &stem,
        &b64_bytecode,
    );
    fs::write(&generated_js_path, format!("{}\n{}", generated_js, bootstrap))?;

    let _ = fs::remove_file(&raw_wasm_path);
    println!("Standalone JS module produced: {}", generated_js_path.display());
    Ok(())
}

pub fn produce_fusion_js_bundle(
    wasm_artifact: &Path,
    source_path: &Path,
    ast: &[ast::Statement],
    output: Option<&Path>,
) -> Result<()> {
    try_log!("Producing JS fusion bundle...");
    let out_path = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| source_path.with_extension("js"));
    try_log!("Output path determined: {}", out_path.display());
    let out_dir = out_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    try_log!("Output directory created: {}", out_dir.display());
    fs::create_dir_all(&out_dir)?;

    let stem = out_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("app")
        .to_string();

    let wasm_bindgen_bin = std_env::var("WASM_BINDGEN").unwrap_or_else(|_| "wasm-bindgen".to_string());
    let status = std::process::Command::new(wasm_bindgen_bin)
        .arg("--target").arg("web")
        .arg("--out-dir").arg(&out_dir)
        .arg("--out-name").arg(&stem)
        .arg(wasm_artifact)
        .status()?;

    if !status.success() {
        bail!("Failed to run wasm-bindgen for JS fusion bundle");
    }

    let generated_js_path = out_dir.join(format!("{}.js", stem));
    let mut generated_js = fs::read_to_string(&generated_js_path)?;
    
    // Rename WASM file to {stem}.wasm
    let old_wasm_path = out_dir.join(format!("{}_bg.wasm", stem));
    let new_wasm_path = out_dir.join(format!("{}.wasm", stem));
    if old_wasm_path.exists() {
        fs::rename(&old_wasm_path, &new_wasm_path)?;
    }
    
    // Update JS to point to {stem}.wasm instead of {stem}_bg.wasm
    generated_js = generated_js.replace(&format!("{}_bg.wasm", stem), &format!("{}.wasm", stem));
    
    let bootstrap = bootstrap::render_fusion_js_bootstrap(&bootstrap::exported_function_names(ast), &stem);
    fs::write(&generated_js_path, format!("{}\n{}", generated_js, bootstrap))?;

    println!("Fusion JS module produced: {}", generated_js_path.display());
    Ok(())
}
