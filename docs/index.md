# MatchBox Documentation

Welcome to the MatchBox documentation. MatchBox is a native Rust implementation of the BoxLang programming language with no JVM dependency, targeting native binaries, WebAssembly, and edge deployments.

---

## Getting Started

| | |
| :--- | :--- |
| [What is MatchBox?](getting-started/what-is-matchbox.md) | Architecture, goals, and how MatchBox compares to the JVM runtime. |
| [Installation](getting-started/installation.md) | Build from source or download a pre-built binary. |
| [Building Your First App](getting-started/building.md) | Interpreter, REPL, bytecode, and compiled builds in five minutes. |
| [Features Overview](getting-started/features-overview.md) | A tour of the supported language features with examples. |

---

## Building & Deploying BoxLang Apps

| | |
| :--- | :--- |
| [Native Builds](building-and-deploying/native-builds.md) | Standalone OS binaries, cross-compilation, and Native Fusion (Rust interop). |
| [JavaScript + WASM](building-and-deploying/javascript-and-wasm.md) | ES modules, raw WASM, and runtime (dynamic) execution in the browser. |
| [WASM Container](building-and-deploying/wasm-container.md) | Wasmtime, WasmEdge, Docker OCI containers, and edge platforms. |
| [ESP32 Firmware](building-and-deploying/esp32.md) | Cross-compiling and flashing for ESP32/S3/C3 microcontrollers. |
| [Web Server](examples/web_server/README.md) | High-performance native web server with BXM template support. |
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
