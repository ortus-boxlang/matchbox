# MatchBox (BoxLang Rust Implementation)

A high-performance, native Rust implementation of the [BoxLang](https://github.com/ortus-boxlang/BoxLang) programming language. MatchBox provides a fast, JVM-independent runtime targeting native binaries, WebAssembly, and embedded systems (ESP32).

## Quick Install

These scripts will prompt you to choose between the **Latest Release** or **Snapshot** version and will install the full **Fat CLI** (which includes runner stubs for all deployment targets).

**Linux / macOS:**
```bash
curl -sSL https://raw.githubusercontent.com/ortus-boxlang/matchbox/master/install/install.sh | bash
```

**Windows (PowerShell):**
```powershell
iex (Invoke-RestMethod -Uri https://raw.githubusercontent.com/ortus-boxlang/matchbox/master/install/install.ps1)
```

## Core Features

- **Bytecode VM**: Fast, stack-based execution engine with a multi-tier JIT compiler.
- **Web Server & BXM**: Built-in high-performance web server with support for BoxLang Markup (`.bxm`).
- **Virtual Threading (Fibers)**: High-concurrency cooperative scheduler supporting `runAsync` and non-blocking `sleep`.
- **OO & Interfaces**: Full support for Classes, Inheritance, and trait-like Interfaces with default implementations.
- **Native Fusion**: High-speed interoperability with native Rust code.
- **Edge Ready**: Capability to produce ultra-lean (~500KB) standalone native and WASM binaries.

## BoxLang Compatibility

MatchBox aims for high compatibility with the core BoxLang specification. Most standard syntax, including Classes, User Defined Functions (UDFs), Closures, and Async programming, is fully supported. We are actively implementing additional Built-in Functions (BIFs) and expanding compatibility with the broader BoxLang ecosystem every day.

## Release Variants

MatchBox is distributed in three distinct variants to suit different deployment needs:

| Variant | Binary Name | Description | Best For... |
| :--- | :--- | :--- | :--- |
| **Fat CLI** | `matchbox` | The complete developer tool. Includes the VM, Compiler, REPL, and embedded runner stubs for all targets (Native, WASM, ESP32). | Local development, cross-compiling, and building standalone apps. |
| **Slim CLI** | `matchbox-slim` | VM, Compiler, and REPL. Excludes embedded cross-compilation stubs to reduce binary size by ~20MB. | CI/CD pipelines and environments where only local execution is needed. |
| **Server** | `matchbox-server` | An optimized, standalone web runtime. Excludes CLI developer tools and focuses entirely on serving `.bxm` and `.bxs` files. | Production web deployments, Docker containers, and edge hosting. |

## Quick Start

### 1. Interactive REPL
Start the BoxLang REPL by running the binary without arguments:
```bash
matchbox
```

### 2. Running a Web Server
Start the built-in server to host BoxLang Markup (`.bxm`) and scripts:
```bash
matchbox --serve --port 8080 --webroot ./www
```

### 3. Compiling to Standalone Native Binary
Bundle your BoxLang code into a single, zero-dependency executable for your current OS:
```bash
matchbox --target native my_app.bxs
```

## Deployment Targets

### Native Binaries
Create zero-dependency, standalone executables for Linux, macOS, and Windows. These binaries bundle the MatchBox VM engine with your compiled BoxLang bytecode for near-instant startup and minimal resource usage.
```bash
matchbox --target native my_app.bxs
```

### WASM & WASI Containers
MatchBox is fully compatible with the WebAssembly System Interface (WASI). Compile your BoxLang code into standard `.wasm` files that can run in the browser, edge platforms (like Cloudflare or Vercel), or within WASI-compliant containers (like Docker WASM).
```bash
matchbox --target wasm my_app.bxs   # Produces a standalone WASM/WASI binary
matchbox --target js   my_lib.bxs   # Produces an ES Module wrapper
```

### ESP32 Embedded
MatchBox can flash BoxLang bytecode directly to ESP32 microcontrollers, enabling high-level language features on low-power hardware.
```bash
matchbox --target esp32 --chip esp32s3 app.bxs --flash
```

## Technical Architecture

1. **Parser**: Built using [Pest](https://pest.rs/) (PEG Grammar).
2. **Compiler**: Multi-stage compiler producing opcodes with line-number metadata.
3. **VM**: Stack-based machine with a cooperative fiber scheduler.
4. **BXM Transpiler**: Ahead-of-time markup transpilation for near-native template rendering.
5. **Portability**: Native binaries are produced by appending bytecode to pre-compiled architecture-specific runner stubs.
