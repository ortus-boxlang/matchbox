# MatchBox (BoxLang Rust Implementation)

A high-performance, native Rust implementation of the [BoxLang](https://github.com/ortus-boxlang/BoxLang) programming language. MatchBox provides a fast, JVM-independent runtime targeting native binaries, WebAssembly, and embedded systems (ESP32).

## Quick Install

These scripts will prompt you to choose between the **Latest Release** or **Snapshot** version and install the full CLI with embedded cross-platform runner stubs.

**Linux / macOS:**
```bash
curl -sSL https://raw.githubusercontent.com/ortus-boxlang/matchbox/master/install/install.sh | bash
```

**Windows (PowerShell):**
```powershell
iex (Invoke-RestMethod -Uri https://raw.githubusercontent.com/ortus-boxlang/matchbox/master/install/install.ps1)
```

**Docker / GHCR:**
```bash
docker pull ghcr.io/ortus-boxlang/matchbox:latest
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --help
```

The Docker image uses `matchbox` as its entrypoint, so it can run scripts, build bytecode, produce standalone artifacts, or serve a mounted webroot.

## Core Features

- **Bytecode VM**: Fast, stack-based execution engine with a multi-tier JIT compiler.
- **Web Runtime**: Built-in static webroot server for `.bxm` plus a routed app server built around `web.server()`, middleware, static asset mounts, and ColdBox-style handlers.
- **Virtual Threading (Fibers)**: High-concurrency cooperative scheduler supporting `runAsync` and non-blocking `sleep`.
- **OO & Interfaces**: Full support for Classes, Inheritance, and trait-like Interfaces with default implementations.
- **Native Fusion**: High-speed interoperability with native Rust code.
- **Edge Ready**: Capability to produce ultra-lean (~500KB) standalone native and WASM binaries.

## BoxLang Compatibility

MatchBox aims for high compatibility with the core BoxLang specification. Most standard syntax, including Classes, User Defined Functions (UDFs), Closures, and Async programming, is fully supported. We are actively implementing additional Built-in Functions (BIFs) and expanding compatibility with the broader BoxLang ecosystem every day.

## Release Downloads

MatchBox publishes one full-featured CLI per supported OS/architecture and a Docker image:

| Offering | Name | Description |
| :--- | :--- | :--- |
| **CLI** | `matchbox-<os>-<arch>` | VM, compiler, REPL, built-in web server, and embedded cross-platform runner stubs. |
| **Docker Image** | `ghcr.io/ortus-boxlang/matchbox` | The same released CLI packaged for Linux `amd64` and `arm64`, with `matchbox` as the entrypoint. |

Slim and standalone server binaries are no longer published. Their source/build options remain available; this changes distribution only. Browser/WASM packaging is deferred.

### Release versioning and downloads

`Cargo.toml` holds the next base version. Develop builds publish `<version>-snapshot+<build-number>`; a push to `master` releases that base version, tags it, and finalizes the changelog. Only after successful publication does CI advance `develop` to the next minor version and start its snapshot build. Stable tags are never overwritten.

GitHub releases retain the platform filenames used by the installers and include `SHA256SUMS` and `version.json`. Downloads are also published under:

```text
https://downloads.ortussolutions.com/ortussolutions/matchbox/<version>/
https://downloads.ortussolutions.com/ortussolutions/matchbox/latest/
https://downloads.ortussolutions.com/ortussolutions/matchbox/snapshot/
```

For example, `0.11.0-snapshot/matchbox-linux-x64` is the version-keyed development download. Stable version directories are immutable; snapshot and evergreen channels are refreshed without long-lived caching. `version.json` records the build number, source revision, and binary checksums.

## Quick Start

### 1. Interactive REPL
Start the BoxLang REPL by running the binary without arguments:
```bash
matchbox
```

### 2. Running the Webroot Server
Start the built-in server to host BoxLang Markup (`.bxm`) and static assets:
```bash
matchbox --serve --port 8080 --webroot ./www
```

### 3. Running the App Server
Start a routed HTTP app built with `web.server()`:
```bash
cargo run -p matchbox_server -- --app docs/examples/app_server/app.bxs
```

For a websocket example:
```bash
cargo run -p matchbox_server -- --app docs/examples/websocket_counter/app.bxs
```

### 4. Compiling to Standalone Native Binary
Bundle your BoxLang code into a single, zero-dependency executable for your current OS:
```bash
matchbox --target native my_app.bxs
```

### 5. Using the Docker Image
Run a script from the current directory:
```bash
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest my_app.bxs
```

Build artifacts into the mounted directory:
```bash
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --build my_app.bxs
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --target wasm my_app.bxs
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --target native my_app.bxs
```

Serve a webroot from a container:
```bash
docker run --rm -p 8080:8080 -v "$PWD/www:/app" \
  ghcr.io/ortus-boxlang/matchbox:latest --serve --host 0.0.0.0 --webroot /app
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
Desktop `bif-zip` is not forwarded to WASM/WASI runners: its C codecs require
an additional C sysroot. Native runners retain ZIP support; enabling it on the
CLI must not prevent packaging web code that does not use ZIP.

### ESP32 Embedded
MatchBox can flash BoxLang bytecode directly to ESP32 microcontrollers, enabling high-level language features on low-power hardware.
```bash
matchbox --target esp32 --chip esp32s3 --flash app.bxs
```

## Technical Architecture

1. **Parser**: Built using [Pest](https://pest.rs/) (PEG Grammar).
2. **Compiler**: Multi-stage compiler producing opcodes with line-number metadata.
3. **VM**: Stack-based machine with a cooperative fiber scheduler.
4. **BXM Transpiler**: Ahead-of-time markup transpilation for near-native template rendering.
5. **Portability**: Native binaries are produced by appending bytecode to pre-compiled architecture-specific runner stubs.

Struct/instance shape layouts use structurally shared HAMT field maps (`rpds`),
not complete field-map copies for each insertion. Shape IDs/property indices
remain immutable for member caches and JIT guards. Layouts still live for the VM
lifetime; this removes quadratic prefix copying, not all metadata retention.
`python3 tests/wide_struct.py /path/to/matchbox` checks source-free wide structs,
ordering/deletion, case-sensitive keys, copies, GC and a Linux 256 MiB RSS ceiling.
It adds no consumer startup download or native-provider ABI change.

Positional native calls skip global-name recovery; named calls still resolve the
BIF name for argument ordering. `python3 tests/native_call_scaling.py /path/to/matchbox`
checks source-free positional/named/reference/callback behavior and rejects
positional-call slowdown from 8,192 unrelated globals.

Canonical interner hits borrow the existing map key before allocating a lowercase
fallback. Mixed-case misses keep Unicode folding/presentation behavior; exact
case-sensitive IDs remain separate. `cargo test -p matchbox_vm --test canonical_keys`
checks allocation-free repeated ASCII/Unicode struct reads and writes through the
public VM API; the wide-struct fixture also checks key spelling and exact Unicode keys.

VM-owned functions share guarded inline-cache buffers across calls instead of
cloning instruction-sized arrays and compiler constant maps. Invocation scopes,
arguments, receivers and promoted GC constants remain independent. Cache state
is reset on script entry/constant promotion; immutable programs remain Send/Sync
and serialized bytecode/native-provider ABI are unchanged. Cache buffers now live
with their owning function: lower allocation traffic trades for retained cache
memory, not a guaranteed RSS reduction. The same public allocation suite checks
cold identifier sites (1/100 reads: 10/10 allocations) and ten warmed calls to
small/padded functions (24,160/24,160 allocated bytes), plus VM identity and
Unicode rope operands. `vm_function_execution.bxs` covers recursion, aliases,
independent closures, changing layouts, case sensitivity and GC.
