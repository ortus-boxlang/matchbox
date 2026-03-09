# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Logical Operators**: `||`, `&&`, and unary `!` (with short-circuit evaluation).
- **Ternary Operator**: `condition ? trueValue : falseValue` expressions, nestable.
- **`continue` Statement**: Skip to the next iteration in both `for` and `for-in` loops.
- **Sparse Array Assignment**: Assigning to an out-of-bounds array index auto-grows the array, filling gaps with `null`. Reading beyond the end also returns `null` instead of throwing.
- **`--output <path>` CLI flag**: Override the output file path when compiling a script.
- **JS Interop (WASM target)**: `JsHandle`-based host function bridge (`matchbox_js_host`) for DOM and JS API access from BoxLang scripts compiled with `--target js`.

### Fixed
- Fixed deserialized function chunks causing an index-out-of-bounds panic in the VM by calling `ensure_caches()` at frame entry when `ip == 0`.
- `continue` now works correctly inside `for-in` loops (was only wired up for C-style `for` loops).

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
