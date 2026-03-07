# Installing MatchBox

## Prerequisites

MatchBox itself has no runtime dependencies — that is the point. However, to **build** it from source you need the Rust toolchain.

| Requirement | Version | Purpose |
| :--- | :--- | :--- |
| [Rust](https://rustup.rs/) | 1.85+ (2024 edition) | Build from source |
| `wasm-bindgen-cli` | latest | WASM output targets |
| A C linker (`cc`) | system default | Native binary linking |

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the prompts, then restart your shell or run `source $HOME/.cargo/env`.

---

## Option 1: Build from Source

Clone the repository and build a release binary:

```bash
git clone https://github.com/your-org/matchbox.git
cd matchbox
cargo build --release
```

The compiled binary will be at `target/release/matchbox`. Add it to your `PATH`:

```bash
# macOS / Linux
export PATH="$PATH:$(pwd)/target/release"

# Or copy it to a directory already on your PATH
cp target/release/matchbox /usr/local/bin/matchbox
```

Verify the installation:

```bash
matchbox --version
```

---

## Option 2: Pre-built Binaries

Pre-built binaries for macOS (arm64 / x86_64), Linux (x86_64), and Windows (x86_64) are published as GitHub Release assets on every tagged release.

1. Go to the [Releases page](https://github.com/your-org/matchbox/releases).
2. Download the archive for your platform.
3. Extract and move the `matchbox` binary to a directory on your `PATH`.

```bash
# Example: macOS arm64
curl -LO https://github.com/your-org/matchbox/releases/latest/download/matchbox-macos-arm64.tar.gz
tar -xf matchbox-macos-arm64.tar.gz
chmod +x matchbox
mv matchbox /usr/local/bin/matchbox
```

---

## Installing the WASM Toolchain (optional)

Only required if you intend to compile BoxLang to WebAssembly (`--target wasm` or `--target js`).

```bash
# Add the WASM target to your Rust installation
rustup target add wasm32-unknown-unknown

# Install wasm-bindgen-cli (must match the version in Cargo.toml)
cargo install wasm-bindgen-cli
```

Build the WASM runtime:

```bash
cargo build --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir ./pkg \
    target/wasm32-unknown-unknown/release/matchbox.wasm
```

This produces the `pkg/` directory containing `matchbox.js` and `matchbox_bg.wasm` — the files you ship to the browser.

---

## Verifying Your Installation

Run the built-in REPL to confirm everything is working:

```bash
matchbox
```

You should see a prompt. Type a BoxLang expression and press Enter:

```
> println("Hello from MatchBox!")
Hello from MatchBox!
```

Press `Ctrl+D` or type `exit` to quit.

---

**Next:** [Building Your First App →](building.md)
