# What is MatchBox?

MatchBox is a **native Rust implementation** of the [BoxLang](https://github.com/ortus-boxlang/BoxLang) programming language. It lets you write BoxLang scripts (`.bxs`) and run them as blazing-fast, standalone applications — with no JVM, no Java installation, and no runtime overhead.

## The Problem it Solves

The main BoxLang runtime runs on the JVM, which brings excellent ecosystem access and dynamic power, but also brings weight: a JVM on the host machine, startup latency, and memory overhead. MatchBox is designed for scenarios where none of that is acceptable:

- **Edge deployments** where cold-start time and binary size are critical.
- **CLI tools** that need to ship as a single, zero-dependency binary.
- **Browser apps** where you want to run BoxLang logic natively via WebAssembly.
- **Embedded systems or containers** with minimal compute resources.

## How it Works

MatchBox is a complete, self-contained language toolchain:

1. **Parser** — A [Pest](https://pest.rs/) PEG grammar reads `.bxs` source files and produces an AST.
2. **Compiler** — A multi-stage compiler lowers the AST into a compact bytecode format.
3. **VM** — A stack-based Bytecode Virtual Machine executes the bytecode with a cooperative fiber scheduler for high-concurrency async operations.
4. **Targets** — The same bytecode can be embedded in a native binary, run inside a WASM runtime, or executed directly by the MatchBox runner.

## The MatchBox Philosophy

| Property | MatchBox | BoxLang (JVM) |
| :--- | :--- | :--- |
| Runtime dependency | None | JVM required |
| Startup time | Milliseconds | Seconds |
| Binary size | ~500 KB | Full JVM stack |
| Java interop | Experimental (JNI) | Full |
| WASM/browser support | ✅ | ❌ |
| Strict subset of BoxLang | ✅ | Full language |

## One-Way Compatibility

MatchBox implements a **strict subset** of the BoxLang language. Code written for MatchBox will generally run unmodified on the main BoxLang JVM runtime. Code written for the JVM runtime may use features not yet present in MatchBox.

Think of MatchBox as a "portable and deployable" profile of BoxLang — you target MatchBox when distribution, performance, or JVM-independence is a requirement.

## What You Can Build

- **CLI tools** — compile to a single native binary and distribute without any installer.
- **Serverless functions** — ultra-small cold start, ideal for AWS Lambda, Cloudflare Workers, or Fastly Compute.
- **Browser applications** — run BoxLang logic directly in the browser via a WASM module.
- **Native Fusion apps** — mix BoxLang with hand-written Rust for peak performance.

---

**Next:** [Installation →](installation.md)
