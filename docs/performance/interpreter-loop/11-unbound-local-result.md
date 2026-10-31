# Unbound-local candidate: retained

Status at measurement completion: **single approved candidate implemented, validated and retained uncommitted.** Completed 2026-10-30 within the 45-minute implementation/build/test/native-review budget. During that phase there were no optimization variants, debugger/profile sessions, installations, campaign restart, commits or pushes. The retained reference executable was not replaced.

Subsequent user-approved finalization: source/tests committed as **`ef95ce4c41ccdec992c791f5d11f2392a99053d4`**, with these investigation reports versioned separately. The user approved merging the full stack into `develop`, including the four retained interpreter improvements that form this candidate's tested baseline. Finalization does not authorize another improvement run; the measured sources and original evidence remain unchanged.

## Outcome

Same million-iteration marked checksum region, three fresh native processes each, no warmup, exact output **500001500000**. All-thread retired user instructions; reported main-thread totals equal aggregate totals in these samples. Every counter has `time_running == time_enabled > 0`; no scaling/fallback. No concurrent measurement workloads.

| Metric | Reference | Candidate |
|---|---:|---:|
| Median retired instructions | 2,172,994,420 | 1,818,993,931 |
| MAD | 37 | 265 |
| Batch-normalized instructions/iteration | 2172.994420 | 1818.993931 |

**Reduction: 354,000,489 instructions, or 16.2909%.** This is not itself a latency result. Against the original, non-refreshed Node total, the ratio falls from about 8.55x to **7.15x**, closing about **18.45%** of that comparator gap. Language/storage differences and Node's earlier approximately 3.3% model residual remain; no parity or universal runtime claim.

Raw instruction samples:
- Reference: **[2172994457, 2172994420, 2172993749]**.
- Candidate: **[1818994349, 1818993931, 1818993666]**.

## Separate exploratory timing, as requested

No timing prediction or timing-driven tuning. Same original million-loop timing script copied byte-for-byte, with `getTickCount()` immediately around the loop; millisecond resolution. Startup/source compilation, console priming, output and verification are outside the loop clock. Three fresh processes per row; no warmup, preload, markers or tracing. Processes were serialized; outcome runs alternated reference/candidate.

| Batch | Loop samples (ms) | Median (ms) | MAD (ms) |
|---|---|---:|---:|
| Reference, before product edits/build | 140, 153, 156 | 153 | 3 |
| Reference, refreshed alongside outcome | 162, 141, 142 | 142 | 1 |
| Candidate | 121, 117, 124 | 121 | 3 |

The refreshed comparison is **142 → 121 ms**, approximately **14.8% lower loop time**, a median ratio of about **1.17x**. The pre-build reference median was 153 ms: **report that drift**, rather than selecting it to advertise a larger improvement. The 162-ms reference sample is retained. Three samples, millisecond quantization and uncontrolled host variation do not establish a confidence interval or a robust universal speedup.

Whole-process wall times were also saved by the timing harness, explicitly labeled as including startup/compiler/output. Counter-controller whole-process times include signal/control waiting and are **not** used as loop timing. No new Node timing was requested or run.

## What changed

- `crates/matchbox-vm/src/vm/scopes.rs`: distinct private `ScopeMode::Unbound`; admission only with no parameters/arity/bindings and not ComponentBody; checked direct stack reads/writes; invalidation before BIND_LOCAL. Existing Stack/Shared behavior and bound-local dictionary paths remain.
- `crates/matchbox-vm/src/vm/mod.rs`: admission called only in the existing frame-change reload block. No new per-dispatch classification, changed polling cadence, pointer lifetime or cache ownership.
- `crates/matchbox-vm/src/vm/scopes/unbound_tests.rs`: seven invariant/transition/bounds tests.

There is no benchmark recognition, type-specialized interpreter, changed algorithm, extra execution engine, weakened test or enabled JIT. Ordinary zero-declared-argument UDFs still have implicit arguments and do not qualify. Values remain rooted in the same fiber stack. Late binding and exposure invalidate the certificate before the scope-aware path is needed.

## Prediction → compiled mechanism → measurement

P1 was written before implementation in [10-candidate-selection.md](10-candidate-selection.md), and copied unchanged into the raw packet. It predicted **310–449 fewer instructions/cycle** after replacement cost, not elimination of all 553 instructions.

After building, but **before outcome counters or candidate timing**, `prediction-P2-native.md` recorded a conditional static path refinement:

| Work per ordinary continuing cycle | Reference executed trace | Candidate static path model | Delta |
|---|---:|---:|---:|
| Three local-access sequences | 553 | 155 | −398 |
| Everything outside those sequences | 1620 | 1664 | +44 |
| **Total** | **2173** | **1819** | **−354** |

Candidate local sequences: two reads **52 each**, one write **51**. Actual checked loads/store remain. Binding/Option helper execution disappears from those paths; repeated fiber access is reduced, not eliminated. This is a compiled replacement bill, not a subtraction of hotspot samples.

The surrounding growth is real: shared header grows from 84 to 91 instructions, seven times (**+49**), offset by a net **−5** elsewhere. **P1's isolated 0–40 surrounding-code allowance was four too small**, although its overall net range still contains the result. Do not rewrite that estimate after learning the compiler output. Effective replacement including surrounding growth is **155+44=199**, within P1's broader 104–243 budget. The generated code does not achieve P1's most optimistic reference-reuse case.

