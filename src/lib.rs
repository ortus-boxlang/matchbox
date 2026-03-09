use matchbox_compiler::{ast, compiler, parser};
use matchbox_vm::{types, vm, Chunk};

use std::env as std_env;
use std::fs;
use std::path::Path;
use std::io::{self, Write};
use anyhow::{Result, bail, Context};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

const MAGIC_FOOTER: &[u8; 8] = b"BOXLANG\x01";

mod stubs;

const JS_GLUE_TEMPLATE: &str = include_str!("js_bundle_template.js");

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_boxlang_bytecode(bytes: &[u8]) -> String {
    let res = (|| -> Result<String> {
        let chunk: Chunk = bincode::deserialize(bytes)?;
        let mut vm = vm::VM::new();
        let val = vm.interpret(chunk)?;
        Ok(val.to_string())
    })();

    match res {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_boxlang(source: &str) -> String {
    let res = (|| -> Result<String> {
        let ast = parser::parse(source)?;
        let compiler = compiler::Compiler::new("wasm_input");
        let chunk = compiler.compile(&ast, source)?;
        let mut vm = vm::VM::new();
        let val = vm.interpret(chunk)?;
        Ok(val.to_string())
    })();

    match res {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct BoxLangVM {
    vm: vm::VM,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BoxLangVM {
    #[wasm_bindgen(constructor)]
    pub fn new() -> BoxLangVM {
        BoxLangVM { vm: vm::VM::new() }
    }

    pub fn load_bytecode(&mut self, bytes: &[u8]) -> Result<(), String> {
        let res = (|| -> Result<()> {
            let chunk: Chunk = bincode::deserialize(bytes)?;
            self.vm.interpret(chunk)?;
            Ok(())
        })();

        res.map_err(|e| format!("Error: {}", e))
    }

    pub fn call(&mut self, name: &str, args: js_sys::Array) -> Result<JsValue, String> {
        let mut bx_args = Vec::new();
        for i in 0..args.length() {
            bx_args.push(self.vm.js_to_bx(args.get(i)));
        }

        let func = self.vm.get_global(name)
            .ok_or_else(|| format!("Function {} not found", name))?;

        match self.vm.call_function_value(func, bx_args) {
            Ok(val) => Ok(self.vm.bx_to_js(&val)),
            Err(e) => Err(format!("Error: {}", e)),
        }
    }
}

pub fn run() -> Result<()> {
    // 1. Check for WASM custom section first
    #[cfg(target_arch = "wasm32")]
    if let Ok(chunk) = load_wasm_custom_section() {
        return run_chunk(chunk);
    }

    // 2. Check for embedded bytecode at end of binary (Native)
    if let Ok(embedded_chunk) = load_embedded_bytecode() {
        return run_chunk(embedded_chunk);
    }

    let args: Vec<String> = std_env::args().collect();
    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        print_usage();
        return Ok(());
    }

    if args.contains(&"--version".to_string()) || args.contains(&"-v".to_string()) {
        print_version();
        return Ok(());
    }

    let is_build = args.contains(&"--build".to_string());
    let no_shaking = args.contains(&"--no-shaking".to_string());
    let no_std_lib = args.contains(&"--no-std-lib".to_string());
    let strip_source = args.contains(&"--strip-source".to_string());
    let target = if let Some(idx) = args.iter().position(|a| a == "--target") {
        args.get(idx + 1).map(|s| s.as_str())
    } else {
        None
    };

    let mut keep_symbols = Vec::new();
    if let Some(idx) = args.iter().position(|a| a == "--keep") {
        if let Some(val) = args.get(idx + 1) {
            keep_symbols = val.split(',').map(|s| s.trim().to_string()).collect();
        }
    }

    let output: Option<std::path::PathBuf> = if let Some(idx) = args.iter().position(|a| a == "--output") {
        args.get(idx + 1).map(|s| std::path::PathBuf::from(s))
    } else {
        None
    };

    let filename = args.iter().skip(1)
        .find(|a| !a.starts_with("--") && *a != "native" && *a != "wasm" && *a != "wasi" && *a != "js"
              && (args.iter().position(|arg| arg == *a).map(|pos| pos == 0 || args[pos-1] != "--target").unwrap_or(true))
              && (args.iter().position(|arg| arg == *a).map(|pos| pos == 0 || args[pos-1] != "--keep").unwrap_or(true))
              && (args.iter().position(|arg| arg == *a).map(|pos| pos == 0 || args[pos-1] != "--output").unwrap_or(true))
        );

    match filename {
        Some(name) => {
            let path = Path::new(name);
            if path.is_dir() {
                process_directory(path, is_build, target, keep_symbols, no_shaking, no_std_lib, strip_source, output.as_deref())?;
            } else {
                process_file(path, is_build, target, keep_symbols, no_shaking, no_std_lib, strip_source, output.as_deref())?;
            }
        }
        None => {
            if is_build || target.is_some() {
                bail!("No filename provided for build/target");
            }
            run_repl()?;
        }
    }

    Ok(())
}

fn print_usage() {
    println!("Usage: matchbox [options] [file.bxs|file.bxb|directory]");
    println!("\nOptions:");
    println!("  -h, --help          Show this help message");
    println!("  -v, --version       Show version information");
    println!("  --build             Compile to bytecode (.bxb)");
    println!("  --target <native>   Produce a standalone native binary");
    println!("  --target <wasi>     Produce a standalone WASI container binary");
    println!("  --target <wasm>     Produce a standalone WASM binary (Web)");
    println!("  --target <js>       Produce a JavaScript module wrapper");
    println!("  --keep <symbols>    Comma-separated list of BIFs to preserve");
    println!("  --no-shaking        Disable tree-shaking and include all prelude BIFs");
    println!("  --no-std-lib        Exclude the standard library (prelude) entirely");
    println!("  --output <path>     Set the output file path for compiled artifacts");
    println!("  --strip-source      Strip embedded source text from compiled output");
    println!("                      Errors still report file:line; native binaries fall back to disk for snippets");
    println!("\nIf no file is provided, matchbox starts in REPL mode.");
}

fn print_version() {
    let commit = env!("GIT_COMMIT");
    let date = env!("BUILD_DATE");
    let version = env!("CARGO_PKG_VERSION");
    println!("matchbox version {}", version);
    println!("commit: {}", commit);
    println!("built on: {}", date);
}

pub fn process_file(path: &Path, is_build: bool, target: Option<&str>, keep_symbols: Vec<String>, no_shaking: bool, no_std_lib: bool, strip_source: bool, output: Option<&Path>) -> Result<()> {
    if path.extension().and_then(|s| s.to_str()) == Some("bxb") {
        let bytes = fs::read(path)?;
        let chunk: Chunk = bincode::deserialize(&bytes)?;
        run_chunk(chunk)?;
    } else {
        let source = fs::read_to_string(path)?;
        let ast = parser::parse(&source).map_err(|e| anyhow::anyhow!("Parse Error: {}", e))?;
        let mut chunk = matchbox_compiler::compile_with_treeshaking(path.to_str().unwrap_or("unknown"), &ast, &source, keep_symbols, no_shaking, no_std_lib)
            .map_err(|e| anyhow::anyhow!("Compiler Error: {}", e))?;

        if strip_source {
            strip_sources(&mut chunk);
        }

        if is_build {
            let bytes = bincode::serialize(&chunk)?;
            let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| path.with_extension("bxb"));
            fs::write(&out_path, bytes)?;
            println!("Compiled to {}", out_path.display());
        } else if let Some(t) = target {
            let project_root = path.parent().unwrap_or(Path::new("."));
            let native_dir = project_root.join("native");
            
            if native_dir.exists() && native_dir.is_dir() {
                return produce_fusion_artifact(&chunk, path, &native_dir, t, &ast, output);
            }

            match t {
                "wasi" => produce_wasi_binary(&chunk, path, output)?,
                "wasm" => produce_wasm_binary(&chunk, path, output)?,
                "js" => produce_js_bundle(&chunk, path, &ast, output)?,
                target => produce_native_binary(&chunk, path, target, output)?,
            }
        } else {
            run_chunk(chunk)?;
        }
    }
    Ok(())
}

fn produce_js_bundle(chunk: &Chunk, source_path: &Path, ast: &[ast::Statement], output: Option<&Path>) -> Result<()> {
    let bytecode = bincode::serialize(chunk)?;
    let b64_bytecode = base64_simd::STANDARD.encode_to_string(&bytecode);
    
    let wasm_bytes = stubs::get_stub("wasi").unwrap_or(&[]).to_vec();
    if wasm_bytes.is_empty() {
        bail!("WASI runner stub is empty. The matchbox CLI must be rebuilt with the wasm32-wasip1 target installed.");
    }
    let b64_wasm = base64_simd::STANDARD.encode_to_string(&wasm_bytes);

    let mut functions = Vec::new();
    for stmt in ast {
        if let ast::StatementKind::FunctionDecl { name, .. } = &stmt.kind {
            functions.push(name.clone());
        }
    }

    let mut bootstrap = String::new();
    bootstrap.push_str(&format!("const wasmBase64 = \"{}\";\n", b64_wasm));
    bootstrap.push_str(&format!("const bytecodeBase64 = \"{}\";\n\n", b64_bytecode));
    
    bootstrap.push_str("let vm = null;\n");
    bootstrap.push_str("async function ensureInit() {\n");
    bootstrap.push_str("    if (vm) return;\n");
    bootstrap.push_str("    const wasmBinary = Uint8Array.from(atob(wasmBase64), c => c.charCodeAt(0));\n");
    bootstrap.push_str("    await init(wasmBinary);\n");
    bootstrap.push_str("    vm = new BoxLangVM();\n");
    bootstrap.push_str("    const bytecodeBinary = Uint8Array.from(atob(bytecodeBase64), c => c.charCodeAt(0));\n");
    bootstrap.push_str("    vm.load_bytecode(bytecodeBinary);\n");
    bootstrap.push_str("}\n\n");

    for func in functions {
        bootstrap.push_str(&format!("export async function {}(...args) {{\n", func));
        bootstrap.push_str("    await ensureInit();\n");
        bootstrap.push_str(&format!("    return vm.call(\"{}\", args);\n", func));
        bootstrap.push_str("}\n\n");
    }

    let final_js = JS_GLUE_TEMPLATE.replace("/* __REPLACE_ME__ */", &bootstrap);

    let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| source_path.with_extension("js"));
    fs::write(&out_path, final_js)?;
    println!("Standalone JS module produced: {}", out_path.display());
    Ok(())
}

fn process_directory(path: &Path, is_build: bool, target: Option<&str>, keep_symbols: Vec<String>, no_shaking: bool, no_std_lib: bool, strip_source: bool, output: Option<&Path>) -> Result<()> {
    let entry_points = ["index.bxs", "main.bxs", "Application.bx"];
    let mut entry_file = None;
    for ep in entry_points {
        let p = path.join(ep);
        if p.exists() {
            entry_file = Some(p);
            break;
        }
    }
    let entry_file = entry_file.context("No entry point found in directory")?;
    process_file(&entry_file, is_build, target, keep_symbols, no_shaking, no_std_lib, strip_source, output)
}

/// Recursively clear embedded source text from a chunk tree.
/// `filename` and per-opcode `lines` are preserved so errors still report `file:line`.
/// Native binaries automatically fall back to reading the source file from disk.
fn strip_sources(chunk: &mut Chunk) {
    chunk.source = String::new();
    for constant in chunk.constants.iter_mut() {
        match constant {
            types::Constant::CompiledFunction(f) => {
                strip_sources(&mut *f.chunk.borrow_mut());
            }
            types::Constant::Class(cls) => {
                strip_sources(&mut *cls.constructor.chunk.borrow_mut());
                for method in cls.methods.values() {
                    strip_sources(&mut *method.chunk.borrow_mut());
                }
            }
            _ => {}
        }
    }
}

pub fn run_chunk(chunk: Chunk) -> Result<()> {
    let mut vm = vm::VM::new();
    vm.interpret(chunk)?;
    Ok(())
}


fn run_repl() -> Result<()> {
    println!("BoxLang REPL (Rust)");
    println!("Type 'exit' or 'quit' to exit.");

    let mut vm = vm::VM::new();
    
    // Load full prelude for REPL
    let prelude_ast = parser::parse(matchbox_compiler::PRELUDE_SOURCE)?;
    let compiler = compiler::Compiler::new("prelude");
    let prelude_chunk = compiler.compile(&prelude_ast, matchbox_compiler::PRELUDE_SOURCE)?;
    vm.interpret(prelude_chunk)?;

    loop {
        print!("bx> ");
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            println!();
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        if input == "exit" || input == "quit" {
            break;
        }

        match parser::parse(input) {
            Ok(ast) => {
                let mut compiler = compiler::Compiler::new("repl");
                compiler.is_repl = true;
                match compiler.compile(&ast, input) {
                    Ok(chunk) => {
                        match vm.interpret(chunk) {
                            Ok(val) => {
                                if val != types::BxValue::new_null() {
                                    println!("=> {}", val);
                                }
                            }
                            Err(e) => println!("Error: {}", e),
                        }
                    }
                    Err(e) => println!("Compiler Error: {}", e),
                }
            }
            Err(e) => println!("Parse Error: {}", e),
        }
    }

    Ok(())
}

fn load_embedded_bytecode() -> Result<Chunk> {
    let self_path = std_env::current_exe().map_err(|_| anyhow::anyhow!("Not an executable"))?;
    let bytes = fs::read(self_path)?;
    if bytes.len() < 16 { bail!("Too small"); }
    let footer_start = bytes.len() - 8;
    if &bytes[footer_start..] != MAGIC_FOOTER { bail!("No footer"); }
    let len_start = bytes.len() - 16;
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&bytes[len_start..footer_start]);
    let len = u64::from_le_bytes(len_bytes) as usize;
    let chunk_start = len_start - len;
    let chunk_bytes = &bytes[chunk_start..len_start];
    let chunk: Chunk = bincode::deserialize(chunk_bytes)?;
    Ok(chunk)
}

#[cfg(target_arch = "wasm32")]
fn load_wasm_custom_section() -> Result<Chunk> {
    bail!("Custom section loading requires host support in this POC")
}

fn produce_native_binary(chunk: &Chunk, source_path: &Path, target: &str, output: Option<&Path>) -> Result<()> {
    let stub_key = if target == "native" { "host" } else { target };
    let native_bytes = stubs::get_stub(stub_key).unwrap_or(&[]);
    if native_bytes.is_empty() {
        bail!("Runner stub for target '{}' is missing. Please ensure the cross-compile feature is enabled and the target is supported.", target);
    }
    let mut binary_bytes = native_bytes.to_vec();
    let chunk_bytes = bincode::serialize(chunk)?;
    let chunk_len = chunk_bytes.len() as u64;
    binary_bytes.extend_from_slice(&chunk_bytes);
    binary_bytes.extend_from_slice(&chunk_len.to_le_bytes());
    binary_bytes.extend_from_slice(MAGIC_FOOTER);
    let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        if cfg!(windows) { source_path.with_extension("exe") } else { source_path.with_extension("") }
    });
    fs::write(&out_path, binary_bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&out_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&out_path, perms)?;
    }
    println!("Native binary produced: {}", out_path.display());
    Ok(())
}

