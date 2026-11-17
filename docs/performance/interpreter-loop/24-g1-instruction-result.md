# G1 measured scoped instructions and prediction reconciliation

**Measured:** unchanged direct million-iteration workload, **1092997584 →972997418** median retired user-space CPU instructions, saving **120000166 /10.9789965%** versus fresh C3. This closes **14.3075344% of the remaining fresh Node gap**. All nine checksums/exits and27 per-thread unscaled counters pass. This report is the counter phase only; the separately approved timing phase has not yet run when this record is sealed.

## Scope, inputs and provenance

User approved the proposed counter phase followed by separate timing (“Fantastic! Go for it”). Counter budget: **2026-10-31T05:10:56Z–05:40:56Z**, <=30minutes, fixed nine workload processes, three per engine,30-second process timeout, no retries/outlier removal. Workloads finished **05:13:24.269545Z**. No builds, product/test/evaluator changes, profiling/debugger sessions, settings changes, installs, commits/merge/push/ref changes or cleanup occurred.

Same original direct body, integer iteration count, arithmetic and output:

```boxlang
var sum = 0;
for (var i = 0; i < 1000000; i++) {
    sum += i + 2;
}
println(sum);
```

Original absolute `baseline-20261030T220339Z/count.bxs`, `count.js`, `expected-count.txt` and `markers.so` were reused, not rewritten or rebuilt. Scripts prime output and initialize sum before START, measure loop declaration/control/body between START/END, then print checksum **500001500000** after END. No warmup, wrapper, scaling, storage/boundary relocation or benchmark-specific path. MatchBox's sum remains a **VM global**; Node's CommonJS sum/i remain wrapper-frame locals.

Frozen binaries:

| Engine | SHA-256 |
|---|---|
| C3 |`72585b4106c8876b0ed45c3dff1f2948ed1549c016698069c976b9a567f60f3a`|
| G1 |`ad2ec4ebd6aa468b9c0a8ed6707bcecdc28ea27cd761b7fa6fbe8057f8842e74`|
| Node22.23.3 `--jitless` |`fde6a4bf8d0562f7751d1a2d6cb9b417c4cfe107bbcb0aa3e9a24e125e348f48`|

G1 is built from investigation `7882dfa` plus exact uncommitted patch `af2af2f7cc1b25c89b1d91d74a0d1d8bd504ff4ae14572df6f73dcee68cdd532`; product/test hashes match [report23](23-g1-implementation-validation.md). C3 and G1 have matching source base/profile, Rust1.98.1/LLVM22.1.8, default features, opt-level=z/LTO/oneCGU/panicabort/`RUSTFLAGS=-C strip=none`, unchanged real compiler/tree-shaken prelude harness with pre/post JIT-inactive assertions. Version labels come from previously probed/hash-identified provenance, not extra workload launches or inferred checkout revisions.

Native AMD Ryzen7PRO6850U, Linux7.2.8-1-default x86-64/glibc2.44. No inherited preload/audit/Node/tracing instrumentation. The unchanged standard evaluator supplies only the existing marker preload for region counters. Main stays clean at `a99b926`; origin `c2b6ec8` is3commits ahead and was not adopted, built or tested here. Canonical reference and all worktree heads are unchanged.

## Fixed protocol and raw results

Engine groups **C3×3 →G1×3 →Node×3**. Metric: aggregate retired USER instructions across pre-existing/inherited target threads, excluding kernel/hypervisor. Exactly one counted thread per MatchBox sample, seven per Node sample. All27counters have **time_running_ns ==time_enabled_ns >0**, no multiplexing/scaling/fallback. Exact stdout, markers, exit0 and checksum pass for all nine processes. Normal evaluator tests:13passed, five opt-in live tests skipped; no extra live workload tests.

| Runtime | Raw aggregate instructions | Median | MAD |
|---|---|---:|---:|
| C3 |1092997584;1092997629;1092997503|**1092997584**|45|
| G1 |972997428;972997381;972997418|**972997418**|10|
| Node `--jitless` |254277492;254276808;254279563|**254277492**|684|

