# Installing MatchBox

## Prerequisites

MatchBox itself has no runtime dependencies — that is the point. However, to **build** it from source you need the Rust toolchain.

| Requirement | Version | Purpose |
| :--- | :--- | :--- |
| [Rust](https://rustup.rs/) | 1.85+ (2024 edition) | Build from source |
| `wasm-bindgen-cli` | 0.2.114 | WASM output targets |
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

## Option 3: Docker Image

MatchBox images are published to GitHub Container Registry:

```bash
docker pull ghcr.io/ortus-boxlang/matchbox:latest
```

The image uses `matchbox` as its entrypoint and `/app` as the working directory. Mount your project into `/app` and pass normal MatchBox CLI arguments:

```bash
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --help
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest my_script.bxs
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --build my_script.bxs
```

Release tags:

| Tag | Use |
| :--- | :--- |
| `latest` | Latest stable release image |
| `vX.Y.Z` | Specific stable release image |
| `develop` | Rolling develop branch image |
| `snapshot` | Rolling snapshot image |
| `be` | Rolling BE/development image alias |

---

## Installing the WASM Toolchain (optional)

Only required if you intend to compile BoxLang to WebAssembly (`--target wasm` or `--target js`).

```bash
# Add the WASM target to your Rust installation
rustup target add wasm32-unknown-unknown

# Install wasm-bindgen-cli (must exactly match the workspace's wasm-bindgen crate)
cargo install wasm-bindgen-cli --version 0.2.114
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
