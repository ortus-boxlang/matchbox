# Matchbox Native Module Guide

This guide explains how to create reusable, Rust-powered modules for the **Matchbox** runtime. These modules allow you to extend the BoxLang language with high-performance native code, custom classes, and hardware-specific features.

## 1. Directory Structure

To create a module named `my-utils`, set up the following structure:

```text
my-utils/
├── ModuleConfig.bx        # Module lifecycle and configuration
├── matchbox/              # Native Rust implementation
│   ├── Cargo.toml         # Rust crate manifest
│   └── src/
│       └── lib.rs         # Registration and logic
└── bifs/                  # (Optional) Pure BoxLang BIFs
    └── extra.bxs
```

## 2. Module Lifecycle (`ModuleConfig.bx`)

Every module must have a `ModuleConfig.bx`. Matchbox executes this in an isolated VM at compile-time to collect metadata and settings.

```boxlang
// ModuleConfig.bx
class {
    // Runs when the module is first discovered
    function onLoad() {
        println("Loading MyUtils module...");
    }

    // Must return a struct of settings. 
    // These are accessible via getModuleSettings("my-utils")
    function configure() {
        return {
            "version": "1.0.0",
            "enabled": true
        };
    }
}
```

## 3. Rust Implementation (`matchbox/`)

The `matchbox/` directory contains a standard Rust library crate.

### `Cargo.toml`
Your crate must depend on `matchbox_vm` and use `crate-type = ["rlib"]`.

```toml
[package]
name = "my-utils-native"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["rlib"]

[dependencies]
# Path is relative to the matchbox/ directory
matchbox_vm = { path = "../../../crates/matchbox-vm" }
```

### `src/lib.rs` - BIFs and Macros
Use macros for simple functions or the manual signature for full control.

```rust
use matchbox_vm::{matchbox_fn, matchbox_class, matchbox_methods, BxObject};
use matchbox_vm::types::{BxValue, BxVM, BxNativeFunction, BxNativeObject};
use std::collections::HashMap;

// --- Option A: Macro-driven BIF (Limited to f64 returns currently) ---
#[matchbox_fn]
pub fn add_numbers(a: f64, b: f64) -> f64 {
    a + b
}

// --- Option B: Manual BIF (Full control over types) ---
pub fn greet(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() { return Err("greet requires 1 arg".into()); }
    let name = vm.to_string(args[0]);
    let msg = format!("Hello, {}!", name);
    // Strings must be allocated on the VM heap
    Ok(BxValue::new_ptr(vm.string_new(msg)))
}

// --- Native Class Implementation ---
#[derive(Debug, BxObject)] // Automates BxNativeObject trait
pub struct Processor {
    pub count: i32,
}

#[matchbox_methods] // Generates the method dispatcher
impl Processor {
    pub fn process(&mut self, val: f64) -> f64 {
        self.count += 1;
        val * 2.0
    }
}

// Registration functions Matchbox looks for:
pub fn register_bifs() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("addNumbers".into(), add_numbers_wrapper as BxNativeFunction);
    map.insert("greet".into(), greet as BxNativeFunction);
    map
}

pub fn register_classes() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    // Classes are registered as: "<crate_name>.<ClassName>"
    // If your Cargo.toml name is "my-utils-native", use "my_utils_native"
    map.insert("my_utils_native.Processor".into(), create_processor as BxNativeFunction);
    map
}

fn create_processor(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let obj = Processor { count: 0 };
    let id = vm.native_object_new(std::rc::Rc::new(std::cell::RefCell::new(obj)));
    Ok(BxValue::new_ptr(id))
}
```

## 4. ESP32 & Cross-Compilation

When building for ESP32, your Rust code runs in a resource-constrained environment (ESP-IDF).

- **Standard Library**: You *can* use `std` (Matchbox uses the `esp-idf-sys` environment), but avoid large allocations.
- **Dependencies**: Ensure dependencies don't require OS features like `fork`, `shm`, or specific SIMD instructions.
- **Panic Behavior**: Panics on ESP32 will trigger a hardware watchdog reset. Always return `Err` instead of using `unwrap()`.
- **Linking**: Matchbox handles the complex linking process when you run `matchbox --target esp32 app.bxs`.

## 5. Dependency Management

Matchbox integrates with the [CommandBox](https://www.ortussolutions.com/products/commandbox) ecosystem. You can manage your modules using `box.json` and the standard `box install` command.

### Using `box.json`

Add your modules to the `dependencies` or `devDependencies` section of your `box.json`. Matchbox will automatically resolve them.

```json
{
    "name": "my-app",
    "dependencies": {
        "my-utils": "./path/to/my-utils",
        "bx-strings": "git+https://github.com/ortus-boxlang/bx-strings.git"
    }
}
```

- **Relative Paths**: If the dependency value is a relative path (e.g., `./libs/my-mod`), Matchbox resolves it directly.
- **Git/Remote**: For remote modules, run `box install`. CommandBox will clone them into the `modules/` or `boxlang_modules/` directory, where Matchbox will automatically discover them.

### Directory Conventions

Matchbox automatically scans the following directories for modules (any folder containing a `ModuleConfig.bx`):

1. `modules/`
2. `boxlang_modules/`

You do **not** need to list these explicitly in `matchbox.toml` if they follow these conventions.

### `matchbox.toml` (Legacy/Explicit)

If you prefer not to use `box.json`, you can still use `matchbox.toml` for explicit path mapping:

```toml
[modules]
my-utils = "./custom/path/to/module"
```

---

## 💡 AI Agent & Developer Reference

### BoxLang Quick-Start for AI
- **Case Sensitivity**: BoxLang variable and function calls are **case-insensitive**. However, Rust registration keys in `HashMap` are **case-sensitive**. Always register BIFs in lowercase to ensure compatibility.
- **String Concatenation**: Use `&`, not `+`. (e.g., `"Hello " & name`).
- **Dynamic Typing**: Everything is a `BxValue`. Use `vm.to_string()`, `val.as_number()`, or `val.as_bool()` in Rust to coerce types.
- **Fibers**: Matchbox uses a fiber-based cooperative scheduler. Long-running Rust BIFs will block the entire thread; for heavy work, use small chunks or allow the VM to yield.

### Common Errors & Troubleshooting
| Issue | Cause | Fix |
| :--- | :--- | :--- |
| `Function [X] not found` | Registration name mismatch. | Check `register_bifs` key vs BoxLang call. |
| `Class [X] not found` | Incorrect `new rust:...` path. | Format must be `rust:crate_name.StructName`. |
| `Illegal Instruction` | Dependency incompatible with ESP32. | Check `Cargo.toml` for target-specific features. |
| `Stack Overflow` | Deep recursion in BIF. | ESP32 stacks are small (usually 8-32KB). Move data to heap. |
| `Type Mismatch` | Macro failed to coerce. | Use manual BIF signature for complex type validation. |

### Registration Logic (Internal)
Matchbox scans all `.rs` files in the module's `matchbox/src` directory. If it finds a function named `register_bifs` or `register_classes`, it generates the glue code to link them into the final binary during the "Native Fusion" build process.
