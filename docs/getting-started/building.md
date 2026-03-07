# Building Your First App

This guide walks you through the four main ways to run BoxLang with MatchBox, from the quickest (interpreter) to the most distributable (compiled binary).

---

## Quick Start — Interpreter Mode

The fastest way to get started is to run a `.bxs` file directly. MatchBox will parse, compile, and execute it in one step.

Create a file called `hello.bxs`:

```boxlang
name = "World"
println("Hello, #name#!")
```

Run it:

```bash
matchbox hello.bxs
```

```
Hello, World!
```

No build step, no configuration — just run.

---

## Interactive REPL

Start the REPL by running `matchbox` with no arguments:

```bash
matchbox
```

Each line you type is compiled and executed immediately, making it great for experimenting with the language:

```
> x = 10
> y = 20
> println(x + y)
30
> greet = (name) => "Hello, " & name & "!"
> println(greet("BoxLang"))
Hello, BoxLang!
```

---

## Compile to Portable Bytecode

For cases where you want to pre-compile a script without tying it to a specific OS or architecture, use `--build`. This produces a `.bxb` bytecode file that any MatchBox runner can execute.

```bash
matchbox --build hello.bxs
# Produces: hello.bxb
```

Distribute `hello.bxb` and run it on any machine that has MatchBox installed:

```bash
matchbox hello.bxb
```

Bytecode files are compact and load faster than source, because the parsing and compilation step is already done.

---

## Compile to a Standalone Native Binary

The flagship feature of MatchBox. Produce a single self-contained binary with no external dependencies — not even MatchBox itself needs to be installed on the target machine.

```bash
matchbox --target native hello.bxs
# Produces: ./hello  (or hello.exe on Windows)
```

Run it directly:

```bash
./hello
Hello, World!
```

The resulting binary is typically around **500 KB** because MatchBox uses an ultra-lean runner stub architecture — only the VM core and your compiled bytecode are bundled in, with all debug symbols and dead code stripped.

See [Native Builds](../building-and-deploying/native-builds.md) for the full reference.

---

## Compile to WebAssembly / JavaScript

Compile your BoxLang app to run in browsers or Node.js:

```bash
# JavaScript ES module wrapping a WASM binary
matchbox --target js hello.bxs
# Produces: hello.js + hello.wasm

# Raw WASM binary (no JS wrapper)
matchbox --target wasm hello.bxs
# Produces: hello.wasm
```

See [JavaScript & WASM](../building-and-deploying/javascript-and-wasm.md) for integration details.

---

## CLI Reference

```
USAGE:
    matchbox [OPTIONS] [FILE]

ARGS:
    [FILE]    A .bxs source file or .bxb bytecode file to run

OPTIONS:
    --build              Compile to portable bytecode (.bxb), do not execute
    --target <TARGET>    Compile and bundle for a specific deployment target
                         Possible values: native, wasm, js
    --version            Print version info
    -h, --help           Print help
```

When `FILE` is omitted, MatchBox starts the interactive REPL.

---

**Next:** [Features Overview →](features-overview.md)
