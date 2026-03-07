# MatchBox Developer Guide

Welcome to MatchBox! This guide is for BoxLang developers who want to write applications that run on the high-performance MatchBox runtime.

## Architecture & Core Differences

MatchBox is a native Rust implementation of BoxLang. While it runs BoxLang scripts (`.bxs`), it represents a fundamentally different architectural approach compared to the main BoxLang JVM runtime.

### What you need to know:
* **Strict Subset:** MatchBox implements a strict subset of language and runtime features. Not all BIFs or advanced dynamic JVM features are available.
* **One-Way Compatibility:** Code written for MatchBox will generally run perfectly on the main BoxLang JVM runtime, but the reverse is not always true.
* **No JVM:** MatchBox is 100% JVM-independent. It does not require Java to be installed on the host machine.
* **The MatchBox VM:** Under the hood, MatchBox uses a custom stack-based Bytecode Virtual Machine with a cooperative fiber scheduler. This allows for lightning-fast execution, extremely low memory overhead, and true non-blocking async operations (`runAsync`, `sleep`).

---

## Build Targets

One of the most exciting features of MatchBox is its deployment flexibility. You can compile your BoxLang code into several standalone formats:

* **Native! (`--target native`)**: Produces a single, ultra-lean executable binary for your OS. Perfect for CLI tools or zero-dependency deployments.
* **Embedded (`--build`)**: Compiles your script into a portable bytecode file (`.bxb`) that can be executed by the MatchBox runner anywhere.
* **WASM (`--target wasm` / `--target js`)**: Compiles your BoxLang app into WebAssembly.
    * **Web:** Run BoxLang directly in the browser using the JavaScript wrapper module.
    * **Containers:** Ideal for edge computing or running inside minimal WASM/Docker runtimes.

---

## Interoperability

### Java Interop
Because MatchBox does not run on the JVM, traditional Java integration is heavily restricted.
* **WASM:** Java interop is completely unsupported in WASM environments.
* **Native:** MatchBox includes experimental JNI (Java Native Interface) support to instantiate Java objects natively. However, this feature is highly experimental and requires a Java installation on the host machine.

### Rust Interop ("Native Fusion")
MatchBox provides a groundbreaking feature called **Native Fusion**. If you need extreme performance or access to system libraries, you can write native Rust functions and statically link them into your BoxLang app!

When MatchBox detects a `native/` directory in your project containing Rust files (`.rs`), it automatically compiles them alongside the MatchBox VM engine, creating a hybrid binary where your Rust code is exposed as BoxLang Built-In Functions (BIFs).

---

## MatchBox CLI

The `matchbox` command-line tool is your gateway to running and compiling applications.

### Installation
*(Assuming MatchBox is distributed via Cargo or pre-built binaries)*
```bash
# Build from source
cargo build --release
# The binary will be located at target/release/matchbox
```

### Usage
* **REPL Mode:** `matchbox` (Starts the interactive console)
* **Interpreter Mode:** `matchbox my_script.bxs` (Runs the script directly)
* **Compile to Bytecode:** `matchbox --build my_script.bxs` (Produces `my_script.bxb`)
* **Produce Native Binary:** `matchbox --target native my_script.bxs`
* **Produce JS/WASM Module:** `matchbox --target js my_script.bxs`

---

## Tutorials

### 1. Short Native App Tutorial
Let's build a standalone native executable.

1. Create a file called `hello.bxs`:
```boxlang
println("Hello from a native BoxLang app!");
```
2. Compile to native:
```bash
matchbox --target native hello.bxs
```
3. Run the resulting binary (e.g., `./hello` on Unix or `hello.exe` on Windows):
```bash
./hello
# Output: Hello from a native BoxLang app!
```

### 2. Short JS WASM Tutorial
Let's compile BoxLang to run in the browser.

1. Create a file called `web.bxs`:
```boxlang
function greet(name) {
    return "Hello " & name & " from WASM!";
}
```
2. Compile to JS/WASM:
```bash
matchbox --target js web.bxs
```
3. This produces `web.js` and `web.wasm`. You can now import it in your HTML/JS:
```html
<script type="module">
    import { greet } from './web.js';
    greet("Developer").then(res => console.log(res));
</script>
```

### 3. Native App Tutorial with Rust Interop
Let's build a hybrid app using Native Fusion to call Rust code from BoxLang.

1. Create your BoxLang script `app.bxs`:
```boxlang
// Call our custom Rust BIF
result = rust_math(10, 5);
println("Result from Rust: " & result);
```

2. Create a directory named `native/` in the same folder as `app.bxs`.

3. Define the BIF in `native/rust_math.rs`:

```rust
use matchbox_vm::types::{BxValue, BxVM, BxNativeFunction};
use std::collections::HashMap;

// The function signature MatchBox expects
pub fn rust_math(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("Expected 2 arguments".to_string());
    }

    // Simple addition in Rust
    let a = args[0].as_number();
    let b = args[1].as_number();

    Ok(BxValue::new_number(a + b))
}

// Register the BIF so BoxLang can find it
pub fn register_bifs() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("rust_math".to_string(), rust_math as BxNativeFunction);
    map
}
```

4. Compile the project:
```bash
matchbox --target native app.bxs
```
MatchBox will detect the `native/` folder, compile the Rust code, bundle the BoxLang bytecode, and produce a single `app` executable.

5. Run it!
```bash
./app
# Output: Result from Rust: 15
```