Node main-thread samples separately:253985007;253856331;253831252, median253856331/MAD25079. The primary comparison uses aggregate threads, not a main-only substitution. No outlier removal or empty-boundary/GC/marker subtraction is applied. START resumption/write and END dispatch, periodic work and normal GC inside the boundary remain charged.

Controller whole-process duration is diagnostic metadata with signals/waiting, **not an uninstrumented timing outcome or whole-process instruction total**.

## Delivered scoped effect

```text
Fresh C3 median              1,092,997,584
Fresh G1 median                972,997,418
Fresh Node median              254,277,492
C3 −G1                         120,000,166
C3 −Node gap                   838,720,092
G1 −Node gap                   718,719,926
```

- **10.9789964549%** fewer scoped instructions than C3.
- **14.3075344378%** of remaining Node gap closed.
- Instruction ratio **4.2984441× →3.8265181× Node**, not a wall-time speedup.

These use fresh controls. Earlier C3 and Node medians in report21 remain historical, not correction factors. No comparison to an unidentified newly built/incoming-server executable is made.

## Prediction versus observation

Preimplementation **G1-P1v1/v2** remain preserved with their same120/cycle conditional net:154gross −34replacement;16same-PC work retained, cache170→50. They did not promise eliminating all170 or bound cold/other-user effects.

**G1-P2-v1**, frozen **05:01:12.222763Z** before outcomes, SHA `9be86c1ff3be775b39e54bba3464e7a6d129e7ac09037a2097f87cfef4e00e11`, independently verified actual compiledG=0, ordinary cycle1093→973. Its prediction is120million scoped saving **minus signed unknown D**; whole-process saving additionally subtracts unmeasured S. No absolute hardware batch-total forecast or timing forecast was made.

| Quantity | Value |
|---|---:|
| Frozen ordinary million-cycle saving |120000000|
| Measured fresh C3−G1 saving |120000166|
| Observed minus ordinary prediction |**+166**|
| Fresh C3 residual:1092997584 −1093×1m |−2416|
| Fresh G1 residual:972997418 −973×1m |−2582|
| Residual change / algebraic D |**−166**|

The observed saving is166above the ordinary point (**0.0001383%** of120million). This is an algebraic reconciliation, **not a causal explanation** of166instructions or proof every conditional branch frequency held. The signed residual change stays unassigned; it is not called noise, GC, marker overhead, setup or statistical confidence. No old C3−2270/B1−6662 residual is inherited/subtracted, and P1/P2 are not revised to fit the outcome. S remains unmeasured by these scoped counters.

## Decision, limits and evidence

**Retain G1 unchanged for the already approved separate timing phase/review.** Fixed-workload scoped instructions improve and closely support the compiled model. This does not authorize another candidate, integration or a claim of universally beneficial cache-lock removal.

Keep report23's tradeoffs: one extra48-byte native entry allocation request, cold/setup/drop/short-script/resource uncertainty, pseudo metadata's changed IC alias, full private-Chunk clone copying and Rc-extended lifetime. Ordinary functions/recursive calls retain shared85/hit cache work; existing routes were already private, so no ordinary-function/per-request server speedup is predicted. C3 noneligible-local45–65 handler-instruction and standalone-CONSTANT+1 taxes, broader workload effects, external consumers and target/interleaving limits remain. Preserve ESP32's outer serialized VM mutex; no permanent thread-pinning assumption.

No whole-process instruction, new timing, RSS/GC-pause, incoming-server/ESP32/Wasm/device or general workload claim follows. B1's32/cycle attribution residual and C3's4165saving shortfall are not resolved by this result.

Raw evidence: `benchmarks/instructions/results/g1-instructions-20261031T051056Z/` in the investigation worktree: plan, preflight/provenance/source hashes, original input and prediction copies, exact commands, evaluator tests, all nine samples/per-thread counters, summary/reconciliation and seal. All14prior packets/983listed files, binaries/targets, G1 code/tests, predictions, earlier docs, reference and refs are preserved. Raw evidence is ignored/local; documentation alone does not transfer it.

The next action under the current approval is **one separate timing-only nine-process matrix**, after this counter packet is sealed, with its own <=15minute start/deadline. No counters/preload/builds/retries/variants, no altered workload or larger batch. Stop after both phases; further checks, implementation, commits/merge/push or cleanup require a separate decision.
