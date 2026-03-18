# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-03-18

### Added
- **Built-in Web Server**: Integrated a high-performance native web server based on Axum and Tokio directly into the CLI.
- **BoxLang Markup (BXM)**: New `.bxm` template support with a pluggable transpiler that converts markup into high-speed VM bytecode.
- **Web Scopes**: Automatic injection of `url`, `form`, `cookie`, `session`, and `cgi` scopes for web requests.
- **Integrated Web CLI**: New CLI flags: `--serve`, `--port`, `--host`, and `--webroot` for instant web hosting.
- **Output Buffering**: Added request-level output buffering to the VM and a new `writeOutput()` BIF.
- **Easy Install Scripts**: One-liner installation scripts for Linux, macOS (bash), and Windows (PowerShell).
- **Multiple Release Variants**: CI now produces "Fat" (all-in-one), "Slim" (no stubs), and "Server" (headless runtime) binaries.
- **Module System**: Dynamic module discovery from `matchbox.toml` and `--module` CLI flags; new `ModuleInfo` struct encapsulating module metadata.
- **Native Module Support**: Load native Rust `.so` modules and auto-register exported BIFs without boilerplate.
- **Module Settings**: Settings support in module lifecycle hooks for configurable modules.
- **Cranelift JIT Compilation (Tier-1 & Tier-2)**: Initial JIT backend using Cranelift. Tier-1 eliminates dead-code in empty loops; Tier-2 translates numeric loop bodies to Cranelift SSA IR.
- **JIT Type Guards and Deoptimization**: Type-checked JIT paths that fall back to the interpreter on type mismatch.
- **JIT Tier-3 — Array Iteration**: `ArrayIterJitFn` for JIT-compiled numeric array iteration.
- **JIT Tier-3 — Struct Inline Caching**: Per-site polymorphic shape-based caching for struct property access within JIT-compiled code, with deoptimization support.
- **JIT Hot Function Compilation and OSR**: Persistent profiling counters across quanta, fast-path execution for hot functions, and On-Stack Replacement (OSR) for long-running loops.
- **JIT Tier-4 — Leaf Function Calls**: JIT compilation for call sites targeting leaf functions.
- **JIT Shape ID and Property Loading**: Struct property access via JIT using shape IDs and inline caches.
- **JIT String Concatenation**: JIT-compiled string concatenation via `jit_concat`.
- **JIT Enabled by Default**: JIT compilation is now active by default; no extra flag required.
- **File System BIFs**: `directoryExists`, `directoryCreate`, `directoryDelete`, `directoryList`, `fileExists`, `fileDelete`, `fileMove`, `fileCopy`, `fileInfo`, `fileCreateSymlink`, `fileSetExecutable`, `fileRead`, `fileWrite`.
- **HTTP BIF**: `httpRequest` supporting GET, POST, PUT, and DELETE methods.
- **JSON BIFs**: `jsonSerialize` and `jsonDeserialize` for encoding and decoding JSON data.
- **Cryptography BIF**: `hash` for SHA-256 hashing.
- **String BIFs**: `trim`, `listToArray`, `indexOf`, and `chr`.
- **ESP32 Firmware Support**: New `matchbox-esp32-runner` crate; CLI flags `--flash`, `--full-flash`, and `--chip` for building and flashing bytecode to ESP32 devices; watch mode for live updates during development.
- **Web WASM Target**: Stub management extended to support browser/web WASM targets alongside the existing WASM container target.
- **Elvis Operator (`?:`)**: Null-coalescing Elvis operator for concise null checks.
- **Safe Member Access (`?.`)**: Null-safe chained member access operator.
- **`while` Loop**: Full `while` loop support in the parser and compiler.
- **`switch` Statement**: `switch`/`case` statement with `break` support.

### Changed
- **Weakly Typed Math**: The `ADD` opcode now automatically converts strings to numbers if possible, matching BoxLang behavior.
- **Escaped Quote Support**: Strings now support BoxLang-style escaping using double-quotes (`""` and `''`).
- **Flat Function Representation**: Refactored `Chunk` and VM to use a flat, string-keyed function model instead of nested chunks; `CallFrame` now carries a direct chunk reference for improved dispatch.
- **Watch Mode**: Platform-specific process handling for more reliable live-reload behavior on Linux, macOS, and Windows.

### Fixed
- Fixed literal hash (`#`) parsing issues in `bxm` templates by implementing `##` escaping.
- ESP32 binary production logic and serial port handling in watch mode.
- ESP32 Docker image tagging and `Reflect.set` index usage.
- Set `RUSTUP_HOME` and `CARGO_HOME` correctly in ESP32 build environment.
- Regression in `LOCAL_JUMP_IF_NE_CONST` optimization and return propagation.

## [0.3.0] - 2026-03-09

