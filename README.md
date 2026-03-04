# bx-rust

A high-performance, native Rust implementation of the BoxLang programming language. This project features a stack-based Bytecode Virtual Machine (VM) and a multi-stage compiler, providing a standalone runtime independent of the JVM.

## Core Features

- **Bytecode VM**: Fast, stack-based execution engine with support for nested call frames.
- **Virtual Threading (Fibers)**: High-concurrency cooperative scheduler supporting `runAsync` and non-blocking `sleep`.
- **OO Support**: Full support for Classes, Objects, `this` scope, and `variables` (private) scope.
- **Modern Syntax**: Support for UDFs, Closures, Arrow functions (Lambdas), and String Interpolation.
- **JS Interop (WASM)**: Direct access to JavaScript APIs and DOM manipulation when running in the browser.
- **Deployment**: Capability to produce standalone native and WASM binaries.

## Usage Guide

The `bx-rust` binary is a versatile tool that can interpret source code, compile to portable bytecode, or bundle applications into standalone executables.

### 1. Running Source Code (Interpreter Mode)
Run a BoxLang script (`.bxs`) directly from source. The tool will parse, compile to memory, and execute it immediately.

```bash
bx-rust my_script.bxs
```

### 2. Interactive REPL
Start the BoxLang REPL by running the binary without arguments:

```bash
bx-rust
```

### 3. Compiling to Bytecode
Compile source code into a compact, portable binary format (`.bxb`). This is useful for distribution where you don't want to expose source code or want to skip the parsing phase in production.

```bash
bx-rust --build my_script.bxs
# Produces: my_script.bxb
```

### 4. Producing Standalone Native Binaries
Create a single executable file that contains both the BoxLang VM engine and your compiled code. This binary has **zero dependencies**.

```bash
bx-rust --target native my_script.bxs
# Produces: my_script (an executable)
```

## WebAssembly & Browser Support

`bx-rust` supports running BoxLang directly in the browser via WebAssembly, including a full bridge to JavaScript APIs.

### 1. Building for WASM
To use BoxLang in a web project, first compile the runtime to WASM using `wasm-pack` or `cargo build`:

```bash
# Install wasm-bindgen-cli if you haven't
cargo install -f wasm-bindgen-cli

# Build the runtime
cargo build --target wasm32-unknown-unknown --release

# Generate JS glue code
wasm-bindgen --target web --out-dir ./pkg target/wasm32-unknown-unknown/release/bx_rust.wasm
```

### 2. Including in HTML
You can then initialize the BoxLang VM and run scripts from your HTML:

```html
<script type="module">
    import init, { run_boxlang } from './pkg/bx_rust.js';

    async function run() {
        await init();
        
        const code = `
            doc = js.document;
            app = doc.getElementById("app");
            app.innerHTML = "<h1>Hello from BoxLang WASM!</h1>";
            
            // Async works too!
            runAsync(() => {
                sleep(1000);
                js.console.log("Delayed message from BoxLang fiber");
            });
        `;
        
        run_boxlang(code);
    }
    
    run();
</script>

<div id="app">Loading BoxLang...</div>
```

## Language Support Matrix

| Feature | Status | Syntax Example |
| :--- | :--- | :--- |
| **Variables** | ✅ | `x = 10`, `var y = 20` |
| **Math** | ✅ | `(10 + 5) * 2 / 3` |
| **Logic** | ✅ | `if (x > 5) { ... } else { ... }` |
| **Loops** | ✅ | `for (i=1; i<=10; i++)`, `for (item in arr)` |
| **Arrays** | ✅ | `arr = [1, 2, "three"]`, `arr[1]` (1-indexed) |
| **Structs** | ✅ | `s = { key: "val" }`, `s.key` (case-insensitive) |
| **Functions** | ✅ | `function add(a,b) { return a+b }`, `(x) => x*2` |
| **Strings** | ✅ | `"Hello #name#"`, `str1 & str2` |
| **Classes** | ✅ | `class MyClass { property p; this.p = 1; }` |
| **Exceptions**| ✅ | `try { throw "err"; } catch(e) { ... }` |
| **Async** | ✅ | `f = runAsync(task); f.get(); sleep(100);` |
| **JS Interop**| ✅ | `js.window.location.href`, `js.alert("Hi")` |

## Technical Architecture

1. **Parser**: Built using [Pest](https://pest.rs/) (PEG Grammar).
2. **Compiler**: Multi-stage compiler producing opcodes with line-number metadata.
3. **VM**: Stack-based machine with a cooperative fiber scheduler.
4. **Serialization**: Uses `bincode` for binary bytecode representation.
