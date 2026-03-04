mod ast;
mod bifs;
mod parser;
mod types;
mod vm;
mod compiler;

use std::env as std_env;
use std::fs;
use std::path::Path;
use anyhow::{Result, bail, Context};
use crate::vm::chunk::Chunk;

const MAGIC_FOOTER: &[u8; 8] = b"BOXLANG\x01";

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
    if args.len() < 2 {
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
        .find(|a| !a.starts_with("--") && *a != "native" && *a != "wasm")
        .context("No filename or directory provided")?;
    let path = Path::new(filename);

    if path.is_dir() {
        process_directory(path, is_build, target)?;
    } else {
        process_file(path, is_build, target)?;
    }

    Ok(())
}

fn print_usage() {
    println!("Usage: bx-rust [options] <file.bxs|file.bxb|directory>");
    println!("\nOptions:");
    println!("  --build             Compile to bytecode (.bxb)");
    println!("  --target <native>   Produce a standalone native binary");
    println!("  --target <wasm>     Produce a standalone WASM binary");
}

fn process_file(path: &Path, is_build: bool, target: Option<&str>) -> Result<()> {
    if path.extension().and_then(|s| s.to_str()) == Some("bxb") {
        let bytes = fs::read(path)?;
        let chunk: Chunk = bincode::deserialize(&bytes)?;
        run_chunk(chunk)?;
    } else {
        let source = fs::read_to_string(path)?;
        let ast = parser::parse(&source).map_err(|e| anyhow::anyhow!("Parse Error: {}", e))?;
        let compiler = compiler::Compiler::new();
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
    vm.interpret(chunk).map_err(|e| anyhow::anyhow!("VM Runtime Error: {}", e))?;
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
    // In WASI, we usually need the module bytes. 
    // This is hard to get without external help. 
    // For this POC, we assume the host provides the bytecode in a specific way or we bail.
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
    let wasm_runner_path = Path::new("target/wasm32-wasip1/debug/bx-rust.wasm");
    if !wasm_runner_path.exists() {
        bail!("WASM runner not found. Please run 'cargo build --target wasm32-wasip1' first.");
    }
    
    let wasm_bytes = fs::read(wasm_runner_path)?;
    let chunk_bytes = bincode::serialize(chunk)?;
    
    // Use wasm-encoder to add a custom section
    let mut module = wasm_encoder::Module::new();
    
    // Copy existing sections
    use wasmparser::Parser;
    for payload in Parser::new(0).parse_all(&wasm_bytes) {
        let payload = payload?;
        match payload {
            wasmparser::Payload::CustomSection(c) => {
                module.section(&wasm_encoder::CustomSection {
                    name: c.name().into(),
                    data: c.data().into(),
                });
            }
            // For a robust implementation, we'd copy ALL sections (Type, Import, Function, etc.)
            // But wasm-encoder doesn't have a "raw section" copy for all types easily.
            // Simplified: We'll just append our custom section to the end of the file.
            // WASM format actually allows this!
            _ => {}
        }
    }
    
    // Actually, appending a custom section to the end of a valid WASM file IS valid.
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