### Added
- **System BIFs**: Added `createUUID()`, `createGUID()`, and `getSystemSetting()`.
- **CLI BIFs**: Added `cliGetArgs()` (structured parsing), `cliRead()`, `cliConfirm()`, `cliClear()`, and `cliExit()`.
- **Expanded Array Library**: 20+ new BIFs including `arraySort`, `arrayFindNoCase`, `arrayAvg`, `arraySum`, `arrayUnique`, etc.
- **Expanded Struct Library**: 15+ new BIFs including `structAppend`, `structMap`, `structFilter`, `structSort`, `structKeyExists`, etc.
- **Math BIFs**: `abs`, `min`, and `max`.
- **`rust:` Imports**: Support for importing native Rust modules and classes using `import rust:path.to.Module`.
- **MatchBox Macros**: New `matchbox-macros` crate for zero-boilerplate Rust-to-BoxLang interop (automatic BIF and Class registration).
- **Polymorphic Inline Caches (PIC)**: Enhanced member access performance with support for multiple shapes per call site.
- **Generational Garbage Collection**: Improved GC performance with young/old generation separation and optimized allocation strategies.
- **String Interning**: Added a global string interner for deduplicating identifiers and property names, reducing memory overhead.
- **Fiber Scheduling**: Implemented timeslice-based quantum management and priority handling for concurrent tasks.
- **Logical Operators**: `||`, `&&`, and unary `!` (with short-circuit evaluation).
- **Ternary Operator**: `condition ? trueValue : falseValue` expressions, nestable.
- **`continue` Statement**: Skip to the next iteration in both `for` and `for-in` loops.
- **Sparse Array Assignment**: Assigning to an out-of-bounds array index auto-grows the array, filling gaps with `null`. Reading beyond the end also returns `null` instead of throwing.
- **`--output <path>` CLI flag**: Override the output file path when compiling a script.
- **`--strip-source` CLI flag**: Strip embedded source text from compiled `.bxb` output for smaller binaries (~35% reduction).
- **JS Interop (WASM target)**: `JsHandle`-based host function bridge (`matchbox_js_host`) for DOM and JS API access from scripts.

### Changed
- **Flat u32 Bytecode**: Refactored the entire VM to use a high-performance flat u32 opcode encoding.
- **Optimized Loop Handling**: Added specialized `FOR_LOOP_STEP` opcodes for faster iteration.
- **Generic `len()`**: The `len()` BIF now natively supports Arrays, Structs, and Strings.

### Fixed
- Fixed critical stack management bug in `CALL` where passing too many arguments caused a subtraction overflow.
- Enabled string comparison support in `GREATER` and `LESS` opcodes to allow sorting strings in prelude functions.
- Fixed deserialized function chunks causing an index-out-of-bounds panic in the VM.
- Fixed source text being duplicated into every function/method sub-chunk at compile time.
- `continue` now works correctly inside `for-in` loops.

## [0.2.0] - 2026-03-07

### Added
- **BoxLang-Authored Prelude**: Standard BIFs (e.g., `arrayMap`, `structEach`, `arrayFilter`, `isEmpty`) are now implemented in BoxLang, reducing core VM size.
- **Tree-Shaking Compiler**: Automated dependency analysis to exclude unused prelude functions from standalone binaries.
- **Cross-Compilation Support**: Embedded runner stubs for Linux, macOS, and Windows into "fat" CLI binaries via the `cross-compile` Cargo feature.
- **Multi-Stage CI**: Refactored GitHub Actions to natively build stubs on each OS and aggregate them into single distribution binaries.
- **Namespaced Function Attributes**: Support for `@matchbox-keep` to explicitly preserve symbols from tree-shaking.
- **New CLI Flags**:
  - `--keep <symbols>`: Manually preserve BIFs in the final binary.
  - `--no-shaking`: Disable tree-shaking to include the full prelude.
  - `--no-std-lib`: Completely exclude the standard library.
- **Java Interop**: Implemented dynamic JNI reflection bridge for automatic Java method discovery and invocation.
- **Asynchronous Execution**: Implemented `runAsync` BIF and VM-level fiber spawning.
- **Documentation**: Added comprehensive documentation structure in `/docs`.

### Changed
- Migrated multiple BIFs (Math, Array, Struct) from Rust implementations to the BoxLang prelude.
- Refactored `build.rs` to support pre-existing stub injection and multi-target compilation.
- Unified bytecode serialization to `u32` for better cross-platform compatibility.

### Fixed
- Fixed specialized integer math opcodes (`OpAddInt`, `OpSubInt`, `OpMulInt`) to correctly handle float-backed constants.
- Resolved grammar conflict between classic for-loop initialization and optional semicolons.
- Fixed case-insensitivity in member method resolution (e.g., `.toUpperCase()` correctly maps to `ucase`).

## [0.1.0] - 2026-03-07

### Added
- Portable runner stubs architecture for ultra-lean standalone native binaries.
- Cargo Workspace refactoring: separated `matchbox-vm`, `matchbox-compiler`, and `matchbox-runner`.
- Class inheritance support via `extends` attribute.
- Implicit accessors support (`accessors="true"`) for class properties.
- Optional type hints and access modifiers for functions and parameters.
- Default arguments support for function parameters.
- Interfaces with support for abstract methods, default implementations, and multiple implementation.
- `onMissingMethod` magic method for dynamic method interception in classes.
- Semantic `.onError()` member method for asynchronous Futures.
- Multi-target Native Fusion (Hybrid builds for Native, WASM, and JS).
- Persistent `BoxLangVM` for WASM with dynamic `call()` support.
- Automated JavaScript module generation via `--target js`.
- Member method delegation to BIFs (e.g., `"foo".ucase()`).
- High-performance integration testing macro.
- GitHub Actions for automated multi-platform builds.
- Tracing Mark-and-Sweep Garbage Collector.
- Hidden Classes (Shapes) and Monomorphic Inline Caches for performance.

### Changed
- Renamed project from `bx-rust` to `MatchBox`.
- Refactored binary into a library/binary hybrid.
- Updated GitHub workflows to support workspace building.

### Fixed
- Fixed critical stack management bug in `OpInvoke`.
- Fixed parser panic on empty anonymous function parameters.
- Fixed case-insensitive function lookup in WASM/JS bridge.
- Fixed greedy parsing issue in function declarations.
- Fixed various WASM runtime errors.
- Object lifetime issues in JNI bridge for release builds.
