# C3 scoped instruction outcome and prediction reconciliation

**Status:** separately authorized fixed nine-process counter phase complete. Frozen C3 uses **23.1903% fewer scoped retired user instructions** than fresh B1, closing **28.2358% of the remaining Node gap** on the unchanged direct million-iteration checksum workload. This is an instruction result, not a new timing result or a universal-workload claim.

## Scope and provenance

The user approved closing C3 counts followed by a bounded read-only global-cache ownership/pricing audit. Instruction phase: 2026-10-31T03:45:27Z, at most20minutes, deadline04:05:27Z. Exactly three fresh processes per runtime, sequential B1/C3/Node, 30-second process timeout, no warmup/retries/outlier removal. Workloads completed **03:57:10.519446Z**. No implementation change, build, timing rerun, profiler/debugger session, install, CPU/security-setting change, commit/merge/push/fetch/reset or cleanup.

Existing identified executables, not rebuilt from the current checkout:

| Runtime | SHA-256 | Original source identity |
|---|---|---|
| B1 | `9af288307e2705365708940eec170e790c33c5ff94dd88c0e65829bd6cb14741` | B1 patch `8f988109…` on `582473a`; committed `57b6f18` |
| C3 | `72585b4106c8876b0ed45c3dff1f2948ed1549c016698069c976b9a567f60f3a` | C3 patch `52ea408f…` on `4957e5e`; committed `ecc9074` |
| Node v22.23.3 | `fde6a4bf8d0562f7751d1a2d6cb9b417c4cfe107bbcb0aa3e9a24e125e348f48` | Native `--jitless`, unchanged previously version-probed executable |

Both MatchBox harnesses: original offline Rust1.98.1/LLVM22.1.8/default-feature release builds, `opt-level=z`, LTO, one codegen unit, panic abort, `RUSTFLAGS=-C strip=none`; real parser/compiler/tree-shaken prelude, assertions that JIT is inactive before/after execution. See original build packets for commands/image/input hashes.

Main remains local `develop` at `a99b926`, investigation at `7882dfa`, originally clean/equal trees. **Unlike the earlier finalization, origin has now advanced to `c2b6ec8`, three incoming server commits ahead of main.** No refs/history were changed here. The frozen C3 executable is not a claim about a newly built incoming server revision. The subsequent audit must consider those already-fetched lifecycle changes read-only.

## Measurement boundary and correctness

Original immutable absolute inputs in `baseline-20261030T220339Z/`: `count.bxs`, `count.js`, `expected-count.txt`, and existing `markers.so` (SHA-256 `659262daf29f597ce11fe22121888b2e56f21344c260b858c9dee2ec864dcc8a`). Same exact direct million iterations and arithmetic, no wrapper/storage relocation or loop recognition. MatchBox `sum` is a VM global; Node CommonJS `sum`/`i` are wrapper locals. That comparator difference remains.

Priming and `sum` initialization precede START; loop declaration/control/body are within the region; checksum/output follow END. No warmup. All nine processes exited zero and produced exactly:

```text
PRIMED
__ICOUNT_START__
__ICOUNT_END__
500001500000
```

Native Linux x86-64 retired **user-space** CPU instructions, all pre-existing threads attached individually with inheritance for subsequent threads, kernel/hypervisor excluded. MatchBox: one counted thread/sample; Node: seven. All **27 per-thread counters** had `time_running_ns == time_enabled_ns > 0`, no scaling or simulated fallback. Raw signal/marker/periodic/GC work remains; no empty-loop/overhead subtraction. Controller whole-process durations are diagnostic metadata, not uninstrumented timings. Evaluator tests: **13 passed, five existing opt-in hardware tests skipped**; no extra live workload tests.

## Raw outcome

| Runtime | Raw aggregate instructions | Median | MAD | Batch median / 1,000,000 |
|---|---|---:|---:|---:|
| B1 | 1,422,993,464; 1,422,993,565; 1,422,993,568 | **1,422,993,565** | 3 | 1422.993565 |
| C3 | 1,092,997,768; 1,092,997,730; 1,092,997,677 | **1,092,997,730** | 38 | 1092.997730 |
| Node `--jitless` | 254,277,191; 254,277,354; 254,276,769 | **254,277,191** | 163 | 254.277191 |

