# MatchBox Agent Instructions

## What is MatchBox

MatchBox is a custom VM for BoxLang, a spiritual successor to CFML. It aims for compatibility with the BoxLang JVM reference implementation while targeting environments where the JVM struggles: WebAssembly, ESP32, cloud functions, and native binaries.

The VM includes tiered JIT compilation (Cranelift backend) for hot code paths, NaN-boxed values for efficient memory use, and generational garbage collection.

## Commands

```bash
cargo build                    # Build
cargo test                     # Run all tests
cargo test vm_array_bifs       # Run specific test
cargo test --test boxlang_compat_tests  # Run the BoxLang compat transfer suite
cargo clippy                   # Lint
cargo build --release --features "bif-http,bif-zip"  # Verify release build
```

## Performance benchmarks

For standalone runtime comparisons, load `.pi/skills/benchmark-run-suite/SKILL.md`. For adding or porting workloads, load `.pi/skills/benchmark-add-test/SKILL.md`. The durable cross-runtime suite lives in `benchmarks/runtime/` and does not require Pi. For executed machine-instruction counts of a script (whole-process or scoped after warmup), load `.pi/skills/measure-instructions/SKILL.md`; its standalone tool lives in `benchmarks/instructions/`. Running or adding a benchmark does not authorize implementation optimization or an autoresearch loop.

## Autoresearch

Before preparing or running automated experiments, load `.pi/skills/matchbox-autoresearch/SKILL.md` and follow its MatchBox-specific constraints alongside the upstream skills. Setup alone does not authorize starting a loop. See `.pi/README.md` for installation and operation.

## Project Structure

- `crates/matchbox-vm/src/bifs/mod.rs` — Native Rust BIFs
- `crates/matchbox-compiler/src/prelude.bxs` — Prelude BIFs (BoxLang)
- `tests/scripts/` — Integration test scripts (.bxs)
- `tests/integration_tests.rs` — Test registration
- `reference/boxlang/` — BoxLang JVM reference (read-only)
- `BIF_STATUS.md` — BIF implementation tracking

## Adding BIFs

Load the `add-bif` skill. It covers:
- Prelude vs native decision tree
- Implementation patterns
- Testing workflow
- Registration steps

## BoxLang Reference

Load the `reference-boxlang` skill. BoxLang JVM defines canonical behavior. Always check it first.


## Engineering Principles

- Do not preserve backward compatibility. Remove obsolete paths instead of adding compatibility layers, fallbacks, or migrations.
- Choose the simplest implementation that fully meets the current requirements. Avoid speculative abstractions, configuration, and indirection.
- Grow the system in layers. Start from the smallest version that works end to end, and add each new capability on top of a product that already works. Never trade a working product for unfinished complexity.
- Keep components modular and concerns clearly separated.
- Prefer established, well-maintained libraries when they reduce overall complexity or improve reliability. Do not reimplement common functionality without a clear reason.
- Lean on the dependencies already in the project before writing your own implementation or adding packages. Do not assume a library lacks a capability without checking its documentation and types.
- Make architectural decisions for the long term. Do not accept a stopgap that only works for now and is meant to be replaced later.

## Rust Documentation

Every Rust item you add or change gets rustdoc (`///`), including private functions, methods, struct fields and enum variants. Write it for the next reader, in plain sentences.

- Start with one line that says what the item does or is. Add detail only when the behavior is not obvious from the name and types.
- A function that returns `Result` or `Option` documents when it fails or returns `None` in a `# Errors` section (or a sentence for `Option`).
- A function that can panic documents it in `# Panics`. An `unsafe` function documents its contract in `# Safety`.
- Add a doctest (`# Examples`) to pure helpers such as parsers and formatters. Skip it for code that needs a VM, the file system or the network.
- Module and crate docs (`//!`) say what the module is for and how it fits with the others.
- Link to other items with ``[`Item`]``. Use ``[`fn@name`]`` when a function shares its name with a macro.
- Do not repeat the code in prose, and do not leave stale names or paths in a doc after a rename.

`matchbox_utility` enforces this with `#![warn(missing_docs)]`. Check a crate with:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc -p matchbox_utility --no-deps --document-private-items
cargo test -p matchbox_utility --doc
```

The other crates are not enforced yet. Document what you touch in them, and do not add `#![warn(missing_docs)]` to a crate in an unrelated change.
