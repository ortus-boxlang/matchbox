# Native Builds

MatchBox can compile your BoxLang application into a **single, self-contained native binary** for your operating system. The resulting executable requires no JVM, no MatchBox installation, and no runtime dependencies of any kind.

---

## How it Works

MatchBox uses a **runner stub** architecture to produce lean binaries:

1. A pre-compiled, stripped, architecture-specific runner stub (~500 KB) contains only the VM core.
2. The `matchbox` compiler compiles your `.bxs` source to bytecode.
3. The bytecode is appended to the end of the stub binary.
4. At startup, the stub reads its own trailing bytes, finds the embedded bytecode, and executes it.

This means no intermediate files, no installers, and no external loaders — just a single file you can `chmod +x` and ship.

---

## Building a Native Binary

```bash
matchbox --target native my_app.bxs
```

This produces an executable named after your input file (e.g., `my_app` on macOS/Linux, `my_app.exe` on Windows).

### Full Example

Create `cli_tool.bxs`:

```boxlang
println("MatchBox CLI Tool")
println("Running on MatchBox " & matchbox.version)

for (i = 1; i <= 5; i++) {
    println("Step #i# complete")
}
```

Compile:

```bash
matchbox --target native cli_tool.bxs
```

Run:

```bash
./cli_tool
MatchBox CLI Tool
Step 1 complete
Step 2 complete
Step 3 complete
Step 4 complete
Step 5 complete
```

---

## Cross-Compilation

Native binaries are platform-specific. To produce binaries for other targets you have two options:

### Option A: GitHub Actions (recommended)

Use the included GitHub Actions workflow. Every tagged release automatically builds binaries for:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

### Option B: Manual Cross-Compilation

Add the desired Rust target and build:

```bash
rustup target add x86_64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu
```

Note: cross-compilation may require a cross-linker for the target platform. The [`cross`](https://github.com/cross-rs/cross) tool simplifies this via Docker:

```bash
cargo install cross
cross build --release --target x86_64-unknown-linux-gnu
```

---

## Binary Size

MatchBox applies aggressive size optimizations in the release profile:

```toml
[profile.release]
opt-level    = "z"   # Optimize for size
lto          = true  # Link-Time Optimization
codegen-units = 1    # Better dead-code elimination
panic        = "abort"
strip        = true  # Strip symbols
```

The resulting binaries are typically **~500 KB** — small enough to ship as a GitHub release asset or embed in a container image.

---

## Native Fusion: Rust Interop

**Native Fusion** lets you write performance-critical functions in Rust and expose them as BoxLang BIFs (Built-In Functions), all statically linked into your final binary.

### When to Use It

- You need maximum throughput for a hot path (e.g., data parsing, compression, crypto).
- You want to use a Rust crate (e.g., `serde`, `reqwest`, `image`) from BoxLang.
- You need direct access to OS or hardware APIs.

### Setting Up a Native Fusion Project

1. Create your BoxLang entry point, e.g., `app.bxs`.
2. Create a `native/` directory alongside it.
3. Write one or more `.rs` files inside `native/`.

When MatchBox detects the `native/` directory, it compiles the Rust files together with the VM and links everything into the final binary.

### Writing a Native BIF

Each file in `native/` must expose a `register_bifs` function that returns a map of function names to implementations:

```rust
// native/math.rs
use matchbox_vm::types::{BxValue, BxVM, BxNativeFunction};
use std::collections::HashMap;

/// Compute the factorial of a non-negative integer.
pub fn factorial(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let n = args
        .first()
        .map(|v| v.as_number())
        .ok_or("factorial: expected a number argument")? as u64;

    let result = (1..=n).product::<u64>();
    Ok(BxValue::new_number(result as f64))
}

pub fn register_bifs() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("factorial".to_string(), factorial as BxNativeFunction);
    map
}
```

Call it from BoxLang like any other BIF:

```boxlang
println(factorial(10))   // 3628800
```

### Build

```bash
matchbox --target native app.bxs
```

MatchBox automatically detects `native/`, compiles the Rust code, merges the BIF registrations, and produces the final binary. No extra tooling needed.

---

## Experimental Java Interop (JNI)

In native builds only, MatchBox includes an experimental JNI bridge that lets you instantiate Java classes and call methods — provided a compatible JVM is installed on the host machine at runtime.

> **This feature is highly experimental.** APIs may change and stability is not guaranteed.

```boxlang
// Instantiate a Java class via JNI
sb = java.new("java.lang.StringBuilder", "Hello")
sb.append(", World!")
println(sb.toString())    // Hello, World!
```

This is not available in WASM builds.