fn produce_fusion_artifact(chunk: &Chunk, source_path: &Path, native_dir: &Path, target: &str, ast: &[ast::Statement], output: Option<&Path>) -> Result<()> {
    println!("Native Fusion detected! Target: {}. Building hybrid artifact...", target);
    
    let build_dir = std_env::current_dir()?.join("target").join("fusion");
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir)?;
    }
    fs::create_dir_all(&build_dir.join("src"))?;

    let is_wasm = target == "wasm" || target == "js";
    let is_wasi = target == "wasi";

    // 1. Generate Cargo.toml
    // Use forward slashes so the path is valid in a TOML string on all platforms
    // (Windows paths with backslashes would require escaping inside TOML strings).
    let vm_path = concat!(env!("CARGO_MANIFEST_DIR"), "/crates/matchbox-vm")
        .replace('\\', "/");
    let mut cargo_toml = format!(r#"[package]
name = "fusion_build"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
matchbox_vm = {{ path = "{}" }}
bincode = "1.3.3"
anyhow = "1.0"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
"#, vm_path);

    if is_wasm {
        cargo_toml.push_str("\n[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n\n");
        cargo_toml.push_str("wasm-bindgen = \"0.2\"\n");
        cargo_toml.push_str("js-sys = \"0.3\"\n");
    }

    fs::write(build_dir.join("Cargo.toml"), cargo_toml)?;

    // 2. Prepare user modules
    let mut mod_decls = String::new();
    let mut registration_calls = String::new();
    
    for entry in fs::read_dir(native_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let mod_name = path.file_stem().unwrap().to_str().unwrap();
            fs::copy(&path, build_dir.join("src").join(format!("{}.rs", mod_name)))?;
            mod_decls.push_str(&format!("mod {};\n", mod_name));
            
            // Check if module has register_bifs or register_classes using simple string searching
            let content = fs::read_to_string(&path)?;
            if content.contains("pub fn register_bifs") {
                registration_calls.push_str(&format!("    for (name, val) in {}::register_bifs() {{ bifs.insert(name, val); }}\n", mod_name));
            }
            if content.contains("pub fn register_classes") {
                registration_calls.push_str(&format!("    for (name, val) in {}::register_classes() {{ classes.insert(name, val); }}\n", mod_name));
            }
        }
    }

    // 3. Generate main.rs / lib.rs
    let bytecode = bincode::serialize(chunk)?;

    let mut code = format!(r#"
    use matchbox_vm::{{vm::VM, types::{{BxValue, BxNativeFunction}}, Chunk}};
    use std::collections::HashMap;

{}

fn run_interpreted() -> anyhow::Result<()> {{
    let mut bifs = HashMap::new();
    let mut classes = HashMap::new();
{}
    
    let bytecode: Vec<u8> = vec!{:?};
    let chunk: Chunk = bincode::deserialize(&bytecode)?;
    
    let mut vm = VM::new_with_bifs(bifs, classes);
    vm.interpret(chunk)?;
    Ok(())
}}
"#, mod_decls, registration_calls, bytecode);

    if is_wasm {
        code.push_str(r#"
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn run() {
    run_interpreted().unwrap();
}

#[wasm_bindgen]
pub struct BoxLangVM {
    vm: VM,
}

#[wasm_bindgen]
impl BoxLangVM {
    #[wasm_bindgen(constructor)]
    pub fn new() -> BoxLangVM {
        let mut bifs = HashMap::new();
        // TODO: In a real implementation, we'd need to re-run registration here
        BoxLangVM { vm: VM::new_with_bifs(bifs, HashMap::new()) }
    }

    pub fn load_bytecode(&mut self, bytes: &[u8]) -> Result<(), String> {
        let chunk: Chunk = bincode::deserialize(bytes).map_err(|e| e.to_string())?;
        self.vm.interpret(chunk).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn call(&mut self, name: &str, args: js_sys::Array) -> Result<JsValue, String> {
        let mut bx_args = Vec::new();
        for i in 0..args.length() {
            bx_args.push(self.vm.js_to_bx(args.get(i)));
        }
        match self.vm.call_function(name, bx_args) {
            Ok(val) => Ok(self.vm.bx_to_js(&val)),
            Err(e) => Err(e.to_string()),
        }
    }
}
"#);
        fs::write(build_dir.join("src").join("lib.rs"), code)?;
    } else {
        code.push_str("fn main() -> anyhow::Result<()> { run_interpreted() }\n");
        fs::write(build_dir.join("src").join("main.rs"), code)?;
    }

    // 4. Build
    let cargo_bin = std_env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = std::process::Command::new(cargo_bin);
    cmd.arg("build").arg("--release").current_dir(&build_dir);
    
    if is_wasm {
        cmd.arg("--target").arg("wasm32-unknown-unknown");
    } else if is_wasi {
        cmd.arg("--target").arg("wasm32-wasip1");
    }

    let status = cmd.status()?;
    if !status.success() {
        bail!("Failed to compile native fusion binary");
    }

    // 5. Handle Artifact
    if target == "native" {
        let exe_name = if cfg!(windows) { "fusion_build.exe" } else { "fusion_build" };
        let artifact = build_dir.join("target").join("release").join(exe_name);
        let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            if cfg!(windows) { source_path.with_extension("exe") } else { source_path.with_extension("") }
        });
        fs::copy(artifact, &out_path)?;
        println!("Native Fusion binary produced: {}", out_path.display());
    } else if target == "wasi" {
        let artifact = build_dir.join("target").join("wasm32-wasip1").join("release").join("fusion_build.wasm");
        let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| source_path.with_extension("wasm"));
        fs::copy(artifact, &out_path)?;
        println!("WASI Fusion binary produced: {}", out_path.display());
    } else if target == "wasm" {
        let artifact = build_dir.join("target").join("wasm32-unknown-unknown").join("release").join("fusion_build.wasm");
        let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| source_path.with_extension("wasm"));
        fs::copy(artifact, &out_path)?;
        println!("WASM Fusion binary produced: {}", out_path.display());
    } else if target == "js" {
        // For JS, we would normally run wasm-bindgen here. 
        // For this POC, we'll produce the wasm and a notice.
        let artifact = build_dir.join("target").join("wasm32-unknown-unknown").join("release").join("fusion_build.wasm");
        let wasm_out = source_path.with_extension("wasm");
        fs::copy(artifact, &wasm_out)?;
        
        // Re-use standard JS bundle logic but pointed to this new wasm
        produce_js_bundle(chunk, source_path, ast, output)?;
        println!("JS Fusion module produced. NOTE: Requires wasm-bindgen on the fusion artifact for full functionality.");
    }

    Ok(())
}

