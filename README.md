# MatchBox

A high-performance, native Rust implementation of the BoxLang programming language. This project features a stack-based Bytecode Virtual Machine (VM) and a multi-stage compiler, providing a standalone runtime independent of the JVM.

## Core Features

- **Bytecode VM**: Fast, stack-based execution engine with support for nested call frames.
- **Virtual Threading (Fibers)**: High-concurrency cooperative scheduler supporting `runAsync` and non-blocking `sleep`.
- **OO Support**: Full support for Classes, Objects, `this` scope, and `variables` (private) scope.
- **Modern Syntax**: Support for UDFs, Closures, Arrow functions (Lambdas), and String Interpolation.
- **JS Interop (WASM)**: Direct access to JavaScript APIs and DOM manipulation when running in the browser.
- **Deployment**: Capability to produce standalone native and WASM binaries.

## Usage Guide

The `matchbox` binary is a versatile tool that can interpret source code, compile to portable bytecode, or bundle applications into standalone executables.

### 1. Running Source Code (Interpreter Mode)
Run a BoxLang script (`.bxs`) directly from source.

```bash
matchbox my_script.bxs
```

### 2. Interactive REPL
Start the BoxLang REPL by running the binary without arguments:

```bash
matchbox
```

### 3. Compiling to Bytecode
Compile source code into a compact, portable binary format (`.bxb`).

```bash
matchbox --build my_script.bxs
```

### 4. Producing Standalone Native Binaries
Create a single executable file that contains both the BoxLang VM engine and your compiled code.

```bash
matchbox --target native my_script.bxs
```

## WebAssembly & Browser Support

`MatchBox` supports running BoxLang directly in the browser via WebAssembly.

### 1. Runtime Integration (JIT-like)
You can include the BoxLang engine in your page and run source code dynamically.

**Build the runtime:**
```bash
cargo build --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir ./pkg target/wasm32-unknown-unknown/release/matchbox.wasm
```

**Use in HTML:**
```javascript
import init, { run_boxlang } from './pkg/matchbox.js';
await init();
run_boxlang('println("Hello World")');
```

### 2. Ahead-of-Time (AOT) Deployment
For production, you can compile your BoxLang code into a standalone WASM binary that contains your application bytecode in a custom section.

**Compile your app to WASM:**
```bash
# 1. Ensure runtime is built
cargo build --target wasm32-unknown-unknown --release

# 2. Compile your script to a specialized WASM binary
matchbox --target wasm my_app.bxs
# Produces: my_app.wasm
```

**Deploy in the browser:**
```html
<script type="module">
    import init, { run_boxlang_bytecode } from './pkg/matchbox.js';

    async function deploy() {
        // 1. Fetch the WASM binary containing your app
        const response = await fetch('my_app.wasm');
        const buffer = await response.arrayBuffer();
        
        // 2. Initialize the BoxLang engine using the fetched bytes
        await init(buffer);
        
        // 3. Extract and run the embedded bytecode
        const module = await WebAssembly.compile(buffer);
        const sections = WebAssembly.Module.customSections(module, "boxlang_bytecode");
        if (sections.length > 0) {
            run_boxlang_bytecode(new Uint8Array(sections[0]));
        }
    }
    
    deploy();
</script>
```

### 3. JavaScript Module Generation
You can compile BoxLang scripts into native JavaScript modules that run in the browser or Node.js via WASM:

```bash
matchbox --target js my_lib.bxs
```

This produces a `my_lib.js` file that exports all top-level BoxLang functions as asynchronous JS functions:

```javascript
import { multiply } from './my_lib.js';

const result = await multiply(10, 20);
console.log(result); // 200
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
