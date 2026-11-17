---
name: benchmark-run-suite
description: Run and interpret the MatchBox cross-runtime performance suite against Node.js and JVM BoxLang, with verified results, reproducible reports, and explicit timing boundaries. Use when asked to benchmark MatchBox, run the runtime suite, or compare these engines.
---

# Run the cross-runtime benchmark suite

This skill runs a standalone evaluator. It does **not** authorize performance
implementation changes or an autoresearch campaign.

## Read first

Resolve these paths relative to this skill directory:

- `../../..` is the repository root.
- `../../../benchmarks/runtime/README.md` documents requirements, commands,
  measurement boundaries and portability lessons. Read it completely.
- `../../../benchmarks/runtime/manifest.json` lists cases and pinned sources.
- `../matchbox-autoresearch/SKILL.md` applies only if campaign preparation or
  optimization is requested separately; load it before that work.

## Procedure

1. Inspect Git status and preserve existing changes. Identify requested cases,
   runtimes, sample count, iteration counts, and wall-time budget. Default to the
   documented suite settings when no overrides are requested, but explain that
   slow cases may need minutes. Do not silently expand into a long sweep.
2. Preflight Python 3.10+ and executable locations. Use `--list` to inspect cases.
   Default engine selection is MatchBox, Node.js and JVM BoxLang. If a requested
   engine is missing, stop and explain how to supply its executable; don't silently
   omit it or install/change global toolchains. An explicitly requested subset is
   valid but must be labeled partial.
3. Build MatchBox **before** measurement (`cargo build --release --locked`) when
   a current-source build is needed and the toolchain is available. Never select
   an unrelated old executable just because it exists. If Rust is unavailable,
   agree on a prebuilt artifact or isolated builder and record its revision,
   toolchain, profile and features. Run all measured engines in comparable host
   environments; don't time one in an emulator/container and the others natively
   without prominently qualifying that comparison.
4. Read the manifest and run the runner tests:

   ```bash
   python3 -m unittest discover -s benchmarks/runtime -p 'test_*.py' -v
   ```

5. Run from the repository root, adapting only the requested flags:

   ```bash
   python3 benchmarks/runtime/run.py --benchmarks sieve \
     --samples 3 --warmup 100 --iterations 1000 --timeout 120 \
     --output .auto/artifacts/sieve-baseline.json \
     --note 'Build revision/toolchain/profile/features and runtime options'
   ```

   Omit `--benchmarks` to run all registered cases. Use executable-path options
   or `MATCHBOX_BIN`, `NODE_BIN`, `BOXLANG_BIN` as documented. Avoid overwriting an
   earlier baseline: choose a unique output filename or the timestamped default.
   Run engines sequentially, not concurrently. Do not alter manifests, ported
   workloads, expected results, GC behavior, build profiles, or runtime algorithms
   merely to improve the numbers.
6. Require exit success, JSON `status: complete`, every requested engine/case,
   the expected sample counts and verified outputs. A timeout, wrong answer,
   missing engine, zero duration, or changed evaluator is a failure, not a score.
   Preserve failed reports and report the cause. Respect cancellation and time
   budgets instead of automatically retrying larger batches indefinitely.
7. Inspect raw samples, MAD and short-batch warnings. Millisecond timing cannot
   reliably establish small deltas from a 20ms batch. Propose larger iterations
   and a new explicit timeout/budget if needed; don't quietly change settings
   between baseline and candidate. Warmup count alone does not establish JIT
   steady state. Identify noisy, short or incomplete results as provisional.
8. Summarize case, input size, versions/build, warmup, measured iterations,
   samples, batch median/MAD, per-invocation time, whole-process median, and report
   path. State that workload time includes allocation/verification and process
   time includes startup/source processing/warmup. Fresh processes may still use
   filesystem/JVM compiled-template caches. Do not call process time compiler
   time, and don't manufacture peak-memory or GC-pause measurements.

## Comparing results

- MatchBox's own approved baseline is the useful before/after comparison.
  Node.js/JVM ratios are external context, not a score to game.
- Use equivalent source algorithms, sizes, repetition policies and hardware.
  Normalize by iterations only when policies are explicit and comparable.
- Sieve is currently the only case; it is not a general-purpose runtime score.
  Do not claim confidence intervals or universal speedups from three samples.
- Preserve JSON evidence locally; ignore rules keep generated results out of
  source control. Sharing/committing reports is a separate explicit decision.
- No `/autoresearch` activation, automatic commits, pushes, or implementation
  optimization is part of this skill.
