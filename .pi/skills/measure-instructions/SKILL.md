---
name: measure-instructions
description: Count executed CPU machine instructions for a script on MatchBox, Node.js, or another native Linux runtime. Use for whole-process or post-warmup scoped instruction counts, Node --jitless comparisons, and investigating VM overhead independently of startup. Saves verified raw hardware-counter reports; does not optimize code or start autoresearch.
compatibility: Native Linux x86-64, Python 3.10+, perf_event_open permission; region mode additionally needs a compiled libc marker helper.
---

# Measure executed machine instructions

## Read first

Resolve paths relative to this skill directory:

- `../../../benchmarks/instructions/README.md` — read completely for commands,
  counting boundaries, helper build, limitations and reporting rules.
- `../../../benchmarks/instructions/run.py` — standalone evaluator; no Pi or
  third-party Python packages needed.
- Repository root is `../../..`. Run evaluator commands there unless explicitly
  choosing a different target working directory.

This workflow measures hardware **retired user-space instructions**, not generated
code size, VM bytecodes, cycles, or elapsed-time speedups. It does not authorize
runtime implementation changes, tuning, autoresearch, commits, or pushes.

## Procedure

1. Inspect Git status and preserve existing work. Identify the script, intended
   inputs/result, runtimes/flags, scope, samples, repetition policy and budget.
   Default to **three fresh-process samples, 30 seconds per process**, only the
   requested scripts/engines. Do not expand a small question into a long sweep.
2. Choose and state the boundary:
   - `--mode process`: unchanged script, executable startup through exit. Include
     an empty-file control for tiny programs. Never call this isolated execution
     or compiler cost.
   - `--mode region`: script copy/harness with exactly one START/END marker pair,
     setup/warmup before START and result verification/output after END. Preserve
     the user's original file; explain any wrapping or repetition. Do not repeat
     stateful/side-effectful code without establishing equivalent semantics.
3. Preflight native Linux x86-64, Python and each requested executable. Check
   `matchbox --version` / `node --version` and executable provenance. Build a
   current-source release executable **outside measurement** if needed and the
   toolchain is available. If not, agree on an identified prebuilt binary or
   isolated builder. Record embedded commit, profile/features if known, SHA-256,
   and caveats. Do not equate checkout revision with binary build revision.
   Never silently omit an unavailable requested engine, install toolchains, lower
   perf security settings, or replace hardware counts with estimates.
4. For region mode, build `markers.c` into an ignored artifact using an available
   host compiler or an agreed native builder. Measured runtimes must both run
   natively on the host. Follow the README's stdout/libc restrictions. Prime
   output before warmup. Use matching warmup and inputs; background compilation
   must be allowed to finish if claiming warmed execution. Inspect JIT traces in
   **separate unmeasured processes**, not the counted runs.
5. Run the evaluator tests, then execute engines sequentially:

   ```bash
   python3 -m unittest discover -s benchmarks/instructions -p 'test_*.py' -v
   python3 benchmarks/instructions/run.py \
     --mode process --samples 3 --timeout 30 \
     --input script.bxs --expect-last-line 10 \
     --runtime-version 'Actual version/build output' \
     --note 'Actual build provenance, profile/features and measurement policy' \
     -- target/release/matchbox script.bxs
   ```

   For scoped runs add `--mode region --marker-library PATH/markers.so` and use
   an explicitly marked harness. Hash every relevant source/module/harness with
   repeated `--input`. Record warmup, measured iterations, any engine-specific
   harness differences and unknown build details in `--note`. Runtime flags go
   after `--`; use **`node --jitless script.js`** for interpreter-only Node.
   `--no-opt` does not disable every JIT tier. Leave MatchBox configuration
   unchanged unless separately requested, and say so. `--jitless` is not a claim
   about Node's universal slowest possible configuration.
6. Require a zero evaluator exit, JSON `status: complete`, requested sample count,
   correct output and `time_running_ns == time_enabled_ns > 0` on every counter.
   Use `--expect-last-line` or exact `--expect-stdout-file`. For a silent script,
   independently verify its result before accepting empty expected stdout. A
   timeout, unavailable counter, missing marker or wrong answer is a **failure**,
   not a score. Keep failed reports and diagnostics. Reports are timestamped and
   ignored by default; named `--output` files must not already exist.
7. For small measured regions, measure the empty boundary and/or at least two
   batch sizes with the same warmup and source structure. Estimate marginal
   instructions/iteration as `(median_large - median_small) / (large - small)`.
   Label subtraction as approximate; negative/noisy/nonlinear increments are
   inconclusive. Prefer direct loop bodies over artificial per-iteration calls.
   Keep results observable, and distinguish constant-foldable versus varying
   input tests. Do not silently alter workload algorithms or optimize the runtime.
8. Report scope, CPU/OS, exact runtimes/flags/build caveats, input sizes, warmup,
   samples, raw batch medians/MAD and controls, any per-iteration normalization,
   correctness results and report paths. State whether comparing all-thread or
   main-thread counts. The tool's process total includes inherited threads;
   region mode additionally exposes a main-thread count. Include startup/marker,
   JIT and GC boundary caveats. Instruction-count ratios are **not** wall-time
   speedups; do not infer an opcode-level cause from counts alone.

## Keep evidence durable

The evaluator/helper/docs are versioned in `benchmarks/instructions/`; local
reports and built helpers belong in ignored `results/` or `.auto/artifacts/`.
Never depend on this session's ad hoc artifact directories for future runs.
Sharing or committing reports is a separate decision; review output/local paths
for sensitive data. Running this skill never activates an optimization loop.