Batch normalization is not a measurement of an isolated ordinary iteration. Node main-thread counts separately: 253,878,601; 253,879,198; 253,859,808; median253,878,601 / MAD597. The primary comparison uses aggregate threads, not Node-main-only.

- B1 → C3: **329,995,835 fewer instructions / 23.1902549%** of fresh B1.
- Remaining Node gap: 1,168,716,374 → **838,720,539**, **28.2357501%** closed.
- Aggregate MatchBox/Node ratio: **5.59623× → 4.29845×**.
- The earlier separate uninstrumented B1/C3 timing result (92→73ms) remains in [report20](20-c3-timing-result.md); it was not rerun or causally inferred from these counts.

## Frozen predictions versus observation

Original P1 SHA-256 `4c2386e9a2af32f73da6da6084c3b456d02405286c0229881c7f5f27d147dde0`, frozen01:04:18.110237Z. Compiled P2 SHA-256 `6d6174849a726cd76f52decbdd19a027021fd50ee8ccf75a5759b65d02c5eea7`, frozen01:56:54.341289Z. Both predate timing and these counters; neither has been rewritten.

| Quantity | Frozen prediction | Observed | Difference / status |
|---|---:|---:|---|
| P1 conditional million-cycle saving | 218,000,000–297,000,000 | 329,995,835 | 32,995,835 beyond P1 best case; planning range was not a guarantee |
| P2 conditional million-cycle saving | 330,000,000 | 329,995,835 | **4,165 less saving** (0.0012621% of prediction), signed/unassigned |
| B1 ordinary-cycle model | 1423 ×1,000,000 | 1,422,993,565 | batch residual **−6,435** |
| C3 ordinary-cycle model | 1093 ×1,000,000 | 1,092,997,730 | batch residual **−2,270** |

P2's compiled reconciliation remains **383 gross +22 additional audited disjoint old work −75 actual introduced work −0 net ordinary surrounding growth =330/cycle**. Its 33/cycle improvement over P1's best saving was priced before outcomes: additional22 credit plus11 cheaper net replacement. This explains the compiled revision, not the remaining measured4165 difference.

The batch-residual change is **+4,165**, which algebraically accounts for the saving shortfall; that is **not a causal explanation**. Cold/first-hit/periodic/backedge/boundary/final work and other model mismatch remain signed unknowns. The old measured B1 residual−6662 is neither inherited nor subtracted. Likewise, close aggregate agreement does not prove every conditional branch frequency or remove B1's separate32/cycle causal residual. No attribution to GC/noise, trace or forced frequency explanation is made.

## Decision, remaining risk and evidence

This closes the missing C3 hardware-counter check and supports the existing fixed-workload mechanism/outcome agreement. C3 was already retained/merged by explicit prior approval; this phase makes no product change. Noneligible-site taxes remain real: selected float-local declines add45–65 body instructions/access; warm standalone Number-CONSTANT adds one. Broader workloads/targets and incoming server builds are unmeasured.

Raw local evidence is ignored and **not transferred by committing this document**:

`/home/jacob/dev/ortus-boxlang/matchbox-checksum-baseline-20261030T2203Z/benchmarks/instructions/results/c3-instructions-20261031T034527Z/`

Key files: `plan.md`, `provenance.json`, frozen P1/P2 copies, `commands.json`, `count-{B1,C3,Node}.json/.log`, evaluator log, `measurement.log/.exit`, `summary.json`, source inventory, final verification and `files-sha256.json`. Eleven prior packets (701 listed files), binaries, original inputs and committed product files were reverified. New packet writes stop at sealing. No authority for implementation, a new build/measurement/timing/optimization loop, commit/merge/push or cleanup is created by this report. The separately approved next step is the bounded **read-only** cache audit.
