# B1 performance outcome: retained for review

2026-10-31. **Measured: 21.77% fewer retired user-space instructions for the unchanged direct million-iteration loop.** This closes **25.31% of the fresh Node `--jitless` instruction gap**. Separate unprofiled timing samples measured 122 → 85 ms median, **30.33% less loop elapsed time (1.44×)**, with only three samples and millisecond clocks.

State at the outcome gate, before separately authorized repository finalization: **retain this one B1 candidate for review** in worktree `/home/jacob/dev/ortus-boxlang/matchbox-checksum-baseline-20261030T2203Z`, branch `investigation/checksum-baseline-20261030T2203Z`. Main checkout/local `develop` remain at `582473adb6eb643f35ec1fe2e21da37520fb3ad0`; no commit, merge or push. No automatic next optimization or profiling/variant loop is authorized.

Read [implementation/contracts/native validation](16-b1-implementation-validation.md), [original pre-implementation P1](15-b1-pricing-prediction.md), and [selection](14-design-selection.md). Those are preserved historical decision records, not rewritten to fit this outcome.

## Separately authorized repository finalization

After these results, the user approved committing the validated implementation/tests and investigation documents, then merging the stack into local `develop`. Implementation/test commit: **`57b6f18d0ad2ace67557cadccbb1b4daa5292219`**. The documentation is recorded separately; the authorized non-fast-forward merge preserves the existing local stack rather than resetting, rebasing or squashing it. No push or further performance work is authorized.

No product edits, builds, tests or workloads are added during finalization. The implementation commit's four-file diff from `582473a` is byte-identical to the tested `candidate.patch` below. The original binaries, P1/P2 and six prior evidence packets remain unchanged. Their recorded old HEAD/dirty states are intentionally historical; the new Git commits do not imply that the measured executables were rebuilt. Local finalization provenance is saved under `benchmarks/instructions/results/repository-finalization-20261031T001742Z/` in the dedicated worktree.

## 1. Scope and validity

The user separately authorized the proposed 30-minute outcome phase. It began 2026-10-31T00:01:41Z; all workload processes finished by 00:10:51Z. Exactly **18** workload processes ran, serialized: three instruction counts plus three SEPARATE unprofiled timing samples for each of baseline MatchBox, B1 MatchBox and native Node v22.23.3 `--jitless`. No new builds, tuning, extra warmup, retries, profiler/debugger sessions, installs or security changes. Normal evaluator tests passed 13 tests; five opt-in live tests remained disabled/skipped.

Every workload exited zero and produced checksum **500001500000**. All three counter reports completed, totaling nine valid samples; **every counter had `time_running_ns == time_enabled_ns > 0`**, with no multiplex scaling, simulated fallback or overhead subtraction. Both MatchBox harnesses assert JIT inactive before/after execution. Native execution was on the same Linux x86-64 host, AMD Ryzen 7 PRO 6850U, kernel 7.2.8-1-default, glibc 2.44; only the earlier builds used the pinned container.

The exact original count/time script paths and bytes were reused. Source body remains:

```boxlang
var sum = 0;
for (var i = 0; i < 1000000; i++) {
    sum += i + 2;
}
println(sum);
```

As before, MatchBox's `sum` is a VM global; Node's CommonJS `sum`/`i` are wrapper-frame locals. No wrapper, storage relocation, arithmetic change or iteration-count change manufactured parity. Zero warmup matches the earlier baseline policy; console priming and sum initialization precede START.

Instruction scope: aggregate retired USER-SPACE instructions across attached/inherited threads between START/END markers, including normal allocation/GC and marker overhead inside that boundary. Startup/source compilation/priming/sum initialization are outside it. MatchBox has one counted thread; Node has seven. Node's raw main-thread medians are separately retained, not substituted for its aggregate. Timing uses the original unmarked scripts with `getTickCount()`/`Date.now()`, no preload/counter controller/tracing. Whole-process wall times are separately archived, not compiler-cost estimates or subtracted overhead.