fn produce_wasi_binary(chunk: &Chunk, source_path: &Path, output: Option<&Path>) -> Result<()> {
    let mut wasm_bytes = stubs::get_stub("wasi").unwrap_or(&[]).to_vec();
    if wasm_bytes.is_empty() {
        bail!("WASI runner stub is empty. The matchbox CLI must be rebuilt with the wasm32-wasip1 target installed.");
    }

    let chunk_bytes = bincode::serialize(chunk)?;

    // The runner stub contains a 1 MB sentinel region whose layout is:
    //   [0..8]  = magic "BXLG\x00\x00\x00\x01"
    //   [8..12] = bytecode length (u32 LE)
    //   [12..]  = bytecode data
    // We locate it by magic and patch in-place — no cargo invocation needed.
    const SENTINEL: [u8; 8] = [b'B', b'X', b'L', b'G', 0, 0, 0, 1];
    const EMBED_CAPACITY: usize = 1024 * 1024;
    const EMBED_DATA_CAPACITY: usize = EMBED_CAPACITY - 12;

    let sentinel_pos = wasm_bytes
        .windows(SENTINEL.len())
        .position(|w| w == SENTINEL)
        .context("WASI stub is missing the BOXLANG_EMBED sentinel. \
                  Rebuild the runner stub: \
                  cargo build --release --target wasm32-wasip1 -p matchbox_runner")?;

    if chunk_bytes.len() > EMBED_DATA_CAPACITY {
        bail!(
            "Bytecode ({} bytes) exceeds the WASI embed capacity ({} bytes). \
             Increase EMBED_CAPACITY in crates/matchbox-runner/src/main.rs and rebuild the stub.",
            chunk_bytes.len(),
            EMBED_DATA_CAPACITY
        );
    }

    let len_offset  = sentinel_pos + 8;
    let data_offset = sentinel_pos + 12;

    let len_bytes = (chunk_bytes.len() as u32).to_le_bytes();
    wasm_bytes[len_offset..len_offset + 4].copy_from_slice(&len_bytes);
    wasm_bytes[data_offset..data_offset + chunk_bytes.len()].copy_from_slice(&chunk_bytes);

    let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| source_path.with_extension("wasm"));
    fs::write(&out_path, wasm_bytes)?;

    println!("WASI container binary produced: {}", out_path.display());
    Ok(())
}

fn produce_wasm_binary(chunk: &Chunk, source_path: &Path, output: Option<&Path>) -> Result<()> {
    let wasm_bytes = stubs::get_stub("wasi").unwrap_or(&[]).to_vec();
    if wasm_bytes.is_empty() {
        bail!("WASI runner stub is empty. The matchbox CLI must be rebuilt with the wasm32-wasip1 target installed.");
    }
    let chunk_bytes = bincode::serialize(chunk)?;
    
    let mut out_bytes = wasm_bytes;
    use wasm_encoder::Encode;
    let custom_section = wasm_encoder::CustomSection {
        name: "boxlang_bytecode".into(),
        data: (&chunk_bytes).into(),
    };
    let mut section_bytes = Vec::new();
    custom_section.encode(&mut section_bytes);
    out_bytes.extend_from_slice(&section_bytes);
    
    let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| source_path.with_extension("wasm"));
    fs::write(&out_path, out_bytes)?;
    
    println!("WASM binary produced: {}", out_path.display());
    Ok(())
}
