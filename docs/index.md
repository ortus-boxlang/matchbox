# MatchBox Documentation

Welcome to the MatchBox documentation. MatchBox is a native Rust implementation of the BoxLang programming language with no JVM dependency, targeting native binaries, WebAssembly, and edge deployments.

---

## Getting Started

| | |
| :--- | :--- |
| [What is MatchBox?](getting-started/what-is-matchbox.md) | Architecture, goals, and how MatchBox compares to the JVM runtime. |
| [Installation](getting-started/installation.md) | Build from source or download a pre-built binary. |
| [Building MatchBox](getting-started/building-matchbox.md) | Clone, build custom variants, and configure build features. |
| [Building Your First App](getting-started/building.md) | Interpreter, REPL, bytecode, and compiled builds in five minutes. |
| [Features Overview](getting-started/features-overview.md) | A tour of the supported language features with examples. |

---

## Building & Deploying BoxLang Apps

| | |
| :--- | :--- |
| [Native Builds](building-and-deploying/native-builds.md) | Standalone OS binaries, cross-compilation, and Native Fusion (Rust interop). |
| [Docker Image](building-and-deploying/docker-image.md) | Use the GHCR image for CI builds, direct execution, and webroot serving. |
| [JavaScript + WASM](building-and-deploying/javascript-and-wasm.md) | ES modules, raw WASM, and runtime (dynamic) execution in the browser. |
| [WASM Container](building-and-deploying/wasm-container.md) | Wasmtime, WasmEdge, Docker OCI containers, and edge platforms. |
| [ESP32 Firmware](building-and-deploying/esp32.md) | Cross-compiling and flashing for ESP32/S3/C3 microcontrollers. |
| [Web Server (Webroot)](examples/web_server/README.md) | Static files, `.bxm` templates, and automatic web scopes. |
| [App Server](examples/app_server/README.md) | Routed HTTP apps with `web.server()`, `event/rc/prc`, middleware, static asset mounts, sessions, templates, and webhooks. |
| [WebSocket Counter](examples/websocket_counter/README.md) | Routed app server plus SocketBox-style websocket listener classes and browser updates. |
| [JIT Compilation](building-and-deploying/jit.md) | How the four-tier JIT works, where it is active, and how to extend it. |

---

## Reference

| | |
| :--- | :--- |
| [Differences from BoxLang (JVM)](differences-from-boxlang.md) | What is and isn't supported compared to the main BoxLang runtime. |
| [Developer Guide](developer_guide.md) | Low-level architecture notes, tutorials, and CLI reference. |

---

## Quick Reference

### CLI

```bash
matchbox                          # Start REPL
matchbox my_script.bxs            # Run interpreter
matchbox --build my_script.bxs    # Compile to .bxb bytecode
matchbox --target native app.bxs  # Standalone native binary
matchbox --target js     lib.bxs  # ES module + .wasm
matchbox --target wasm   app.bxs  # Raw .wasm binary
matchbox --target esp32  app.bxs  # Build and flash ESP32
```

### Docker

```bash
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest my_script.bxs
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --target wasm app.bxs
docker run --rm -p 8080:8080 -v "$PWD/www:/app" ghcr.io/ortus-boxlang/matchbox:latest --serve --host 0.0.0.0 --webroot /app
```

### Language at a Glance

```boxlang
// Variables & string interpolation
name = "MatchBox"
println("Hello from #name#!")

// Functions with type hints and defaults
public string function greet(required string name, greeting = "Hello") {
    return greeting & ", " & name & "!"
}

// Arrow functions
double = (x) => x * 2

// Async
future = runAsync(() => { sleep(100); return 42 })
println(future.get())

// Classes
class Person accessors="true" {
    property name
    function greet() { return "Hi, I'm " & this.name }
}

p = new Person()
p.setName("Jacob")
println(p.greet())
```