## 2. Measured instruction results

Three fresh processes/runtime; MAD = median absolute deviation. Per-iteration values are batch normalization, NOT marginal isolated arithmetic cost.

| Runtime | Raw aggregate instructions | Median | MAD | Batch-normalized / iteration |
|---|---|---:|---:|---:|
| Baseline MatchBox | 1818994113, 1818993672, 1818993724 | **1,818,993,724** | 52 | 1818.993724 |
| B1 MatchBox | 1422993338, 1422993171, 1422993539 | **1,422,993,338** | 167 | 1422.993338 |
| Node `--jitless` | 254276690, 254277177, 254277011 | **254,277,011** | 166 | 254.277011 |

**Measured reduction: 396,000,386 instructions**, or **21.7703%** of the fresh MatchBox baseline. The fresh comparator gap falls from **1,564,716,713** to **1,168,716,327** instructions: **25.3081% gap closure**, not 21.77% gap closure. MatchBox/Node instruction ratio improves **7.15359× → 5.59623×**. There is still a substantial comparator gap; this is not parity or a universal runtime ranking.

MatchBox main-thread totals equal aggregate in all samples. Node main-thread raw counts are 253814896, 253815726, 253873944 (median 253815726, MAD 830); all-thread totals above remain the comparison metric.

Fresh controls are stable in instruction count against the earlier packet: baseline median changes by **−115** (−0.00000632%); Node by **−2165** (−0.00085143%). These are not adjustments to the result. All headline comparisons use the fresh controls.

## 3. Separately measured elapsed time

Fixed order baseline → B1 → Node repeated three times, separate fresh processes after the counter samples. Clocks have 1 ms resolution; three samples do not establish confidence intervals or portable speedups.

| Runtime | Raw loop ms | Median ms | MAD ms | Unprofiled whole-process median seconds, diagnostic |
|---|---|---:|---:|---:|
| Baseline MatchBox | 122, 133, 119 | **122** | 3 | 0.126415 |
| B1 MatchBox | 85, 107, 83 | **85** | 2 | 0.089504 |
| Node `--jitless` | 13, 13, 13 | **13** | 0 | 0.040120 |

The B1 loop median is **37 ms / 30.3279% lower**, baseline/B1 **1.43529×**. This is measured separately, NOT inferred from the 21.77% instruction reduction. No microarchitectural reason for the different instruction/time percentages is established here. B1's 107 ms sample and baseline's 133 ms sample remain visible, not dropped as outliers.

Earlier baseline loop median was 118 ms; the fresh control is 122 ms. Node's median remains 13 ms. Use 122, not the older 118, for this timing comparison. Whole-process values include startup/source work/output/shutdown and launcher/wait overhead; they do not isolate compiler time. No process-minus-loop subtraction or whole-process instruction-saving claim is made.

## 4. Original P1 versus compiled P2 versus outcome

Both frozen predictions predate all outcome measurements. Original P1 is unchanged, SHA-256 `be982076228ab71b1913ee6f14d452f942ffdbba53cd813f32f5bbf8bb9c825c`.

- **P1**: 223–331 million fewer instructions, conditional planning range. The observed reduction exceeds its upper case by **65,000,386**. P1 underpredicted this compiler output; it was not a guaranteed bound or a forecast now retroactively “confirmed.”
- **Pre-outcome P2** (`2026-10-30T23:43:15Z`): complete conditional ordinary cycle 1819 → 1423, **396 million** approximate batch reduction. Measured reduction is **396,000,386**, just **386 more** (absolute delta discrepancy **0.0000975%** of the forecast reduction).
- P2's original point, if the old whole-batch residual were unchanged, was **1,422,993,839**; observed candidate is **501 below**. Updating only the reference to the fresh baseline would give **1,422,993,724**, **386 above** the observed candidate. Neither unchanged-residual assumption is treated as proved.

The already-recorded, disjoint P2 bridge remains:

