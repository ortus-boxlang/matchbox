# Investigation brief

## Question

Why does the retained MatchBox interpreter use about **2,173 retired user-space machine instructions per iteration**, versus about **254** for Node --jitless, when the traced repeated MatchBox loop uses seven VM dispatches and the diagnostic V8 sequence uses roughly twelve bytecodes?

Account for the work before proposing a fix. Fewer bytecodes do not imply cheaper handlers or cheaper dispatch.

## Four agreed areas

1. Why do known stack-local accesses traverse general scope-aware machinery?
2. How much work sits behind generic arithmetic and constant access?
3. What do cached global accesses and per-dispatch runtime checks cost?
4. Reconcile the bytecode-to-machine-work discrepancy using the first three analyses, plus other measured work, compiler effects and remaining unknowns.

## Fixed reference

- Source: `139be94a18d7d6bda04078bf192485cf3f67e8e9`.
- Binary: `target/build/release/examples/instruction_interpreter`.
- SHA-256: `5787b8a92026d52d97ce97d18cf552d06fbfc8d324493bcd661b94aa8e220557`.
- Native x86_64 Linux, AMD Ryzen 7 PRO 6850U.
- Real parser/compiler and tree-shaken prelude; default features retained. JIT capability compiled in, inactive by harness assertions.
- Release build: Rust 1.98.1, opt-level=z, LTO, one codegen unit, panic=abort, stripped.
- Comparator: Node v22.23.3 --jitless, native host, standard CommonJS script execution.
- Main workload: [workloads/loop.bxs](workloads/loop.bxs) / [workloads/loop.js](workloads/loop.js), 1,000,000 iterations, checksum **500001500000**.
- Direct body, no per-iteration wrapper call. Control sizes: 0 and 100,000. Three fresh processes each; no warmup.

## What the existing evidence establishes

At one million iterations, all-thread instruction medians: MatchBox 2,172,993,829; Node 254,269,661 (8.546x). Separate uninstrumented loop timings: 142 ms and 14 ms (10.14x, provisional due to millisecond resolution and short batches).

The arithmetic includes i+2 and sum+term, variable accesses, loop control and runtime bookkeeping. These are not isolated ADD costs. Marker counts exclude startup/compilation; setup cannot explain the marginal gap. Node's instruction slope is not perfectly linear across sizes. This study is not a universal runtime comparison or an opcode cost profile.

## Boundaries and safety

- Do not change VM/compiler/GC/JIT/types, build profiles, dependencies, workloads, expected checksums or evaluator to improve a score.
- No new autoresearch initialization, campaign resets, manual experiment commits or profile tuning. Do not edit `.auto/prompt.md`, frozen campaign inputs, or `reference/`.
- Preserve reflection, aliases, scope deletion, argument forms, closures, events, coercion, GC and error behavior when discussing designs. Consult canonical JVM code before asserting semantic equivalence.
- Read-only inspection first. Coordinator alone schedules measurements, sequentially and with explicit process timeouts. Preserve raw failures; no counter fallback or security-setting changes.
- Diagnostic builds are not accepted substitutes for the retained binary without provenance and validation. A profile of a changed binary is labeled as such.
- Native sampling shares do not automatically equal retired-instruction shares. Never multiply CPU sample percentages by totals and present them as exact instructions.

## Deliverable format for each analysis

1. Exact source paths/functions and commit checked.
2. Actual hot-path operations and source-derived execution frequency.
3. Assembly/profile evidence, or an explicit missing-evidence marker.
4. Semantic purpose of each major operation.
5. Candidate explanations with confidence and falsifying observations.
6. Proposed minimal diagnostics for coordinator review; no product edits.
7. Cross-area costs to avoid counting twice.

A handoff is not complete merely because it contains plausible overheads. It must distinguish **measured**, **source-confirmed**, **inferred**, and **unresolved** findings.