A second disjoint native-accounting view:
- `run_fiber_inner` self: **1123 → 1058**, −65.
- Fiber-index helper self: **294 → 246**, −48 (49 → 41 calls).
- Local binding/Option helper self: **241 → 0**, −241.
- Other helper self totals unchanged in the model.
- Total: **−65−48−241 = −354**. This overlaps the table above; do not add them.

### Evidence strength and reconciliation

The reference paths were previously debugger-observed. The candidate model is **conditional static native inspection**, not a new debugger trace. `native-walk.py` follows explicit reviewed branch decisions for ordinary numeric continuing cycles; twelve helper functions directly/indirectly used have identical complete normalized instruction sequences across the reference/candidate. Relocations/comments are ignored; this does not independently prove equivalence of all referenced data.

Assumptions include Unbound mode, valid bounds, warmed global/constant hits, ordinary GC-check phase, no pending completion/yield/trace, and JIT/timeslice inactive. Source invariants, tests and compiled mode3 admission/branches support those assumptions. Runtime/boundary paths are left to the independent hardware envelope.

P2 predicted approximately **354 × 1,000,000 = 354,000,000** fewer instructions. Measured reduction: **354,000,489**; difference **489** instructions over the batch. The separate envelope residuals are:
- Reference: measured **5580 lower** than `2173×1m`.
- Candidate: measured **6069 lower** than `1819×1m`.

These residuals account arithmetically for the 489 difference. They are not exhaustively assigned to initialization, safepoints or final iteration behavior. The candidate ordinary-cycle model differs from its measured batch by about **0.000334%**. That close agreement strongly supports the native mechanism without turning static assumptions into a dynamic trace of every iteration.

## Correctness, build and resource checks

| Suite | Result |
|---|---|
| VM unit, including seven new tests | 66 passed |
| VM integration | 17 passed |
| Root library | 27 passed |
| BoxLang compatibility | 543 passed, 29 ignored |
| Host lifecycle | 5 passed |
| Main integration | 195 passed |
| Route/cache isolation | 4 passed |

**857 cases passed in the listed suites.** VM unit/integration suites were repeated both without JIT features and with the JIT feature; those repeat executions are not counted again as unique cases. Coverage includes existing lexical closures, scope aliases/deletion/nulls, script loop visibility, finally/throws, nested future callbacks and JIT contracts. New tests cover admission exclusions, nonzero stack base, relocation/GC/null values, late binding, permanent exposure/capture, truncation/reload, wide binding and bounds panics.

Initial test compilation failed on a mutable-borrow lifetime in BIND_LOCAL and a fixture `&str`/String mismatch. Both were mechanically corrected within the same design; original failing patch/log retained. No source-tuning variant or failed runtime score was hidden. Existing host OpenJDK25 was mounted read-only for tests; no install or test weakening.

Pinned cached builder/Rust **1.98.1**; original `opt-level=z`, LTO, one codegen unit, panic abort, default features, compiler/prelude and JIT-inactive harness. Candidate keeps symbols with `-C strip=none` for native inspection, matching the earlier isolated diagnostic method. Measurement executes that identified artifact natively; no container runtime timing. The reference symbol companion was rechecked against original allocated ELF sections: all bytes/addresses/flags match except the build-ID note.

Native x86-64 resource observations (not cross-target measurements):
- Value/frame layout unchanged; native frame stride **216 bytes**.
- `run_fiber_inner` native stack reservation remains **2856 bytes**.
- Interpreter function code grows **91,444 → 94,945 bytes** (+3501).
- Entire ELF `.text` grows **5,176,675 → 5,180,451 bytes** (+3776, approximately 0.073%). This is generated code size, not retired work or memory traffic.

No new performance measurements of noneligible workloads, debugger-feature build, ESP32/Wasm cross-build or device validation. Noneligible accesses gain a mode decision; their timing/instruction tradeoff remains unmeasured. These limits are not erased by passing the semantic suites.

## Artifacts, decision and state

Raw packet, ignored and requiring explicit transfer:
`benchmarks/instructions/results/unbound-locals-20261030T210916Z/`

Key files: approved plan, P1/P2 snapshots and pre-outcome hashes, timing script/reports/stdout/stderr, exact count reports, build/test logs (including initial failure), candidate patch/executable, preserved unstripped reference, full disassemblies, explicit native branch model/helper validation, ELF metadata and machine-readable `result.json`. No original evidence packets were overwritten.

- Original reference SHA-256: `5787b8a92026d52d97ce97d18cf552d06fbfc8d324493bcd661b94aa8e220557`.
- Candidate SHA-256: `d197ed47a737f26fe3f1d2e89b30f2f9fbe708e66f8435e5b589718d462bddf7`.
- Patch SHA-256: `dae01946888fff293e86199365f8719d73e60c66c2c4d1a12b1b89ddf1f95b98`.
- Evidence archive: `benchmarks/instructions/results/unbound-locals-20261030T210916Z-evidence.tar.gz`, SHA-256 `85e4966230270fc621c5a5327d391b1e9f14e1bd61c827b8aa5e6b51949ab7a1`. Originals retained; ignored archive needs explicit transfer. Packet files are covered by `files-sha256.json`.

**Decision at measurement completion: retain the one candidate uncommitted; subsequently committed as noted above after explicit user approval.** Primary instruction improvement agrees with the prior model and tested contracts pass. Timing is a favorable exploratory secondary observation, not the reason to select or tune the implementation. Reference binary remains original; main product source/tests contain the candidate, and the existing diagnostic worktree mirrors it with separate build outputs. No further optimization experiment is automatically authorized.
