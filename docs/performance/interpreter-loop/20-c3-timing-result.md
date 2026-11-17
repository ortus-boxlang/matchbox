# C3: measured timing result and retention decision

## Result

After the [C3 implementation/native-validation phase](19-c3-implementation-validation.md), the user separately asked for current timings. A **timing-only** phase ran exactly nine fresh native workload processes: three each for the frozen B1 binary, C3 candidate and Node.js `--jitless`, serialized B1 → C3 → Node three times. No build, instruction counter, profiler/debugger session, tuning variant, larger workload or retry was run.

The unchanged direct loop executes **1,000,000 iterations**, with checksum **`500001500000`**. All nine processes exited zero with the exact expected output. There is no warmup, function wrapper, storage relocation, literal/loop-bound special case or changed arithmetic.

| Runtime | Raw loop milliseconds | Loop median / MAD | Whole-process median |
|---|---|---|---|
| B1, fresh control | 83, 94, 92 | **92 / 2 ms** | **96.812437 ms** |
| C3 candidate | 68, 81, 73 | **73 / 5 ms** | **77.722495 ms** |
| Node.js 22.23.3 `--jitless` | 12, 18, 15 | **15 / 3 ms** | **51.281235 ms** |

Against fresh B1, C3 uses **19 ms / 20.6521739% less loop time**, a **1.260274× speedup**. C3's loop median is **4.866667×** the fresh Node median. Whole-process elapsed time is reported separately; it is not isolated compiler timing.

These measurements occurred at **2026-10-31T02:14:56Z**, within the timing phase's 15-minute cap (02:12:40–02:27:40 UTC). Final preservation/sealing completed **02:16:17.353520Z**. Every sample, including the slower second round for all three runtimes, is retained. Historical B1's 85 ms and historical Node's 13 ms are **not** the fresh controls for this result. An earlier recollection of seconds is not a verified comparable baseline and is not used to claim a total speedup multiplier.

## Measurement boundaries and limitations

The exact original absolute `time.bxs` / `time.js` paths and bytes from `baseline-20261030T220339Z/` were reused. The original internal clocks are `getTickCount()` / `Date.now()` at millisecond resolution. Console priming and sum initialization precede the loop clock; compilation/startup and final output are outside that clock. External monotonic whole-process timing includes launch, source processing, runtime initialization, priming, execution, verification/output and shutdown. Subtracting the two measurements does not isolate compiler cost.

Both MatchBox release harnesses assert the BoxLang JIT inactive before and after interpretation. All runtimes execute natively on the same host, not in the builder container. The comparison preserves MatchBox's VM-global `sum` versus Node CommonJS wrapper-frame locals; no equality of storage cost is claimed. No marker library/controller/preload, forced GC, CPU-affinity/governor change or other instrumentation was introduced.

**Three short, millisecond-clock samples are exploratory**, not confidence intervals or general-purpose runtime scores. Environmental variation, scheduling, caches and quantization remain. No outlier was removed, workload silently scaled or peak-memory/GC-pause figure manufactured. The noneligible-local guard costs identified in report 19 remain real static costs, not measured broad-workload regressions or resolved adoption risks.

## Timing is not the instruction outcome

Original **C3-P1-v1** remains its pre-implementation conditional 218–297 instructions/cycle reduction. Pre-outcome **C3-P2-v1** remains the compiled 330/cycle prediction, with a conditional cycle of 1093 versus B1's 1423. The two predictions were frozen before these timing outcomes and are unchanged.

**No C3 retired-instruction outcome has been collected.** The timing result neither validates the exact instruction forecast nor explains its timing causally. There was no timing forecast to reconcile. Unknown signed first-hit/cold/periodic/boundary/final/residual changes are not assumed zero, and B1's old `−6662` batch residual is not inherited. The historical B1 32/cycle causal residual also remains unresolved.

## Source, binary and test provenance

C3 source/tests were subsequently committed as **`ecc9074`**, after the user approved committing and merging into local `develop`. The four committed file contents are verified byte-for-byte against the existing candidate build-source hashes. No source change or rebuild accompanied timing or repository finalization.

```text
C3 measured release executable SHA-256:
72585b4106c8876b0ed45c3dff1f2948ed1549c016698069c976b9a567f60f3a

C3 tested/built four-file patch SHA-256:
52ea408f4925b66636a46b2e89e92c0147e69713183b6bf7c8e9e53352164e76

B1 release executable SHA-256:
9af288307e2705365708940eec170e790c33c5ff94dd88c0e65829bd6cb14741

Node executable SHA-256:
fde6a4bf8d0562f7751d1a2d6cb9b417c4cfe107bbcb0aa3e9a24e125e348f48
```

The MatchBox binaries retain the same identified Rust 1.98.1 / LLVM 22.1.8, root default features, release `opt-level=z`, LTO, one codegen unit, `panic=abort`, and `RUSTFLAGS=-C strip=none`. Full original build provenance and separate targets remain preserved; a commit is not evidence that an unrelated preexisting binary was rebuilt from it.

The preceding implementation phase passed **1012 top-level tests across configurations, 29 existing ignores**, with no new ignores or weakened tests. It verified the complete conditional native cycle, all 13 normalized helper bodies, root/observer/interrupt/debugger/JIT contracts and real replacement costs. Not every defensive metadata failure was dynamically forced; no full-workspace/Miri/Wasm/ESP32 or exhaustive signal-interleaving claim is added here.

## Retention and repository finalization

After seeing the timing result, the user explicitly authorized **committing C3 and merging into local `develop`**. This retains the measured fixed-workload improvement with the documented limitations. It does **not** turn the unmeasured instruction forecast into an outcome, prove universal improvement, or erase the **45–65 extra instructions on several selected noneligible float-local paths**, warm standalone Number-CONSTANT +1 body instruction, or unknown affected-workload impact.

Repository finalization uses separate implementation/documentation commits and a non-fast-forward merge, preserving existing unpushed history and all earlier worktrees, binaries, targets and evidence. It does not authorize a push, cleanup, further experiment or optimization loop. Exact final commit/merge/tree verification is recorded in the finalization packet.

## Evidence locations

Relative to `/home/jacob/dev/ortus-boxlang/matchbox-checksum-baseline-20261030T2203Z/`:

- `benchmarks/instructions/results/c3-implementation-20261031T011126Z/`: 216 sealed implementation/test/build/native/pricing files.
- `benchmarks/instructions/results/c3-timing-20261031T021240Z/`: 44 sealed timing files, raw invocations/output, both clocks, checksums, provenance, unchanged predictions, summary and preservation checks. No instruction counts.
- `benchmarks/instructions/results/c3-repository-finalization-20261031T021859Z/`: separate authorization, preflight, commit/merge logs and final verification of all ten prior packets, preserved artifact hashes, exact source blobs and trees.

These packets and large binaries remain **ignored/local**. Documentation commits alone do not transfer them. Do not rerun collectors into sealed packets. Future affected-workload checks, instruction outcomes, builds or implementation changes need a separate explicit scope.
