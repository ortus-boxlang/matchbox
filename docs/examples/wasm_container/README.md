# WASM Container Example

This example shows how to compile a BoxLang script to a raw WebAssembly binary
and run it as a serverless workload inside a minimal OCI container — with no OS,
no JVM, and no MatchBox installation on the host.

The same `.wasm` binary can also be executed directly by
[Wasmtime](https://wasmtime.dev/) or [WasmEdge](https://wasmedge.org/).

## Project Layout

```
wasm_container/
├── service.bxs  ← BoxLang source
└── Dockerfile   ← minimal OCI image (FROM scratch)
```

## Step 1 — Prerequisites

No special prerequisites are needed. `matchbox --target wasi` uses a pre-built
WASI runner stub and produces a `.wasm` binary in under a second with no `cargo`
or Rust toolchain required at compile time.

To run the binary outside of Docker you need a WASI runtime such as Wasmtime:

```bash
# Install Wasmtime (macOS / Linux) — optional, for running without Docker
curl https://wasmtime.dev/install.sh -sSf | bash
```

## Step 2 — Compile to WASM

```bash
cd docs/examples/wasm_container
matchbox --target wasi service.bxs
```

> **`--target wasi` and `--target wasm`** are the same. Both patch the bytecode into the
> pre-built `wasm32-wasip1` runner stub, so no `cargo` invocation is needed and
> compilation completes in under a second. For the browser use `--target js`.

This produces `service.wasm` — a self-contained WASI binary embedding the
MatchBox VM and your compiled BoxLang bytecode.

## Step 3 — Run with Wasmtime (no Docker required)

If you installed Wasmtime, you can run the binary directly and see output in your
terminal immediately — no Docker overhead:

```bash
wasmtime service.wasm
```

Expected output:

```
=== BoxLang WASM Service ===

--- Dataset Summary ---
Values : 10
Sum    : 417
Mean   : 41.7
Min    : 3
Max    : 97
Range  : 94

--- Fibonacci (first 10 terms) ---
  term 1: 0
  term 2: 1
  ...
  term 10: 34

Service completed successfully.
```

Grant filesystem access if your script reads files:

```bash
wasmtime --dir=. service.wasm
```

## Step 4 — Run with WasmEdge

```bash
# Install WasmEdge
curl -sSf https://raw.githubusercontent.com/WasmEdge/WasmEdge/master/utils/install.sh | bash

# Run
wasmedge service.wasm
```

## Step 5 — Build a Docker Image

```bash
# Compile first (produces service.wasm via a full wasm32-wasip1 cargo build)
matchbox --target wasi service.bxs

# Build the OCI image, annotating it for the wasi/wasm32 platform.
# This must match the --platform flag used at run time.
docker buildx build --platform wasi/wasm32 -t matchbox-service .
```

The image is built `FROM scratch` — it contains only `service.wasm`. Total image
size is the WASM file itself (~500 KB).

> **Why `docker buildx build --platform wasi/wasm32`?**  
> Plain `docker build` tags the image for your host's native platform
> (e.g. `linux/arm64`). When `docker run --platform=wasi/wasm32` is given,
> Docker looks for an image with a `wasi/wasm32` manifest, finds none locally,
> and tries to pull from Docker Hub — causing a "repository does not exist" error.
> Building with `--platform wasi/wasm32` ensures the manifest matches.

## Step 6 — Run the Docker Container

The `containerd-shim-wasmtime` always routes the WASM process's stdout through
Docker's log driver. It does not stream output to an attached terminal, regardless
of flags. The reliable pattern is to run detached and read logs after the
container exits:

```bash
docker run -d --name matchbox-svc \
           --runtime=io.containerd.wasmtime.v1 \
           --platform=wasi/wasm32 \
           matchbox-service

docker logs matchbox-svc
docker rm matchbox-svc
```

> **Tip:** For live terminal output during development, skip Docker and use
> `wasmtime service.wasm` directly — it is simpler and faster.

## Push to a Registry

```bash
docker tag matchbox-service ghcr.io/your-org/matchbox-service:latest
docker push ghcr.io/your-org/matchbox-service:latest
```

## Deploy to Fastly Compute

```bash
fastly compute pack --wasm service.wasm
fastly compute deploy
```

## What WASM Containers Are Good For

| Use case | Why WASM containers fit |
|---|---|
| Serverless / FaaS | Near-zero cold start, no OS dependencies |
| Edge computing | Run BoxLang logic at the network edge |
| Hermetic microservices | Strict sandbox, nothing outside the WASM spec |
| CI / batch jobs | Tiny image, fast pull, deterministic execution |

## Limitations

| Feature | Status |
|---|---|
| `println` / stdout | ✅ Works via WASI |
| Filesystem access | ✅ Requires `wasmtime --dir=` grant |
| Network (sockets) | ⚠️ WASI preview2 / experimental |
| DOM / `js.*` APIs | ❌ Browser context only |
| Java interop | ❌ No JNI in WASM |
| Native Fusion | ❌ Native builds only |

See [wasm-container.md](../../building-and-deploying/wasm-container.md) for the
full deployment reference.
