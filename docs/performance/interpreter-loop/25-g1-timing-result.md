# G1 separate timing result and retention decision

**Measured timing:** fresh C3 **77ms →G1 67ms** median internal loop interval: **12.9870% less time /1.14925×** in three samples. Node `--jitless` median13ms. Whole-process medians separately: **82.4332ms →72.7859ms**,11.7031% less. These are exploratory short-interval measurements, not confidence intervals or a universal runtime score.

**Measured instructions, separate earlier processes:** [report24](24-g1-instruction-result.md) records **1092997584 →972997418**,120000166/**10.9790% fewer** retired user instructions,14.3075%of the remaining Node gap closed. No instruction counters, marker library or profiler ran in these timing samples. Counts are not converted into a timing forecast or causal explanation.

## Authorization and fixed protocol

User “Fantastic! Go for it” approved the proposed bounded counter phase followed by separate timing. The counter seal was finalized **2026-10-31T05:16:21.077314Z** before timing. This timing-only phase uses the conservative budget anchor **05:16:21Z**, <=15minutes/deadline **05:31:21Z**; the collector actually starts **05:17:58.627888Z**, after the counter seal. Workloads finish **05:17:59.994473Z**. Exactly nine fresh native processes: **C3 →G1 →Node**, repeated three times;30-second timeout each, no warmup/retry/outlier removal/batch change.

Original absolute `baseline-20261030T220339Z/time.bxs` and `time.js` were reused byte-for-byte. Core arithmetic remains:

```boxlang
var sum = 0;
for (var i = 0; i < 1000000; i++) {
    sum += i + 2;
}
println(sum);
```

All nine stdout records exactly match `PRIMED`, positive integer `ELAPSED_MS`, checksum **500001500000**; exits are0. No wrappers, scaled/repeated batches, literals/loop-bound recognition, global storage relocation, source/test/evaluator changes, new builds/version probes, native extraction, profiling/debugger sessions, installs, settings changes, refs/commit/merge/push or cleanup. MatchBox sum stays a **VM global**, Node CommonJS sum/i wrapper-frame locals. This is the direct checksum case, not the separate Sieve/JVM suite or an affected-workload sweep.

### Boundaries

- **Internal loop interval:** original `getTickCount()` / `Date.now()` millisecond clocks. Output priming and sum initialization before; elapsed/checksum output after. Includes the original clock-boundary/loop-control/variable-access/allocation/GC work as expressed by these unchanged scripts. Not isolated ADD cost or CPU time.
- **External whole-process time:** monotonic `perf_counter` around foreground process execution/capture. Includes launch, source parse/compile, runtime/entry setup, script, output and shutdown. Not isolated compiler time; do not subtract loop time to invent it.

No marker/controller/preload/counter/tracing instrumentation was inherited. The earlier counter controller's signal/wait durations are not used as timings. All batches here are **below100ms**: short three-sample millisecond timing is exploratory. Both MatchBox series decrease across rounds; raw variation is retained, not removed or claimed controlled away. No confidence interval, high-precision loop clock or steady-state assertion is made, and no larger batch was silently substituted.

## Artifacts and host

Same binaries as the immediately preceding counter matrix, no rebuild:

| Runtime | SHA-256 |
|---|---|
| C3 |`72585b4106c8876b0ed45c3dff1f2948ed1549c016698069c976b9a567f60f3a`|
| G1 |`ad2ec4ebd6aa468b9c0a8ed6707bcecdc28ea27cd761b7fa6fbe8057f8842e74`|
| Node22.23.3 `--jitless` |`fde6a4bf8d0562f7751d1a2d6cb9b417c4cfe107bbcb0aa3e9a24e125e348f48`|

Same-profile Rust1.98.1/LLVM22.1.8 root default-feature harnesses, opt-level=z/LTO/oneCGU/panicabort/`RUSTFLAGS=-C strip=none`; real parse/compiler/tree-shaken prelude and pre/post `vm.jit.is_none()` assertions. G1 source base `7882dfa` plus exact uncommitted product/test patch `af2af2f7cc1b25c89b1d91d74a0d1d8bd504ff4ae14572df6f73dcee68cdd532`, source hashes unchanged from [report23](23-g1-implementation-validation.md). A checkout or incoming merge does not identify a different executable.

Native AMD Ryzen7PRO6850U, Linux7.2.8-1-default x86-64/glibc2.44. Host/load/environment/input/command hashes in raw provenance; no security/affinity/governor/CPU/toolchain/global changes. Main remains clean at C3 `a99b926`, origin `c2b6ec8` threeahead; those incoming sources were not integrated/built/tested here. Reference/worktree heads, existing source/tests and protected targets remain unchanged.

## Raw timing results

| Runtime | Loopms samples | Median /MAD | Whole-process ms samples | Median /MAD |
|---|---|---:|---|---:|
| C3 |85;77;72|**77 /5**|91.421957;82.433158;77.466495|**82.433158 /4.966663**|
| G1 |71;67;60|**67 /4**|76.303364;72.785900;64.733474|**72.785900 /3.517464**|
| Node `--jitless` |16;13;13|**13 /0**|46.516796;44.318717;44.139630|**44.318717 /0.179087**|

Full-precision raw external seconds are preserved in each sample JSON and summary:

```text
C3:   0.09142195689491928, 0.08243315783329308, 0.0774664948694408
G1:   0.07630336401052773, 0.07278589997440577, 0.06473347404971719
Node: 0.04651679587550461, 0.04431871697306633, 0.04413962992839515
```

Fresh C3→G1:

- Loop median **10ms less**, **12.9870129870%** reduction, **1.1492537313×** speedup in these samples.
- Whole-process median **9.647257859ms less**, **11.7031278583%** reduction, **1.1325429494×** speedup in these samples.
- G1/Node loop median ratio **5.153846×**; this is distinct from report24's **3.826518× instruction ratio**.

Report20's historical C3 median73ms/Node15ms belongs to its own fresh B1/C3/Node comparison. It is not reused as this phase's control or combined into an exact total historical speedup. Current C3/Node controls are77/13ms. No run was dropped for drift, and no precision/confidence is manufactured from three short samples.

## Prediction and decision

Original G1-P1v1/v2 and pre-outcome **G1-P2-v1**,05:01:12.222763Z, SHA `9be86c1ff3be775b39e54bba3464e7a6d129e7ac09037a2097f87cfef4e00e11`, remain unchanged. P2 verified conditional cycle1093→973 and net120/cycle (154gross −34replacement,16retained); compiledG=0. No timing prediction was made, so the12.99% timing result is not presented as validation of a timing forecast.

The earlier instruction outcome exceeds the ordinary120million point by166; fresh residuals C3−2416/G1−2582, delta−166. That remains an algebraic, causally unassigned reconciliation, not noise/GC/setup attribution or a rewritten prediction. Whole-process **instruction** term S remains unmeasured even though whole-process **elapsed time** is now measured. No old signed residual is inherited or subtracted.

**Retain the single G1 candidate unchanged for review.** On this unchanged workload, correctness passes, the compiled mechanism transfers, scoped instructions improve10.98%, and independent loop/whole-process timing medians improve12.99%/11.70%. This is delivered fixed-workload evidence, not universal benefit or authorization to integrate/search further.

Keep all known tradeoffs from report23: one extra48-byte native entry allocation request and unpriced isolated setup/drop/short-script/allocator/RSS effects; empty pseudo metadata no longer mirroring active ICs; full private Chunk cloning copies its vector; Rc references can extend cache lifetime and metadata reinvocation can initialize another shared store. Ordinary function/recursive shared caches remain85/hit and existing routes were already private, so there is no ordinary-function/per-request/incoming server throughput claim. Keep C3's45–65 noneligible-local handler taxes/standalone-CONSTANT+1 and broader-workload uncertainty. No ESP32/Wasm/device/all-host-interleaving/Miri or exhaustive external-consumer proof. Preserve existing ESP32 outer serialization; no permanent VM-thread-pinning assumption. B1's32/cycle and C3's4165shortfall causal residuals remain unresolved.

## Evidence and repository state

- Counter packet: `benchmarks/instructions/results/g1-instructions-20261031T051056Z/`,28listed files, manifest `d1be0a1d85dd737bbbc7d0d956dcf14b7a77f0ac31c5c6e41c41b8a03a1f01d5`.
- Separate timing packet: `benchmarks/instructions/results/g1-timing-20261031T051621Z/`, original input/prediction copies, plan, source/provenance, exact command/order and each sample JSON/stdout/stderr, summary, final-state and seal.
- All15prior packets/1011listed files, frozen binaries/targets, predictions, code/tests, reports21–24 and earlier evidence, canonical reference and worktree heads preserved. Raw artifacts remain ignored/local; documentation commits do not transfer them.

**At this timing phase's seal,** G1 and reports21–25/index remained uncommitted in the investigation worktree. **No commit/merge/push/ref change occurred during timing. Stop after the timing seal.** Further workloads/resources/short-script/target studies, measurements/builds, source-base integration, implementation, commits/merge/push or cleanup require a separate bounded decision. The original autoresearch campaign and six-session diagnostic budget remain exhausted.

## Subsequent repository-only finalization

The user then separately requested: “Excellent. Lets commit and merge into develop.” Finalization starts **2026-10-31T05:24:22Z**, <=20minutes/deadline05:44:22Z. Implementation/tests are committed as **`73008fe8fe1bc3f4dcd3248afbb5daf8edc53706`**; both exact source blobs and the product/test diff match the frozen tested/built patch `af2af2f7cc1b25c89b1d91d74a0d1d8bd504ff4ae14572df6f73dcee68cdd532`. The executable retains its original dirty-source build identity and hash; a Git commit does not rebuild or remeasure it.

Approval covers a separate documentation commit (reports21–25/index) and a history-preserving `--no-ff` merge into existing local `develop`, initially C3 `a99b926`. Already-fetched origin `c2b6ec8` and its three incoming commits are preserved without adoption/fetch/pull/push/reset/rebase/squash. No new build/test/workload/count/timing/profile/debugger session, source/test edit, setting/install or cleanup occurs. This preserves the matching-base performance evidence and does not dynamically validate incoming server changes.

Original reports21–24 and all16 sealed packets/1057listed files, original predictions, independent targets/binaries, reference state and unrelated worktree heads remain protected. Only this post-outcome repository note/index status changes; the sealed timing packet retains the original pre-commit document bytes. New ignored packet `benchmarks/instructions/results/g1-repository-finalization-20261031T052422Z/` records exact implementation/documentation/merge identities, parents/trees/blobs, protected evidence checks and final clean statuses. Raw binaries/evidence remain local/ignored and are not transferred by documentation commits. Stop after repository verification/seal; no push or further experiment is implied.
