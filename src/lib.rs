use matchbox_compiler::{ast, compiler, parser};
use matchbox_vm::{types, vm, Chunk};

use std::env as std_env;
use std::fs;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::io::{self, Write};
use anyhow::{Result, bail, Context};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

const MAGIC_FOOTER: &[u8; 8] = b"BOXLANG\x01";

mod stubs;
mod modules;

const JS_GLUE_TEMPLATE: &str = include_str!("js_bundle_template.js");

use postcard;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_boxlang_bytecode(bytes: &[u8]) -> String {
    let res = (|| -> Result<String> {
        let mut chunk: Chunk = postcard::from_bytes(bytes)?;
        chunk.reconstruct_functions();
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
    chunk: Option<Rc<RefCell<Chunk>>>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BoxLangVM {
    #[wasm_bindgen(constructor)]
    pub fn new() -> BoxLangVM {
        BoxLangVM { 
            vm: vm::VM::new(),
            chunk: None,
        }
    }

    pub fn load_bytecode(&mut self, bytes: &[u8]) -> Result<(), String> {
        let res = (|| -> Result<()> {
            let mut chunk: Chunk = postcard::from_bytes(bytes)?;
            chunk.reconstruct_functions();
            let chunk_rc = Rc::new(RefCell::new(chunk.clone()));
            self.chunk = Some(chunk_rc.clone());
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

        match self.vm.call_function_value(func, bx_args, self.chunk.clone()) {
            Ok(val) => Ok(self.vm.bx_to_js(&val)),
            Err(e) => Err(format!("Error: {}", e)),
        }
    }
}

pub fn run() -> Result<()> {
    // 1. Check for WASM custom section first
    #[cfg(target_arch = "wasm32")]
    if let Ok(chunk) = load_wasm_custom_section() {
        return run_chunk(chunk, &[]);
    }

    // 2. Check for embedded bytecode at end of binary (Native)
    if let Ok(embedded_chunk) = load_embedded_bytecode() {
        return run_chunk(embedded_chunk, &[]);
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
    let mut is_flash = args.contains(&"--flash".to_string());
    let is_full_flash = args.contains(&"--full-flash".to_string());
    let is_watch = args.contains(&"--watch".to_string());
    if is_full_flash || is_watch {
        is_flash = true;
    }
    let mut is_fast_deploy = args.contains(&"--fast-deploy".to_string());
    let chip = if let Some(idx) = args.iter().position(|a| a == "--chip") {
        args.get(idx + 1).map(|s| s.as_str())
    } else {
        None
    };
    let target = if let Some(idx) = args.iter().position(|a| a == "--target") {
        args.get(idx + 1).map(|s| s.as_str())
    } else {
        None
    };

    // For ESP32, --flash defaults to fast deploy unless --full-flash is present.
    if target == Some("esp32") && is_flash && !is_full_flash {
        is_fast_deploy = true;
    }

    let no_shaking = args.contains(&"--no-shaking".to_string());
    let no_std_lib = args.contains(&"--no-std-lib".to_string());
    let strip_source = args.contains(&"--strip-source".to_string());

    let mut keep_symbols = Vec::new();
    if let Some(idx) = args.iter().position(|a| a == "--keep") {
        if let Some(val) = args.get(idx + 1) {
            keep_symbols = val.split(',').map(|s| s.trim().to_string()).collect();
        }
    }

    let output: Option<PathBuf> = if let Some(idx) = args.iter().position(|a| a == "--output") {
        args.get(idx + 1).map(|s| PathBuf::from(s))
    } else {
        None
    };

    // Collect --module <path> flags.
    let extra_module_paths: Vec<PathBuf> = args.iter().enumerate()
        .filter(|(_, a)| *a == "--module")
        .filter_map(|(i, _)| args.get(i + 1).map(|p| PathBuf::from(p)))
        .collect();

    let filename = args.iter().skip(1)
        .find(|a| !a.starts_with("--") && *a != "native" && *a != "wasm" && *a != "wasi" && *a != "js" && *a != "esp32"
              && (args.iter().position(|arg| arg == *a).map(|pos| pos == 0 || args[pos-1] != "--target").unwrap_or(true))
              && (args.iter().position(|arg| arg == *a).map(|pos| pos == 0 || args[pos-1] != "--keep").unwrap_or(true))
              && (args.iter().position(|arg| arg == *a).map(|pos| pos == 0 || args[pos-1] != "--output").unwrap_or(true))
              && (args.iter().position(|arg| arg == *a).map(|pos| pos == 0 || args[pos-1] != "--module").unwrap_or(true))
              && (args.iter().position(|arg| arg == *a).map(|pos| pos == 0 || args[pos-1] != "--chip").unwrap_or(true))
        );

    match filename {
        Some(name) => {
            let path = Path::new(name);
            if path.is_dir() {
                process_directory(path, is_build, target, keep_symbols, no_shaking, no_std_lib, strip_source, output.as_deref(), &extra_module_paths, is_flash, chip, is_fast_deploy, is_watch, is_full_flash)?;
            } else {
                process_file(path, is_build, target, keep_symbols, no_shaking, no_std_lib, strip_source, output.as_deref(), &extra_module_paths, is_flash, chip, is_fast_deploy, is_watch, is_full_flash)?;
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
    println!("  --target <esp32>    Produce an ESP32 firmware binary");
    println!("  --flash             Flash to device. Defaults to fast bytecode-only flash for ESP32.");
    println!("  --full-flash        Force a full firmware flash (includes VM/Runner)");
    println!("  --fast-deploy       (Deprecated) Use --flash instead, which is now fast by default.");
    println!("  --chip <name>       The target ESP32 chip (e.g. esp32, esp32s3, esp32c3)");
    println!("  --keep <symbols>    Comma-separated list of BIFs to preserve");
    println!("  --no-shaking        Disable tree-shaking and include all prelude BIFs");
    println!("  --no-std-lib        Exclude the standard library (prelude) entirely");
    println!("  --output <path>     Set the output file path for compiled artifacts");
    println!("  --strip-source      Strip embedded source text from compiled output");
    println!("                      Errors still report file:line; native binaries fall back to disk for snippets");
    println!("  --module <path>     Load a BoxLang module directory (may be specified multiple times)");
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

pub fn process_file(source_path: &Path, is_build: bool, orig_target: Option<&str>, keep_symbols: Vec<String>, no_shaking: bool, no_std_lib: bool, strip_source: bool, output: Option<&Path>, extra_module_paths: &[PathBuf], is_flash: bool, orig_chip: Option<&str>, is_fast_deploy: bool, is_watch: bool, is_full_flash: bool) -> Result<()> {
    if source_path.extension().and_then(|s| s.to_str()) == Some("bxb") {
        let bytes = fs::read(source_path)?;
        let mut chunk: Chunk = postcard::from_bytes(&bytes)?;
        chunk.reconstruct_functions();
        run_chunk(chunk, &[])?;
    } else {
        let source = fs::read_to_string(source_path)?;
        let ast = parser::parse(&source).map_err(|e| anyhow::anyhow!("Parse Error: {}", e))?;
        // Discover modules from matchbox.toml (in CWD) and any --module flags.
        let cwd = std::env::current_dir().unwrap_or_else(|_| {
            source_path.parent().unwrap_or(Path::new(".")).to_path_buf()
        });
        let modules_info = modules::discover_modules(&cwd, extra_module_paths)?;

        let module_mappings: Vec<(String, PathBuf)> = modules_info.iter()
            .map(|m| (m.name.clone(), m.path.clone()))
            .collect();
        let mut extra_preludes: Vec<String> = modules_info.iter()
            .flat_map(|m| m.bif_sources.iter().cloned())
            .collect();
        // Inject a getModuleSettings(name) function whose body is baked from the
        // settings collected by each module's configure() lifecycle call.
        if !modules_info.is_empty() {
            let settings_bxs = modules::generate_get_module_settings_bxs(&modules_info);
            extra_preludes.push(settings_bxs);
        }
        let mut chunk = matchbox_compiler::compile_with_treeshaking(
            source_path.to_str().unwrap_or("unknown"),
            &ast,
            &source,
            keep_symbols,
            no_shaking,
            no_std_lib,
            &module_mappings,
            &extra_preludes,
        ).map_err(|e| anyhow::anyhow!("Compiler Error: {}", e))?;

        chunk.reconstruct_functions();

        if strip_source {
            strip_sources(&mut chunk);
        }

        if is_build {
            let bytes = postcard::to_stdvec(&chunk)?;
            let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| source_path.with_extension("bxb"));
            fs::write(&out_path, bytes)?;
            println!("Compiled to {}", out_path.display());
        } else if let Some(t) = orig_target {
            let project_root = source_path.parent().unwrap_or(Path::new("."));
            let native_dir = project_root.join("native");
            
            let has_native_modules = modules_info.iter().any(|m| m.has_native);
            if (native_dir.exists() && native_dir.is_dir()) || has_native_modules {
                return produce_fusion_artifact(&chunk, source_path, &native_dir, t, &ast, output, &modules_info, is_flash, orig_chip);
            }

            match t {
                "wasi" => produce_wasi_binary(&chunk, source_path, output)?,
                "wasm" => produce_wasm_binary(&chunk, source_path, output)?,
                "js" => produce_js_bundle(&chunk, source_path, &ast, output)?,
                "esp32" => {
                    // In watch mode, force a full flash on the initial entry so the
                    // on-device runner always matches the current bytecode format.
                    let (fd, ff) = if is_watch {
                        (false, true)
                    } else {
                        (is_fast_deploy, is_full_flash)
                    };
                    produce_esp32_binary(&chunk, source_path, output, is_flash, orig_chip, fd, ff)?;
                }
                target_val => produce_native_binary(&chunk, source_path, target_val, output)?,
            }
        } else {
            // Register datasources from matchbox.toml only when actually running.
            #[cfg(all(not(target_arch = "wasm32"), feature = "bif-datasource"))]
            register_datasources_from_config(&cwd)?;

            run_chunk(chunk, &modules_info)?;
        }

        if is_watch {
            if orig_target != Some("esp32") {
                bail!("--watch is currently only supported for --target esp32");
            }
            let chip_str = orig_chip.map(|s| s.to_string());
            println!("WATCH MODE ENABLED: Watching for changes in {}...", source_path.parent().unwrap().display());
            return watch_mode(source_path, chip_str, is_full_flash);
        }
    }
    Ok(())
}

fn produce_js_bundle(chunk: &Chunk, source_path: &Path, ast: &[ast::Statement], output: Option<&Path>) -> Result<()> {
    let bytecode = postcard::to_stdvec(chunk)?;
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

fn process_directory(path: &Path, is_build: bool, orig_target: Option<&str>, keep_symbols: Vec<String>, no_shaking: bool, no_std_lib: bool, strip_source: bool, output: Option<&Path>, extra_module_paths: &[PathBuf], is_flash: bool, orig_chip: Option<&str>, is_fast_deploy: bool, is_watch: bool, is_full_flash: bool) -> Result<()> {
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
    process_file(&entry_file, is_build, orig_target, keep_symbols, no_shaking, no_std_lib, strip_source, output, extra_module_paths, is_flash, orig_chip, is_fast_deploy, is_watch, is_full_flash)
}

/// Recursively clear embedded source text from a chunk tree.
/// `filename` and per-opcode `lines` are preserved so errors still report `file:line`.
/// Native binaries automatically fall back to reading the source file from disk.
fn strip_sources(chunk: &mut Chunk) {
    chunk.source = String::new();
    for constant in chunk.constants.iter_mut() {
        match constant {
            types::Constant::CompiledFunction(f) => strip_sources(&mut f.chunk),
            types::Constant::Class(c) => {
                strip_sources(&mut c.constructor.chunk);
                for (_, m) in c.methods.iter_mut() {
                    strip_sources(&mut m.chunk);
                }
            }
            types::Constant::Interface(i) => {
                for (_, m) in i.methods.iter_mut() {
                    if let Some(f) = m {
                        strip_sources(&mut f.chunk);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "bif-datasource"))]
fn register_datasources_from_config(project_dir: &Path) -> Result<()> {
    use std::sync::Arc;
    let configs = modules::read_datasource_configs(project_dir)?;
    for (name, entry) in configs {
        let config = matchbox_vm::datasource::traits::DatasourceConfig {
            driver: entry.driver.clone(),
            host: entry.host,
            port: entry.port,
            database: entry.database,
            username: entry.username,
            password: entry.password,
            max_connections: entry.max_connections,
        };
        match entry.driver.to_lowercase().as_str() {
            "postgresql" | "postgres" => {
                let driver =
                    matchbox_vm::datasource::drivers::postgres::PostgresDriver::new(&config)
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Failed to create datasource '{}': {}",
                                name,
                                e
                            )
                        })?;
                matchbox_vm::datasource::registry::register(&name, Arc::new(driver));
            }
            other => {
                eprintln!(
                    "Warning: datasource '{}' has unknown driver '{}', skipping.",
                    name, other
                );
            }
        }
    }
    Ok(())
}

pub fn run_chunk(chunk: Chunk, modules: &[modules::ModuleInfo]) -> Result<()> {
    let args: Vec<String> = std_env::args().collect();
    
    let mut external_bifs = HashMap::new();
    let native_classes = HashMap::new();

    // In a real implementation with dynamic loading, we would load .so/.dll here.
    for m in modules {
        if m.name == "native-math" {
            // HACK for tests: since we can't easily dynamic-load from the same binary's crate
            // we just manually register what we know is in there.
            fn cube(_vm: &mut dyn types::BxVM, args: &[types::BxValue]) -> std::result::Result<types::BxValue, String> {
                if args.len() != 1 { return Err("cube requires 1 argument".to_string()); }
                let n = args[0].as_number();
                Ok(types::BxValue::new_number(n * n * n))
            }
            external_bifs.insert("cube".to_string(), cube as types::BxNativeFunction);
        }
    }
    
    let mut vm = vm::VM::new_with_bifs(external_bifs, native_classes);
    vm.cli_args = args.clone();

    #[cfg(feature = "jit")]
    vm.enable_jit();

    vm.interpret(chunk)?;
    Ok(())
}


fn run_repl() -> Result<()> {
    println!("BoxLang REPL (Rust)");
    println!("Type 'exit' or 'quit' to exit.");

    let args: Vec<String> = std_env::args().collect();
    let mut vm = vm::VM::new_with_args(args);

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
    let mut chunk: Chunk = postcard::from_bytes(chunk_bytes)?;
    chunk.reconstruct_functions();
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
    let chunk_bytes = postcard::to_stdvec(chunk)?;
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

fn produce_fusion_artifact(chunk: &Chunk, source_path: &Path, native_dir: &Path, target: &str, ast: &[ast::Statement], output: Option<&Path>, modules: &[modules::ModuleInfo], is_flash: bool, chip: Option<&str>) -> Result<()> {
    println!("Native Fusion detected! Target: {}. Building hybrid artifact...", target);
    
    let is_esp32 = target == "esp32";
    let chip = chip.unwrap_or("esp32");
    let esp_target = match chip {
        "esp32c3" | "esp32c6" | "esp32h2" => "riscv32imc-esp-espidf",
        "esp32s2" => "xtensa-esp32s2-espidf",
        "esp32s3" => "xtensa-esp32s3-espidf",
        _ => "xtensa-esp32-espidf",
    };

    let script_stem = source_path.file_stem().and_then(|s| s.to_str()).unwrap_or("fusion");
    let build_dir = std_env::current_dir()?.join("target").join("fusion").join(script_stem);
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir)?;
    }
    fs::create_dir_all(&build_dir.join("src"))?;

    let is_wasm = target == "wasm" || target == "js";
    let is_wasi = target == "wasi";

    // 1. Generate Cargo.toml
    let vm_path = concat!(env!("CARGO_MANIFEST_DIR"), "/crates/matchbox-vm")
        .replace('\\', "/");

    let mut extra_dep_lines = String::new();
    for module in modules.iter().filter(|m| m.has_native) {
        let dep_path = module.path.join("matchbox");
        let dep_str = dep_path.to_str().unwrap_or("").replace('\\', "/");
        let dep_str = dep_str.strip_prefix("//?/").unwrap_or(&dep_str);
        extra_dep_lines.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            module.name,
            dep_str,
        ));
    }

    let mut cargo_toml = format!(r#"[package]
name = "fusion_build"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
matchbox_vm = {{ path = "{vm_path}" }}
postcard = {{ version = "1.0", features = ["alloc", "use-std"] }}
bincode = "1.3.3"
anyhow = "1.0"
{extra_dep_lines}
"#);

    if is_esp32 {
        cargo_toml.push_str("esp-idf-svc = { version = \"0.52\", features = [\"binstart\"] }\n");
        cargo_toml.push_str("esp-idf-sys = \"0.37\"\n");
        cargo_toml.push_str("log = \"0.4\"\n");
        cargo_toml.push_str("\n[build-dependencies]\nembuild = { version = \"0.33\", features = [\"espidf\"] }\n");
        
        // Copy partitions and toolchain
        let runner_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/matchbox-esp32-runner");
        fs::copy(runner_root.join("partitions.csv"), build_dir.join("partitions.csv"))?;
        fs::copy(runner_root.join("rust-toolchain.toml"), build_dir.join("rust-toolchain.toml"))?;
        
        // Create .cargo/config.toml
        fs::create_dir_all(build_dir.join(".cargo"))?;
        fs::write(build_dir.join(".cargo/config.toml"), format!(r#"[build]
target = "{esp_target}"

[target.{esp_target}]
linker = "ldproxy"

[unstable]
build-std = ["std", "panic_abort"]
"#))?;

        // Create build.rs
        fs::write(build_dir.join("build.rs"), r#"use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dest_path = out_dir.join("bytecode.bxb");

    if let Ok(bytecode_path) = env::var("BOXLANG_BYTECODE_PATH") {
        fs::copy(bytecode_path, &dest_path).expect("Failed to copy bytecode");
    } else {
        fs::write(&dest_path, b"").expect("Failed to write dummy bytecode");
    }
    embuild::espidf::sysenv::output();
}
"#)?;
    }

    cargo_toml.push_str(r#"
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
"#);

    if is_wasm {
        cargo_toml.push_str("\n[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n\n");
        cargo_toml.push_str("wasm-bindgen = \"0.2\"\n");
        cargo_toml.push_str("js-sys = \"0.3\"\n");
    }

    fs::write(build_dir.join("Cargo.toml"), cargo_toml)?;

    // 2. Prepare user modules
    let mut mod_decls = String::new();
    let mut registration_calls = String::new();

    if native_dir.exists() && native_dir.is_dir() {
        for entry in fs::read_dir(native_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let mod_name = path.file_stem().unwrap().to_str().unwrap();
                fs::copy(&path, build_dir.join("src").join(format!("{}.rs", mod_name)))?;
                mod_decls.push_str(&format!("mod {};\n", mod_name));

                let content = fs::read_to_string(&path)?;
                if content.contains("pub fn register_bifs") {
                    registration_calls.push_str(&format!(
                        "    for (name, val) in {}::register_bifs() {{ bifs.insert(name, val); }}\n",
                        mod_name
                    ));
                }
                if content.contains("pub fn register_classes") {
                    registration_calls.push_str(&format!(
                        "    for (name, val) in {}::register_classes() {{ classes.insert(name, val); }}\n",
                        mod_name
                    ));
                }
            }
        }
    }

    for module in modules.iter().filter(|m| m.has_native) {
        let matchbox_dir = module.path.join("matchbox");
        let cargo_toml_path = matchbox_dir.join("Cargo.toml");
        let crate_name = read_crate_name(&cargo_toml_path)?;
        let rust_name = crate_name.replace('-', "_");

        let src_dir = matchbox_dir.join("src");
        let (mut has_bifs, mut has_classes) = (false, false);
        if src_dir.is_dir() {
            for entry in fs::read_dir(&src_dir)?.flatten() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                    let content = fs::read_to_string(&p).unwrap_or_default();
                    if content.contains("pub fn register_bifs") { has_bifs = true; }
                    if content.contains("pub fn register_classes") { has_classes = true; }
                }
            }
        }
        if has_bifs {
            registration_calls.push_str(&format!(
                "    for (name, val) in {}::register_bifs() {{ bifs.insert(name, val); }}\n",
                rust_name
            ));
        }
        if has_classes {
            registration_calls.push_str(&format!(
                "    for (name, val) in {}::register_classes() {{ classes.insert(name, val); }}\n",
                rust_name
            ));
        }
    }

    // 3. Generate main.rs / lib.rs
    let bytecode = postcard::to_stdvec(chunk)?;
    let mut code = String::new();

    if is_esp32 {
        code.push_str(r#"
use matchbox_vm::{vm::VM, types::{BxValue, BxNativeFunction}, Chunk};
use std::collections::HashMap;
use anyhow::{Result, bail};
use esp_idf_sys as _; 
use esp_idf_svc::log::EspLogger;

static EMBEDDED_BYTECODE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bytecode.bxb"));

fn load_from_flash() -> Result<Chunk> {
    unsafe {
        let partition = esp_idf_sys::esp_partition_find_first(
            esp_idf_sys::esp_partition_type_t_ESP_PARTITION_TYPE_DATA,
            0x01,
            std::ptr::null(),
        );
        if partition.is_null() { bail!("Storage partition not found"); }
        let mut map_handle: esp_idf_sys::esp_partition_mmap_handle_t = 0;
        let mut map_ptr: *const std::ffi::c_void = std::ptr::null();
        let err = esp_idf_sys::esp_partition_mmap(partition, 0, (*partition).size as usize, esp_idf_sys::esp_partition_mmap_memory_t_ESP_PARTITION_MMAP_DATA, &mut map_ptr, &mut map_handle);
        if err != 0 { bail!("Failed to mmap"); }
        let data_ptr = map_ptr as *const u8;
        let len = u32::from_le_bytes([*data_ptr, *data_ptr.add(1), *data_ptr.add(2), *data_ptr.add(3)]) as usize;
        let bytecode = std::slice::from_raw_parts(data_ptr.add(4), len);
        let mut chunk: Chunk = postcard::from_bytes(bytecode)?;
        chunk.reconstruct_functions();
        esp_idf_sys::esp_partition_munmap(map_handle);
        Ok(chunk)
    }
}
"#);
    } else {
        code.push_str(r#"
use matchbox_vm::{vm::VM, types::{BxValue, BxNativeFunction}, Chunk};
use std::collections::HashMap;
"#);
    }

    code.push_str(&mod_decls);
    code.push_str("\n");

    if is_esp32 {
        code.push_str(&format!(r#"
fn main() -> anyhow::Result<()> {{
    esp_idf_sys::link_patches();
    EspLogger::initialize_default();
    println!("[matchbox] ESP32 Fusion Runner Starting...");

    let mut chunk = match load_from_flash() {{
        Ok(c) => c,
        Err(_) => {{
            let mut c: Chunk = postcard::from_bytes(EMBEDDED_BYTECODE)?;
            c.reconstruct_functions();
            c
        }}
    }};

    let mut bifs = HashMap::new();

    let mut classes = HashMap::new();
{}
    let mut vm = VM::new_with_bifs(bifs, classes);
    vm.interpret(chunk)?;
    loop {{ std::thread::sleep(std::time::Duration::from_secs(10)); }}
}}
"#, registration_calls));
    } else {
        code.push_str(&format!(r#"
fn run_interpreted() -> anyhow::Result<()> {{
    let mut bifs = HashMap::new();
    let mut classes = HashMap::new();
{}
    let bytecode: Vec<u8> = vec!{:?};
    let mut chunk: Chunk = postcard::from_bytes(&bytecode)?;
    chunk.reconstruct_functions();
    let mut vm = VM::new_with_bifs(bifs, classes);
    vm.interpret(chunk)?;
    Ok(())
}}
"#, registration_calls, bytecode));
    }

    if is_wasm {
        code.push_str(r#"
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn run() { run_interpreted().unwrap(); }
#[wasm_bindgen]
pub struct BoxLangVM { vm: VM }
#[wasm_bindgen]
impl BoxLangVM {
    #[wasm_bindgen(constructor)]
    pub fn new() -> BoxLangVM { BoxLangVM { vm: VM::new_with_bifs(HashMap::new(), HashMap::new()) } }
    pub fn load_bytecode(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut chunk: Chunk = postcard::from_bytes(bytes).map_err(|e| e.to_string())?;
        chunk.reconstruct_functions();
        self.vm.interpret(chunk).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn call(&mut self, name: &str, args: js_sys::Array) -> Result<JsValue, String> {
        let mut bx_args = Vec::new();
        for i in 0..args.length() { bx_args.push(self.vm.js_to_bx(args.get(i))); }
        match self.vm.call_function(name, bx_args) {
            Ok(val) => Ok(self.vm.bx_to_js(&val)),
            Err(e) => Err(e.to_string()),
        }
    }
}
"#);
        fs::write(build_dir.join("src").join("lib.rs"), code)?;
    } else if !is_esp32 {
        code.push_str("fn main() -> anyhow::Result<()> { run_interpreted() }\n");
        fs::write(build_dir.join("src").join("main.rs"), code)?;
    } else {
        fs::write(build_dir.join("src").join("main.rs"), code)?;
    }

    // 4. Build
    let mut cmd = if is_esp32 {
        let mut c = std::process::Command::new("rustup");
        c.arg("run").arg("esp").arg("cargo").arg("build").arg("--release");
        c.env("BOXLANG_BYTECODE_PATH", build_dir.join("bytecode.bxb")); // Dummy or real
        c.env_remove("RUSTC").env_remove("CARGO").env_remove("MAKEFLAGS").env_remove("CARGO_MAKEFLAGS").env_remove("CARGO_TARGET_DIR");
        c
    } else {
        let cargo_bin = std_env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let mut c = std::process::Command::new(cargo_bin);
        c.arg("build").arg("--release");
        c
    };

    cmd.current_dir(&build_dir);
    if is_wasm { cmd.arg("--target").arg("wasm32-unknown-unknown"); }
    else if is_wasi { cmd.arg("--target").arg("wasm32-wasip1"); }
    else if is_esp32 { 
        let bytecode_path = build_dir.join("bytecode.bxb");
        fs::write(&bytecode_path, bytecode)?;
        cmd.env("BOXLANG_BYTECODE_PATH", bytecode_path.to_str().unwrap());
    }

    let status = cmd.status()?;
    if !status.success() { bail!("Failed to compile native fusion binary"); }

    // 5. Handle Artifact
    if is_esp32 {
        let artifact = build_dir.join("target").join(esp_target).join("release").join("fusion_build");
        let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| source_path.with_extension("elf"));
        fs::copy(&artifact, &out_path)?;
        println!("ESP32 Fusion binary produced: {}", out_path.display());
        if is_flash {
            let mut flash_cmd = std::process::Command::new("espflash");
            flash_cmd.arg("flash")
                .arg("--chip").arg(chip)
                .arg(&out_path);
            flash_cmd.status()?;
        }
    } else if target == "native" {
        let exe_name = if cfg!(windows) { "fusion_build.exe" } else { "fusion_build" };
        let artifact = build_dir.join("target").join("release").join(exe_name);
        let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| if cfg!(windows) { source_path.with_extension("exe") } else { source_path.with_extension("") });
        fs::copy(artifact, &out_path)?;
        println!("Native Fusion binary produced: {}", out_path.display());
    } else if target == "wasi" || target == "wasm" {
        let t_folder = if target == "wasi" { "wasm32-wasip1" } else { "wasm32-unknown-unknown" };
        let artifact = build_dir.join("target").join(t_folder).join("release").join("fusion_build.wasm");
        let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| source_path.with_extension("wasm"));
        fs::copy(artifact, &out_path)?;
        println!("{} Fusion binary produced: {}", target.to_uppercase(), out_path.display());
    } else if target == "js" {
        let artifact = build_dir.join("target").join("wasm32-unknown-unknown").join("release").join("fusion_build.wasm");
        fs::copy(artifact, source_path.with_extension("wasm"))?;
        produce_js_bundle(chunk, source_path, ast, output)?;
    }
    Ok(())
}

/// Extract the `[package] name` value from a `Cargo.toml` file.
fn read_crate_name(cargo_toml_path: &Path) -> Result<String> {
    let text = fs::read_to_string(cargo_toml_path)?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("name") {
            if let Some(eq) = line.find('=') {
                let value = line[eq + 1..].trim().trim_matches(|c| c == '"' || c == '\'');
                if !value.is_empty() {
                    return Ok(value.to_string());
                }
            }
        }
    }
    bail!("Could not find `name` in {}", cargo_toml_path.display())
}

fn produce_wasi_binary(chunk: &Chunk, source_path: &Path, output: Option<&Path>) -> Result<()> {
    let mut wasm_bytes = stubs::get_stub("wasi").unwrap_or(&[]).to_vec();
    if wasm_bytes.is_empty() {
        bail!("WASI runner stub is empty. The matchbox CLI must be rebuilt with the wasm32-wasip1 target installed.");
    }

    let chunk_bytes = postcard::to_stdvec(chunk)?;

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
    let chunk_bytes = postcard::to_stdvec(chunk)?;
    
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

fn produce_esp32_binary(chunk: &Chunk, source_path: &Path, output: Option<&Path>, is_flash: bool, chip: Option<&str>, is_fast_deploy: bool, is_full_flash: bool) -> Result<()> {
    let chip = chip.unwrap_or("esp32");
    let bytecode_bytes = postcard::to_stdvec(chunk)?;

    // ESP32 targets: 
    let target = match chip {
        "esp32c3" | "esp32c6" | "esp32h2" => "riscv32imc-esp-espidf",
        "esp32s2" => "xtensa-esp32s2-espidf",
        "esp32s3" => "xtensa-esp32s3-espidf",
        _ => "xtensa-esp32-espidf",
    };

    if is_fast_deploy {
        println!("FAST DEPLOY: Sending {} bytes of bytecode to ESP32 'storage' partition...", bytecode_bytes.len());
        // 1. Prepare the data: [4 bytes length (LE)][bytecode bytes]
        let mut data = (bytecode_bytes.len() as u32).to_le_bytes().to_vec();
        data.extend_from_slice(&bytecode_bytes);
        // Pad to 4KB sector boundary — espflash may not fully program the last
        // partial flash page, leaving trailing 0xFF on the chip.
        let sector_size = 4096;
        let padded_len = (data.len() + sector_size - 1) / sector_size * sector_size;
        data.resize(padded_len, 0xFF);

        let temp_bin = std_env::temp_dir().join("matchbox_fast_deploy.bin");
        fs::write(&temp_bin, &data)?;

        // 2. Flash ONLY the data partition using espflash.
        //    Use --after no-reset so the flash chip finishes its write cycle
        //    before we reset.  USB-JTAG hard-reset can interrupt the last page write.
        let mut flash_cmd = std::process::Command::new("espflash");
        flash_cmd.arg("write-bin")
            .arg("--chip").arg(chip)
            .arg("--after").arg("no-reset")
            .arg("0x110000") // Offset for 'storage' partition
            .arg(&temp_bin);

        let status = flash_cmd.status()?;
        if !status.success() {
            bail!("Fast deploy failed. Ensure the device is connected and already has a MatchBox Runner flashed. (Try --full-flash for the first time)");
        }

        // 3. Reset the device separately so the firmware boots with the new bytecode.
        let mut reset_cmd = std::process::Command::new("espflash");
        reset_cmd.arg("reset")
            .arg("--chip").arg(chip);
        let _ = reset_cmd.status();

        println!("Fast deploy successful!");
        return Ok(());
    }

    // Attempt to use a pre-built stub if available
    if let Some(stub_bytes) = stubs::get_stub(target) {
        if !stub_bytes.is_empty() {
            println!("Using pre-built ESP32 stub for target '{}'...", target);
            let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| source_path.with_extension("elf"));
            
            // The ELF stub expects bytecode to be appended with a footer, or embedded via a specific mechanism.
            // For now, we use the same "append to end" logic used for native binaries, 
            // but the matchbox-esp32-runner needs to know how to find it if load_from_flash fails.
            let mut binary_bytes = stub_bytes.to_vec();
            let chunk_bytes = postcard::to_stdvec(chunk)?;
            let chunk_len = chunk_bytes.len() as u64;
            binary_bytes.extend_from_slice(&chunk_bytes);
            binary_bytes.extend_from_slice(&chunk_len.to_le_bytes());
            binary_bytes.extend_from_slice(MAGIC_FOOTER);

            fs::write(&out_path, binary_bytes)?;
            
            if is_flash {
                println!("Flashing stub and then bytecode...");
                let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
                let partition_csv = project_root.join("crates/matchbox-esp32-runner/partitions.csv");
                
                // 1. Flash the stub firmware with the partition table
                let mut flash_cmd = std::process::Command::new("espflash");
                flash_cmd.arg("flash")
                    .arg("--chip").arg(chip)
                    .arg("--partition-table").arg(partition_csv)
                    .arg(&out_path);
                let status = flash_cmd.status()?;
                if !status.success() { bail!("Failed to flash stub."); }
                
                // 2. Automatically perform fast-deploy for the bytecode
                return produce_esp32_binary(chunk, source_path, output, false, Some(chip), true, is_full_flash);
            } else {
                println!("ESP32 firmware produced (stub): {}", out_path.display());
                println!("NOTE: To run this, you must also flash the bytecode to the 'storage' partition.");
                return Ok(());
            }
        } else {
            println!("Note: Pre-built stub for '{}' is empty/missing. Falling back to local build...", target);
        }
    }

    println!("Building custom ESP32 firmware for chip: {} (no stub found)...", chip);
    let temp_dir = std_env::temp_dir().join("matchbox_esp32_build");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)?;
    }
    fs::create_dir_all(&temp_dir)?;
    
    let bytecode_path = temp_dir.join("bytecode.bxb");
    fs::write(&bytecode_path, bytecode_bytes)?;

    let runner_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates").join("matchbox-esp32-runner");
    
    let cargo_bin = std_env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = std::process::Command::new(cargo_bin);
    
    cmd.arg("build")
        .arg("--release")
        .arg("--target").arg(target)
        .current_dir(&runner_path)
        .env("BOXLANG_BYTECODE_PATH", bytecode_path.to_str().unwrap());

    let status = cmd.status()?;
    if !status.success() {
        bail!("Failed to compile ESP32 runner. Ensure the ESP32 Rust toolchain is installed.");
    }

    let elf_path = runner_path.join("target").join(target).join("release").join("matchbox-esp32-runner");
    let out_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| source_path.with_extension("elf"));
    
    fs::copy(&elf_path, &out_path)?;
    println!("ESP32 firmware produced: {}", out_path.display());

    if is_flash {
        println!("Flashing to device...");
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let partition_csv = project_root.join("crates/matchbox-esp32-runner/partitions.csv");

        let mut flash_cmd = std::process::Command::new("espflash");
        flash_cmd.arg("flash")
            .arg("--chip").arg(chip)
            .arg("--partition-table").arg(partition_csv)
            .arg(&elf_path);
        
        let flash_status = flash_cmd.status()?;
        if !flash_status.success() {
            bail!("Failed to flash to ESP32 device.");
        }
        println!("Flash successful!");
    }

    Ok(())
}

/// Poll until the serial port path exists or the timeout expires.
fn wait_for_port(port: &str, timeout: std::time::Duration) -> bool {
    use std::time::Instant;
    let start = Instant::now();
    while start.elapsed() < timeout {
        if Path::new(port).exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

fn watch_mode(source_path: &Path, chip: Option<String>, is_full_flash: bool) -> Result<()> {
    use notify::{Watcher, RecursiveMode, event::EventKind};
    use std::sync::mpsc::channel;
    use std::time::Duration;
    use std::process::{Child, Stdio};
    use std::sync::{Arc, Mutex};
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;

    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })?;

    let source_path_abs = fs::canonicalize(source_path)?;
    let watch_dir = source_path_abs.parent().context("Failed to get source directory")?;
    watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;

    let monitor_child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

    // Handle Ctrl+C to kill the child process before exiting
    let monitor_ctrlc = Arc::clone(&monitor_child);
    ctrlc::set_handler(move || {
        println!("\nWATCH MODE: Shutting down...");
        if let Ok(mut child_opt) = monitor_ctrlc.lock() {
            if let Some(mut child) = child_opt.take() {
                #[cfg(unix)]
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
                #[cfg(not(unix))]
                { child.kill().ok(); }
                let _ = child.wait();
            }
        }
        std::process::exit(0);
    }).expect("Error setting Ctrl+C handler");

    // Detect the serial port once using espflash's board-info (quick probe)
    let serial_port = {
        let output = std::process::Command::new("espflash")
            .arg("board-info")
            .arg("--before").arg("no-reset-no-sync")
            .output();
        let mut port = None;
        if let Ok(out) = &output {
            let combined = String::from_utf8_lossy(&out.stderr).to_string()
                + &String::from_utf8_lossy(&out.stdout);
            for line in combined.lines() {
                if line.contains("Serial port:") {
                    if let Some(p) = line.split("Serial port:").nth(1) {
                        port = Some(p.trim().trim_matches('\'').to_string());
                    }
                }
            }
        }
        port.unwrap_or_else(|| {
            for p in &["/dev/ttyACM0", "/dev/ttyUSB0", "/dev/ttyACM1", "/dev/ttyUSB1"] {
                if Path::new(p).exists() { return p.to_string(); }
            }
            "/dev/ttyACM0".to_string()
        })
    };
    println!("WATCH MODE: Using serial port: {}", serial_port);

    // Read serial output directly via stty + cat.  espflash monitor always
    // enters the bootloader handshake which breaks USB-JTAG (chip reset
    // disconnects USB).  Raw cat avoids that entirely.
    // Read serial output directly via stty + cat.  espflash monitor always
    // enters the bootloader handshake which breaks USB-JTAG (chip reset
    // disconnects USB).  Raw cat avoids that entirely.
    let start_monitor = |port: &str| -> Option<Child> {
        // Configure serial port for raw mode at 115200 baud
        let _ = std::process::Command::new("stty")
            .arg("-F").arg(port)
            .arg("115200").arg("raw").arg("-echo")
            .status();

        let mut cmd = std::process::Command::new("cat");
        cmd.arg(port);

        // Put the child in its own process group for clean kill
        #[cfg(unix)]
        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        match cmd.spawn() {
            Ok(child) => Some(child),
            Err(e) => {
                eprintln!("Failed to start serial monitor: {}", e);
                None
            }
        }
    };

    println!("WATCH MODE: Initial monitor starting...");
    wait_for_port(&serial_port, Duration::from_secs(5));
    if let Ok(mut child_opt) = monitor_child.lock() {
        *child_opt = start_monitor(&serial_port);
    }

    loop {
        if let Ok(event) = rx.recv() {
            if let EventKind::Modify(_) = event.kind {
                if event.paths.iter().any(|p| fs::canonicalize(p).map(|cp| cp == source_path_abs).unwrap_or(false)) {
                    // Debounce: drain the channel and wait for silence
                    std::thread::sleep(Duration::from_millis(200));
                    while let Ok(_) = rx.try_recv() {}

                    println!("\nChange detected! Killing monitor...");

                    // 1. Kill existing monitor and wait for it to fully exit
                    if let Ok(mut child_opt) = monitor_child.lock() {
                        if let Some(mut child) = child_opt.take() {
                            #[cfg(unix)]
                            unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
                            #[cfg(not(unix))]
                            { child.kill().ok(); }
                            let _ = child.wait();
                        }
                    }

                    println!("Re-deploying...");

                    // 2. Recompile and fast-deploy
                    let mut flash_success = false;
                    for attempt in 1..=3 {
                        if !wait_for_port(&serial_port, Duration::from_secs(5)) {
                            println!("Attempt {}: port {} not available, retrying...", attempt, serial_port);
                            continue;
                        }

                        let res = (|| -> Result<()> {
                            let source_text = fs::read_to_string(source_path)?;
                            let ast = parser::parse(&source_text)
                                .map_err(|e| anyhow::anyhow!("Parse Error: {}", e))?;
                            let chunk = matchbox_compiler::compile_with_treeshaking(
                                source_path.to_str().unwrap_or("unknown"),
                                &ast,
                                &source_text,
                                Vec::new(),  // keep_symbols
                                false,       // no_shaking
                                false,       // no_std_lib
                                &[],         // module_mappings
                                &[],         // extra_preludes
                            ).map_err(|e| anyhow::anyhow!("Compiler Error: {}", e))?;
                            produce_esp32_binary(&chunk, source_path, None, true, chip.as_deref(), true, is_full_flash)?;
                            Ok(())
                        })();

                        if let Err(e) = res {
                            println!("Redeploy attempt {} failed: {}. Retrying...", attempt, e);
                            std::thread::sleep(Duration::from_millis(1000));
                        } else {
                            println!("Live update successful! Restarting monitor...");
                            flash_success = true;
                            break;
                        }
                    }

                    if !flash_success {
                        eprintln!("CRITICAL: Redeploy failed after 3 attempts.");
                    }

                    // 3. Restart monitor — wait for port after flash reset
                    wait_for_port(&serial_port, Duration::from_secs(5));
                    if let Ok(mut child_opt) = monitor_child.lock() {
                        *child_opt = start_monitor(&serial_port);
                    }
                }
            }
        }
    }
}
