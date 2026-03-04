mod ast;
mod bifs;
mod parser;
mod types;
mod vm;
mod compiler;

use std::env as std_env;
use std::fs;
use std::path::Path;
use std::io::{self, Write};
use anyhow::{Result, bail, Context};
use crate::vm::chunk::Chunk;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

const MAGIC_FOOTER: &[u8; 8] = b"BOXLANG\x01";

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_boxlang_bytecode(bytes: &[u8]) -> String {
    let res = (|| -> Result<String> {
        let chunk: Chunk = bincode::deserialize(bytes)?;
        let mut vm = vm::VM::new();
        for (name, val) in bifs::register_all() {
            vm.globals.insert(name, val);
        }
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
        let chunk = compiler.compile(&ast)?;
        let mut vm = vm::VM::new();
        for (name, val) in bifs::register_all() {
            vm.globals.insert(name, val);
        }
        let val = vm.interpret(chunk)?;
        Ok(val.to_string())
    })();

    match res {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

fn main() -> Result<()> {
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
        .find(|a| !a.starts_with("--") && *a != "native" && *a != "wasm");

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
    println!("Usage: bx-rust [options] [file.bxs|file.bxb|directory]");
    println!("\nOptions:");
    println!("  -h, --help          Show this help message");
    println!("  --build             Compile to bytecode (.bxb)");
    println!("  --target <native>   Produce a standalone native binary");
    println!("  --target <wasm>     Produce a standalone WASM binary");
    println!("\nIf no file is provided, bx-rust starts in REPL mode.");
}

fn process_file(path: &Path, is_build: bool, target: Option<&str>) -> Result<()> {
    if path.extension().and_then(|s| s.to_str()) == Some("bxb") {
        let bytes = fs::read(path)?;
        let chunk: Chunk = bincode::deserialize(&bytes)?;
        run_chunk(chunk)?;
    } else {
        let source = fs::read_to_string(path)?;
        let ast = parser::parse(&source).map_err(|e| anyhow::anyhow!("Parse Error: {}", e))?;
        let compiler = compiler::Compiler::new(path.to_str().unwrap_or("unknown"));
        let chunk = compiler.compile(&ast).map_err(|e| anyhow::anyhow!("Compiler Error: {}", e))?;

        if is_build {
            let bytes = bincode::serialize(&chunk)?;
            let out_path = path.with_extension("bxb");
            fs::write(&out_path, bytes)?;
            println!("Compiled to {}", out_path.display());
        } else if let Some(t) = target {
            match t {
                "native" => produce_native_binary(&chunk, path)?,
                "wasm" => produce_wasm_binary(&chunk, path)?,
                _ => bail!("Unknown target: {}", t),
            }
        } else {
            run_chunk(chunk)?;
        }
    }
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

fn run_chunk(chunk: Chunk) -> Result<()> {
    let mut vm = vm::VM::new();
    for (name, val) in bifs::register_all() {
        vm.globals.insert(name, val);
    }
    vm.interpret(chunk)?;
    Ok(())
}

fn run_repl() -> Result<()> {
    println!("BoxLang REPL (Rust)");
    println!("Type 'exit' or 'quit' to exit.");

    let mut vm = vm::VM::new();
    for (name, val) in bifs::register_all() {
        vm.globals.insert(name, val);
    }

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
                match compiler.compile(&ast) {
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

fn produce_wasm_binary(chunk: &Chunk, source_path: &Path) -> Result<()> {
    let mut wasm_runner_path = Path::new("target/wasm32-unknown-unknown/release/bx_rust.wasm");
    if !wasm_runner_path.exists() {
        wasm_runner_path = Path::new("target/wasm32-wasip1/debug/bx-rust.wasm");
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