```text
338 planned call/helper/caller-setup removal
+21 unused per-dispatch JIT-pointer preparation
+15 cached promotion-pointer state/read
 +7 redundant code-pointer store
 −7 introduced active-view preservation copy
−10 introduced checked current-frame derivation
+32 UNASSIGNED NET other main-code change
=396 instructions/ordinary cycle
```

The compiled full bill includes replacement/surrounding work; **32 instructions/cycle remain causally unassigned NET**, not pure elimination or proof that replacement was free. Close aggregate hardware agreement supports the overall conditional model for this input, but does not prove each selected branch/frequency or complete that causal explanation. No new execution trace was collected.

Whole-batch arithmetic residuals remain explicit:

```text
baseline:  1818993724 − 1819×1000000 = −6276
candidate: 1422993338 − 1423×1000000 = −6662
residual change: −386
```

The extra 386 measured reduction is the residual difference. It is **unassigned**, not labeled GC, cold/exit work or measurement noise without discriminating evidence. These residuals are not overhead estimates and are never subtracted from the raw results. Periodic/boundary/marker/cold behavior and ordinary-path extrapolation were not independently re-traced.

## 5. Provenance and preservation

Both MatchBox binaries use the same already-built release configuration: default features, opt-level=z, LTO, one codegen unit, panic=abort, `RUSTFLAGS=-C strip=none`; Rust 1.98.1 / LLVM 22.1.8, pinned cached offline builder from report 16. No rebuild was needed or performed.

| Artifact | SHA-256 |
|---|---|
| Baseline executable | `abe03005c7464ecb3b396d8844a06e6692522ce5c36a5700e1c43a0dd6a73d11` |
| B1 executable | `9af288307e2705365708940eec170e790c33c5ff94dd88c0e65829bd6cb14741` |
| B1 product/test patch | `8f98810941fb7601f11a89be4c1d0db8362c0f5d23724f2a0c94e995aff283fd` |
| Node v22.23.3 executable | `fde6a4bf8d0562f7751d1a2d6cb9b417c4cfe107bbcb0aa3e9a24e125e348f48` |

Before execution, all five prior evidence-packet manifests were verified intact; source hashes and candidate patch matched the tested/native-verified implementation. Original script paths/bytes, unchanged evaluator/harness and existing marker library were retained. Full commands, host/environment, timestamps, executable/input/helper hashes and every counter/stdout/stderr are archived. The main checkout/local develop and baseline outputs remain unchanged.

Outcome packet, relative to the dedicated worktree: **`benchmarks/instructions/results/b1-outcome-20261031T000141Z/`**. Key files: `plan.md`, `measure.py`, `provenance.json`, `commands.json`, `count-{baseline,candidate,node}.json`, `time-*-{0,1,2}.{json,stdout,stderr}`, `summary.json`, original P1/P2, `reconcile.py`, `reconciliation.json`, final state and hashes. It is ignored/local; Git documents alone do not transfer raw artifacts.

## 6. Decision and limits

**Retain B1 for review:** the primary instruction metric improves substantially, all 18 checksum/counter validations pass, separate timing samples improve, and the earlier 980 contract-test passes across configurations remain intact. No tests were weakened and no product changes followed the validated build. This is one measured improvement, not another campaign.

Functional tests do not establish performance of JIT/debugger/bound/Shared/callback-heavy workloads, ESP32 or Wasm. Pointer work displaced to active JIT invocation is not globally eliminated work. Cold/root work outside START has no isolated instruction bill; do not extrapolate the region improvement to whole-process instructions or other users. MatchBox/global versus Node/local storage remains part of this comparison. The 32-per-cycle causal residual and 386-per-batch delta residual remain open.

Further attribution, other workloads/targets, another candidate or additional measurements require separate authorization. The subsequent authorization covers this one commit/merge finalization only, not a push or renewed optimization work. Reprice overlapping deferred candidates against the retained B1 state before future selection; do not add their old opportunities to this saving.
