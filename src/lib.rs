pub mod ast;
pub mod bifs;
pub mod parser;
pub mod types;
pub mod vm;
pub mod compiler;

use std::env as std_env;
use std::fs;
use std::path::Path;
use std::io::{self, Write};
use anyhow::{Result, bail, Context};
pub use crate::vm::chunk::Chunk;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

const MAGIC_FOOTER: &[u8; 8] = b"BOXLANG\x01";

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

        let name_lower = name.to_lowercase();
        let func = self.vm.globals.get(&name_lower).cloned()
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

    let is_build = args.contains(&"--build".to_string());
    let target = if let Some(idx) = args.iter().position(|a| a == "--target") {
        args.get(idx + 1).map(|s| s.as_str())
    } else {
        None
    };

    let filename = args.iter().skip(1)
        .find(|a| !a.starts_with("--") && *a != "native" && *a != "wasm" && *a != "js");

    match filename {
        Some(name) => {
            let path = Path::new(name);
            if path.is_dir() {
                process_directory(path, is_build, target)?;
            } else {
                process_file(path, is_build, target)?;
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
    println!("  --build             Compile to bytecode (.bxb)");
    println!("  --target <native>   Produce a standalone native binary");
    println!("  --target <wasm>     Produce a standalone WASM binary");
    println!("  --target <js>       Produce a JavaScript module wrapper");
    println!("\nIf no file is provided, matchbox starts in REPL mode.");
}

pub fn process_file(path: &Path, is_build: bool, target: Option<&str>) -> Result<()> {
    if path.extension().and_then(|s| s.to_str()) == Some("bxb") {
        let bytes = fs::read(path)?;
        let chunk: Chunk = bincode::deserialize(&bytes)?;
        run_chunk(chunk)?;
    } else {
        let source = fs::read_to_string(path)?;
        let ast = parser::parse(&source).map_err(|e| anyhow::anyhow!("Parse Error: {}", e))?;
        let compiler = compiler::Compiler::new(path.to_str().unwrap_or("unknown"));
        let chunk = compiler.compile(&ast, &source).map_err(|e| anyhow::anyhow!("Compiler Error: {}", e))?;

        if is_build {
            let bytes = bincode::serialize(&chunk)?;
            let out_path = path.with_extension("bxb");
            fs::write(&out_path, bytes)?;
            println!("Compiled to {}", out_path.display());
        } else if let Some(t) = target {
            let project_root = path.parent().unwrap_or(Path::new("."));
            let native_dir = project_root.join("native");
            
            if native_dir.exists() && native_dir.is_dir() {
                return produce_fusion_artifact(&chunk, path, &native_dir, t, &ast);
            }

            match t {
                "native" => produce_native_binary(&chunk, path)?,
                "wasm" => produce_wasm_binary(&chunk, path)?,
                "js" => produce_js_bundle(&chunk, path, &ast)?,
                _ => bail!("Unknown target: {}", t),
            }
        } else {
            run_chunk(chunk)?;
        }
    }
    Ok(())
}

fn produce_js_bundle(chunk: &Chunk, source_path: &Path, ast: &[ast::Statement]) -> Result<()> {
    let bytecode = bincode::serialize(chunk)?;
    let b64_bytecode = base64_simd::STANDARD.encode_to_string(&bytecode);
    
    let mut functions = Vec::new();
    for stmt in ast {
        if let ast::StatementKind::FunctionDecl { name, .. } = &stmt.kind {
            functions.push(name.clone());
        }
    }

    let mut js = String::new();
    js.push_str("// Generated by MatchBox\n");
    js.push_str("import init, { BoxLangVM } from './matchbox.js';\n\n");
    js.push_str("let vm = null;\n");
    js.push_str(&format!("const bytecode = \"{}\";\n\n", b64_bytecode));
    
    js.push_str("async function ensureInit() {\n");
    js.push_str("    if (vm) return;\n");
    js.push_str("    await init();\n");
    js.push_str("    vm = new BoxLangVM();\n");
    js.push_str("    const binary = Uint8Array.from(atob(bytecode), c => c.charCodeAt(0));\n");
    js.push_str("    vm.load_bytecode(binary);\n");
    js.push_str("}\n\n");

    for func in functions {
        js.push_str(&format!("export async function {}(...args) {{\n", func));
        js.push_str("    await ensureInit();\n");
        js.push_str(&format!("    return vm.call(\"{}\", args);\n", func));
        js.push_str("}\n\n");
    }

    let out_path = source_path.with_extension("js");
    fs::write(&out_path, js)?;
    println!("JS module produced: {}", out_path.display());
    Ok(())
}

fn process_directory(path: &Path, is_build: bool, target: Option<&str>) -> Result<()> {
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
    process_file(&entry_file, is_build, target)
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
                                if val != types::BxValue::Null {
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

fn produce_native_binary(chunk: &Chunk, source_path: &Path) -> Result<()> {
    let self_path = std_env::current_exe()?;
    let mut binary_bytes = fs::read(self_path)?;
    if binary_bytes.len() > 16 {
        let footer_start = binary_bytes.len() - 8;
        if &binary_bytes[footer_start..] == MAGIC_FOOTER {
            let len_start = binary_bytes.len() - 16;
            let mut len_bytes = [0u8; 8];
            len_bytes.copy_from_slice(&binary_bytes[len_start..footer_start]);
            let len = u64::from_le_bytes(len_bytes) as usize;
            binary_bytes.truncate(len_start - len);
        }
    }
    let chunk_bytes = bincode::serialize(chunk)?;
    let chunk_len = chunk_bytes.len() as u64;
    binary_bytes.extend_from_slice(&chunk_bytes);
    binary_bytes.extend_from_slice(&chunk_len.to_le_bytes());
    binary_bytes.extend_from_slice(MAGIC_FOOTER);
    let out_path = source_path.with_extension("");
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

fn produce_fusion_artifact(chunk: &Chunk, source_path: &Path, native_dir: &Path, target: &str, ast: &[ast::Statement]) -> Result<()> {
    println!("Native Fusion detected! Target: {}. Building hybrid artifact...", target);
    
    let build_dir = std_env::current_dir()?.join("target").join("fusion");
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir)?;
    }
    fs::create_dir_all(&build_dir.join("src"))?;

    let is_wasm = target == "wasm" || target == "js";

    // 1. Generate Cargo.toml
    let mut cargo_toml = format!(r#"[package]
name = "fusion_build"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
matchbox = {{ path = "{}" }}
bincode = "1.3.3"
anyhow = "1.0"
"#, std_env::current_dir()?.display());

    if is_wasm {
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
            registration_calls.push_str(&format!("    for (name, val) in {}::register_bifs() {{ bifs.insert(name, val); }}\n", mod_name));
        }
    }

    // 3. Generate main.rs / lib.rs
    let bytecode = bincode::serialize(chunk)?;
    let bytecode_array = format!("{:?}", bytecode);

    let mut code = format!(r#"
use matchbox::{{vm::VM, types::BxValue, Chunk}};
use std::collections::HashMap;

{}

fn run_interpreted() -> anyhow::Result<()> {{
    let mut bifs = HashMap::new();
{}
    
    let bytecode: Vec<u8> = {};
    let chunk: Chunk = bincode::deserialize(&bytecode)?;
    
    let mut vm = VM::new_with_bifs(bifs);
    vm.interpret(chunk)?;
    Ok(())
}}
"#, mod_decls, registration_calls, bytecode_array);

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
        BoxLangVM { vm: VM::new_with_bifs(bifs) }
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
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("build").arg("--release").current_dir(&build_dir);
    
    if is_wasm {
        cmd.arg("--target").arg("wasm32-unknown-unknown");
    }

    let status = cmd.status()?;
    if !status.success() {
        bail!("Failed to compile native fusion binary");
    }

    // 5. Handle Artifact
    if target == "native" {
        let exe_name = if cfg!(windows) { "fusion_build.exe" } else { "fusion_build" };
        let artifact = build_dir.join("target").join("release").join(exe_name);
        let out_path = source_path.with_extension("");
        fs::copy(artifact, &out_path)?;
        println!("Native Fusion binary produced: {}", out_path.display());
    } else if target == "wasm" {
        let artifact = build_dir.join("target").join("wasm32-unknown-unknown").join("release").join("fusion_build.wasm");
        let out_path = source_path.with_extension("wasm");
        fs::copy(artifact, &out_path)?;
        println!("WASM Fusion binary produced: {}", out_path.display());
    } else if target == "js" {
        // For JS, we would normally run wasm-bindgen here. 
        // For this POC, we'll produce the wasm and a notice.
        let artifact = build_dir.join("target").join("wasm32-unknown-unknown").join("release").join("fusion_build.wasm");
        let wasm_out = source_path.with_extension("wasm");
        fs::copy(artifact, &wasm_out)?;
        
        // Re-use standard JS bundle logic but pointed to this new wasm
        produce_js_bundle(chunk, source_path, ast)?;
        println!("JS Fusion module produced. NOTE: Requires wasm-bindgen on the fusion artifact for full functionality.");
    }

    Ok(())
}

fn produce_wasm_binary(chunk: &Chunk, source_path: &Path) -> Result<()> {
    let mut wasm_runner_path = Path::new("target/wasm32-unknown-unknown/release/matchbox.wasm");
    if !wasm_runner_path.exists() {
        wasm_runner_path = Path::new("target/wasm32-wasip1/debug/matchbox.wasm");
    }
    
    if !wasm_runner_path.exists() {
        bail!("WASM runner not found. Please run 'cargo build --target wasm32-unknown-unknown --release' first.");
    }
    
    let wasm_bytes = fs::read(wasm_runner_path)?;
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
    
    let out_path = source_path.with_extension("wasm");
    fs::write(&out_path, out_bytes)?;
    
    println!("WASM binary produced: {}", out_path.display());
    Ok(())
}
