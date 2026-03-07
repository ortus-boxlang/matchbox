# WASM Container Deployment

MatchBox WASM binaries can be run as standalone server-side workloads using WASM-native runtimes like [Wasmtime](https://wasmtime.dev/), [WasmEdge](https://wasmedge.org/), or inside minimal OCI containers using the WASM OCI image spec.

This deployment model is ideal for:

- **Serverless / FaaS** — minimal cold-start, no OS dependencies.
- **Edge computing** — run BoxLang logic at the network edge (Fastly Compute, Cloudflare Workers via WASI).
- **Microservices** — tiny, hermetically isolated workloads without a full OS stack.

---

## Compiling for WASM/WASI

Use `--target wasm` to produce a raw `.wasm` binary:

```bash
matchbox --target wasm my_service.bxs
# Produces: my_service.wasm
```

The resulting binary contains the MatchBox VM core and your compiled BoxLang bytecode in a custom WASM section. It has no external JS dependencies and can be loaded by any WASM runtime that supports the WASI preview1 interface.

---

## Running with Wasmtime

[Wasmtime](https://wasmtime.dev/) is the reference WASI runtime. Install it and run your binary directly:

```bash
# Install Wasmtime (macOS / Linux)
curl https://wasmtime.dev/install.sh -sSf | bash

# Run your app
wasmtime my_service.wasm
```

Grant filesystem access if your app reads or writes files:

```bash
wasmtime --dir=. my_service.wasm
```

Grant network access (Wasmtime 14+):

```bash
wasmtime --wasi-modules=experimental-wasi-sockets my_service.wasm
```

---

## Running with WasmEdge

[WasmEdge](https://wasmedge.org/) is optimized for cloud-native and microservice workloads:

```bash
# Install WasmEdge
curl -sSf https://raw.githubusercontent.com/WasmEdge/WasmEdge/master/utils/install.sh | bash

# Run
wasmedge my_service.wasm
```

---

## Docker / OCI Container

MatchBox WASM binaries can be run inside minimal OCI containers. Because the WASM runtime handles everything, the container image needs only the binary itself — no OS, no shell, no libc.

### Example Dockerfile

```dockerfile
FROM scratch

COPY my_service.wasm /my_service.wasm

ENTRYPOINT ["/my_service.wasm"]
```

Build and run with a WASM-capable container runtime:

```bash
docker build -t my-boxlang-service .
docker run --runtime=io.containerd.wasmtime.v1 my-boxlang-service
```

> **Note:** Standard Docker uses runc by default. To run WASM containers you need a WASM-compatible runtime shim such as [`containerd-shim-wasmtime`](https://github.com/containerd/runwasi) or Docker Desktop's built-in WASM support.

### Docker Desktop (Built-in WASM Support)

Docker Desktop 4.15+ includes native WASM support via `docker run`:

```bash
docker run --runtime=io.containerd.wasmtime.v1 \
    --platform=wasi/wasm32 \
    my-boxlang-service
```

### Pushing to a Registry

WASM OCI images can be pushed to any OCI-compatible registry:

```bash
docker push ghcr.io/your-org/my-boxlang-service:latest
```

---

## Fastly Compute

[Fastly Compute](https://www.fastly.com/products/edge-compute) runs WASI binaries at the network edge. Package your `.wasm` binary using the Fastly CLI:

```bash
fastly compute pack --wasm my_service.wasm
fastly compute deploy
```

---

## Cloudflare Workers

BoxLang scripts compiled with `--target js` can be deployed as Cloudflare Workers. The JS module wrapper is compatible with the Workers module format:

```bash
matchbox --target js handler.bxs
# Produces: handler.js + handler.wasm

# Deploy via Wrangler
wrangler deploy --main handler.js
```

Your `wrangler.toml` should include the WASM binding:

```toml
[[wasm_modules]]
name = "HANDLER_WASM"
path = "handler.wasm"
```

---

## Limitations

| Feature | WASM Container | Notes |
| :--- | :--- | :--- |
| Java interop | ❌ Not supported | No JNI in WASM |
| Native Fusion | ❌ Not supported | Rust interop is native-only |
| DOM / `js.*` APIs | ❌ Not available | JS interop requires a browser context |
| Filesystem access | ✅ Via WASI | Requires runtime `--dir` grants |
| Network access | ⚠️ Experimental | WASI sockets are in preview |
| `sleep()` / async | ✅ | Cooperative fiber scheduler works in WASI |
