# Evidence-driven improvement: executed native-path ledger

Status: **attribution/mechanism pass complete; no candidate implemented in this pass.** See [subsequent candidate selection and P1 model](10-candidate-selection.md) for the current design recommendation. This follows the nine-question global methodology. It replaces hotspot-to-savings inference with bounded executed-path accounting. No product change, build change or new optimization variant was made. The exhausted autoresearch campaign remains frozen.

## 1. What are we working on?

The unchanged million-iteration direct checksum loop on retained MatchBox `139be94` versus Node v22.23.3 `--jitless`. The primary concern is retired user-space instructions, not generated code size or elapsed time. Preserve BoxLang behavior, global/request namespace contracts, concurrency, GC, scheduler and error behavior. Do not change the benchmark's algorithm or storage arrangement to manufacture parity.

This pass used existing counter/profile evidence plus a bounded native debugger diagnostic: six sequential GDB sessions, at most 90 seconds and 30,000 single-step stops per session, two continuing iterations per session. All six completed and printed exact final checksum **500001500000**. No installations, security changes, product/source edits, new builds or concurrent workloads. The pre-execution plan and an amendment allocating the last two sessions are retained.

Raw packet: `benchmarks/instructions/results/native-ledger-20261030T201844Z/`. Evidence archive: `benchmarks/instructions/results/native-ledger-20261030T201844Z-evidence.tar.gz`, SHA-256 `99663489051e57b7e81c60e1d00702fa347150eb253b2d3b70c190a400795efe`. Originals retained; ignored artifacts must be transferred explicitly. The archived investigation snapshot predates this archive-hash annotation.

## 2. Where are we currently?

Existing validated million-iteration regional hardware totals:

| Runtime | Median retired user instructions | Batch-normalized / iteration |
|---|---:|---:|
| Retained MatchBox, latest reference comparison | 2,172,993,764 | 2172.993764 |
| Node --jitless, original comparison | 254,269,661 | 254.269661 |

Gap **1918.724103 instructions/iteration**, ratio about **8.55x**. These are existing measurements, not refreshed counters in this pass. The Node and MatchBox totals are not a newly paired timing experiment.

Reference executable SHA-256 remains `5787b8a92026d52d97ce97d18cf552d06fbfc8d324493bcd661b94aa8e220557`. Real compiler/tree-shaken prelude; JIT compiled but asserted inactive. Release opt-level=z/LTO/one codegen unit/panic abort unchanged. Original reference executed; separately verified matching symbol companion supplied function names/addresses. Node debugger runs use baseline `--jitless` flags, NOT the profiler-only `--perf-basic-prof` flag.

Startup/compilation/output excluded from existing counter regions except START write/resumption and END dispatch. No warmup, three fresh-process samples per existing batch. Hardware totals include counted inherited threads. The debugger traces below cover only the executing main thread and selected continuing iterations; they are not timing or new PMU measurements.

## 3. Where do we want to be?

Product goal: materially reduce the instruction gap while preserving behavior and general applicability. There is no supported promise of parity or an agreed percentage for another candidate.

This pass's acceptance target: a disjoint native-path ledger that reconciles closely with the existing envelope, identifies corresponding work on both runtimes, and exposes unexplained work. Do not implement merely because a region looks expensive. A product candidate still needs a replacement design and net savings estimate.

## Diagnostic method and limits

Launch unchanged binaries under host GDB with auto-loading disabled and ASLR left enabled. Resolve actual relocated MatchBox addresses; use Node's existing builtin symbols. Locate the loop using native handler entry points and operand conditions. Ignore earlier hits to reach selected iterations, disable the breakpoint, single-step through two complete continuing cycles, then resume to verified exit/checksum.

Recorded states:
- MatchBox reference: i=3,4 and i=100000,100001, validated from numeric ADD operands and checksum arithmetic.
- Node: i=3,4; i=20000,20001; i=100000,100001, validated from the actual frame/register operand in the bound comparison.
- Already rejected cache candidate: i=3,4, solely to reconcile its prior regression; no rebuild or variant.

Raw `checkpoints.bytecode_site` is the tracer's RBX snapshot, meaningful as an instruction-site field for the reference only. The rejected binary has different register allocation; that field is not interpreted as its bytecode site. Its selected i values are validated using numeric operands instead.

A debugger single-step stops repeatedly within REP MOVSB. Reference MatchBox produces 2331 stops/iteration, including 80 byte-steps for each of two REP copies. Collapse consecutive stops at each REP instruction to one logical instruction instance: **2331 − 2×79 = 2173**. This is a stated logical instruction model, not assuming that debugger stop count equals PMU retirement. Its agreement with independent hardware totals strongly supports it for this workload. Raw stops remain available.

Tracing perturbs scheduling and execution. Four observed MatchBox iterations are not proof that every iteration has the identical path; periodic safepoints, setup/exit and runtime work remain. No kernel/other-thread work is attributed from these traces. Native path labels and symbol names can include identical-code-folding artifacts; a shared generic name does not establish regex execution.

## 4. What work accounts for the gap?

### MatchBox: 2173 instructions in each traced reference iteration

Disjoint native-PC/function buckets, not inclusive profile percentages:

| Native work | Instructions / traced iteration |
|---|---:|
| Shared dispatch/header self | 462 |
| Handler bodies self, including inline state/local work | 661 |
| Fiber-indexing helper self: 49 calls × 6 | 294 |
| Local binding/Option helpers self | 241 |
| Operand stack push/pop helpers self | 130 |
| Constant read and constant-vector helper self | 80 |
| Numeric conversion helper self | 88 |
| Cache retrieval + mutex/poison/guard helpers self | 170 |
| Shared RefCell borrow helper self | 27 |
| Other value-vector indexing helper self | 20 |
| **Total** | **2173** |

Handler-body self breakdown: FOR_LOOP_STEP 279; both ADDs combined 134; GET_LOCAL 100; SET_GLOBAL_POP 62; GET_GLOBAL 60; CONSTANT 26. These exclude helpers and shared header work.

The seven per-dispatch header paths each contribute 66 self instructions and three six-instruction fiber-indexing calls: **84 × 7 = 588**, about **27.1%** of the whole iteration. This is a subtotal of the table, not additive to it.

All 49 fiber-index calls plus their call instructions contribute **294 + 49 = 343**, about **15.8%** of the iteration, excluding caller argument preparation. Those calls are distributed across the header, handlers and helpers; do not add this subtotal to the table either.

Source-level local binding lookup does not mutate a local dictionary each iteration. The traced path nevertheless executes the binding checks, frame/base reconstruction and stack fallback. Numeric work executes float operations/tagging, not string parsing or array allocation.

### Node: ordinary paths depend on execution state

| Native handler self | i=3,4 | i=20000,20001 | i=100000,100001 |
|---|---:|---:|---:|
| LdaSmiExtraWide | 7 | 7 | 7 |
| TestLessThan | 30 | 34 | 34 |
| JumpIfFalse | 6 | 6 | 6 |
| Ldar, two executions | 16 | 16 | 16 |
| AddSmi | 23 | 28 | 28 |
| Add | 36 | 41 | 58 |
| Mov | 9 | 9 | 9 |
| ShortStar | 7 | 7 | 7 |
| Inc | 33 | 37 | 37 |
| JumpLoop | 33 | 40 | 40 |
| ExtraWide | 5 | 5 | 5 |
| **Total** | **205** | **230** | **247** |

These rows include each handler's dispatch/feedback/address work. No outside native helper calls occur in these selected Node cycles. This does NOT imply no runtime/GC calls elsewhere in the batch.

Native code carries bytecode-array/offset/dispatch-table state in registers and addresses the interpreter register file relative to a frame base. "V8 register" does not mean every language variable stays in a physical CPU register. In this CommonJS wrapper, sum and i are frame/register-file locals; MatchBox sum is a VM global. This difference is preserved, not silently made storage-matched.

The early-to-middle change adds 25 instructions in control/feedback paths (4 compare + 5 AddSmi + 5 Add + 4 Inc + 7 JumpLoop). Inspected traces show additional feedback-slot/root checks; this is not JIT execution. Middle-to-late adds 17 instructions in ADD: the sum is now heap-number-backed, with double arithmetic and a fast 16-byte bump allocation. The selected native trace explicitly shows the allocation-limit check, allocation-pointer advance and stored double. MatchBox uses immediate NaN-boxed numbers on its observed path.

Thus Node also pays real generic arithmetic, feedback, back-edge and allocation costs. The difference is not "seven versus twelve cheap jumps."

### Corresponding functional work: a disjoint comparison

Grouping the observed late paths by the computation they perform, with dispatch included:

| Functional work | MatchBox | Node late path | Difference |
|---|---:|---:|---:|
| Compute i + 2 | 789 | 36 | 753 |
| Update sum | 785 | 74 | 711 |
| Increment, bound test and back edge | 599 | 137 | 462 |
| **Total** | **2173** | **247** | **1926** |

Derivation, validated on all four reference iterations:
- MatchBox i+2: GET_LOCAL 307 + CONSTANT 233 + first ADD 249. Each includes its following header/callees. Node: first Ldar 8 + AddSmi 28, with immediate 2 embedded in AddSmi rather than a separate generic constant load.
- MatchBox sum update: GET_GLOBAL 270 + second ADD 249 + SET_GLOBAL_POP 266. Node: Add 58 + Mov 9 + ShortStar 7; the old sum is addressed directly as a frame/register operand within Add, not a VM-global cache query.
- MatchBox loop control: FOR_LOOP_STEP 599. Node: wide bound load/prefix, comparison/branch, second Ldar, Inc and JumpLoop total 137. Their chronological placement differs, but each continuing cycle includes them once.

This connects the gap to corresponding work without pretending that all excess work is removable or the storage/semantic contracts are identical. In particular, even i+2 (which does not touch the global sum) is far more expensive in the current MatchBox representation: stack-local/scope/state reconstruction, generic constant retrieval, generic arithmetic and three shared dispatch passes versus a register-file load and immediate arithmetic handler. `semantic-comparison.json` retains the grouping and underlying per-opcode counts.

### Reconciliation with the hardware envelope

- MatchBox model: 2173 × 1,000,000 = 2,173,000,000. Existing latest hardware total is **6236 lower**, a **0.000287%** whole-batch difference. Do not arbitrarily distribute this residual; periodic checks, cold/final paths and region work were not exhaustively traced. At 100000 iterations, the same model differs from the older hardware median by 5958 instructions (0.0027%).
- Node: using 230 before the sum exceeds Smi range and 247 afterwards gives a deliberately approximate ordinary-path model of **245,885,922** at one million iterations. It uses arithmetic to locate overflow at i=65534, NOT an observed transition trace. It omits the initial 205-instruction regime, transitional/feedback work, GC/runtime activity and boundary work. Existing hardware exceeds it by **8,383,739**, about **3.30%** of Node's total. At 100000, this model is 23,585,922 versus measured 23,668,692 (0.35% difference). Earlier profiles contain GC activity, but the complete residual is NOT assigned to GC without further evidence.

The ordinary late-path difference is **2173 − 247 = 1926** instructions. Its discrepancy from the batch-normalized gap is primarily an explicit Node runtime/regime residual, not unaccounted thousands of MatchBox instructions. This is a much stronger account than the earlier sampled shares, but not an exact universal per-iteration bill.

### Correction to sampled-share interpretation

The prior instruction samples remain valid recorded observations, but their PC shares are not reliable instruction bills for this deterministic loop. For example:
- Fiber helper self: traced **294/2173 = 13.5%**, versus sampled 6.4%.
- Cache_get self: traced **70/2173 = 3.2%**, versus sampled 9.0%.

Ordinary sampling/skid/periodic aliasing can misattribute concentrations. This pass does not isolate which sampling mechanism caused each distortion. Use executed-path counts for these observed paths; keep old sample tables as historical evidence, not savings estimates.

## 5. Why is that work occurring, and what requires it?

Concrete repeated-state mechanism:
- `crates/matchbox-vm/src/vm/mod.rs:4316` checks current fiber/frame/yield state each dispatch.
- `vm/mod.rs:4366` refreshes the stack-local base every dispatch because a preceding opcode may reallocate the stack, even though JIT execution is inactive.
- Three header fiber-index calls are observed at native caller PCs `0x40c318`, `0x40c339`, `0x40c588`, seven times each. They support frame-empty, yield and local-pointer accesses on the same current fiber. The first call has a preceding explicit fiber-index bounds check; the helper still checks bounds again.
- Other handler/helper paths repeatedly resolve current fiber/frame/base. The code already caches code/IP/promoted state behind `frame_changed`; that cold reload block is NOT charged every dispatch.
- Node's observed handlers address stable frame/register state directly and dispatch without returning through MatchBox's shared header and outlined container interfaces. Both retain checks, but their organization and representation differ.

These are actual execution-path differences. They establish substantial repeated state-access scaffolding, not that all checks can safely disappear. Cache synchronization is real but the complete cache/lock helper chain is **170 instructions**, not a copy-only 9% target. The two wide copies themselves are **two logical REP instruction instances** in the reference ledger; their argument preparation remains included in accessor self work. Data volume/timing effects are separate; two REP instances is not the entire cost of returning a wide result.

## 6. What can we replace or eliminate safely?

A concrete next design question is **one scoped current-fiber borrow per dispatch header**, rather than three independently reconstructed accesses. End the borrow before handlers/operations that may change fibers, frames, stacks or invoke VM methods. Preserve yield, GC, interrupts, frame reload and stack-pointer refresh semantics. Do not hold raw state across arbitrary callbacks or allocation boundaries without an invalidation/lifetime design.

This is narrower than caching raw pointers across the whole interpreter and does not require weakening Chunk's Send + Sync contract. It is a candidate design direction, not an implemented fix. The broader 49-call layer may require more coherent execution-state ownership; do not assume all 49 accesses are redundant or removable.

Alternatives: full active-execution-state redesign with explicit invalidation; local/scope representation changes; no change pending stronger design. A failed cache-copy candidate is not, by itself, justification for any of them.

## 7. How much closer should that get us, and why?

Concrete removal opportunity, **not yet a complete net prediction**:

The second and third header lookups each execute four caller argument-preparation instructions + one call + six callee instructions. Removing both reconstructed accesses at each of seven dispatches would remove:

**2 × 7 × (4 + 1 + 6) = 154 instructions/iteration**, about **7.1%** of MatchBox's traced total, BEFORE replacement overhead/code-generation changes.

The first lookup and required field reads/checks remain. A scoped-reference implementation may add spills, moves or different bounds machinery. Therefore the current equation is **net saving = 154 − introduced/rearranged work**. It is not a promise of a 7.1% improvement and certainly not a promise to remove the entire 588-instruction header.

Stage-7 gate remains open: review the lifetime-safe replacement and specify its introduced work, assumptions and acceptable prediction range before implementation. Compile-time mechanism inspection would then verify that redundant lookups disappear, and preserve/version the prediction before any outcome counters. No new candidate is implemented in this pass.

## 8. What actually changed, and by how much?

Only diagnostic scripts/evidence/documentation. No product implementation or delivered speed/instruction improvement. Twelve total iterations were traced across six sessions, with all final checksums verified. No new timing, counter sweeps, profiles or optimization variants.

## 9. Why did the previous change miss expectations?

The already rejected executable now has an executed-path explanation:

| Changed native work / iteration | Reference | Rejected | Delta |
|---|---:|---:|---:|
| Cache accessor self | 70 | 72 | +2 |
| New outlined global-index projection helper | 0 | 24 | +24 |
| run_fiber_inner self | 1123 | 1118 | −5 |
| All other function-self buckets | unchanged | unchanged | 0 |
| **Whole iteration** | **2173** | **2194** | **+21** |

The wide copies disappeared, but the replacement accessor/helper cost grew by 26 instructions while the caller saved only five. This accounts for the prior whole-batch regression: **21 × 1,000,000 = 21,000,000**, versus measured **21,000,145**; residual **145** instructions. Native inspection shows result preservation and loss of the original guard-drop tail jump, but the ledger does not allocate every one of the +26 to those individual submechanisms.

There was no valid numeric savings prediction before that earlier implementation. We can now explain its outcome, not pretend that this was an ex ante prediction. The candidate remains rejected and no variants were tried.

## Evidence ledger and decision

| Claim | Label | Evidence / limit |
|---|---|---|
| Baseline ~8.55x instruction gap | Measured | Existing validated scoped counters; not a new timing ratio |
| 2173 reference logical instructions on selected cycles | Executed-path observed | Four iterations, two states; REP handling explicit; debugger perturbation |
| 49 fiber-index calls/cycle | Executed-path observed | Every call and callee path recorded; same four cycles |
| Node ordinary paths 205/230/247 | Executed-path observed | Six iterations at three states; excludes other runtime cycles |
| Node batch residual ~3.3% | Model residual | Transition frequency/control assumptions explicit; not wholly attributed to GC |
| Repeated header lookup opportunity 154 | Conditional removal model | Exact existing bundles; replacement overhead unquantified |
| Old candidate +21/cycle | Executed-path observed + measured agreement | Already preserved rejected executable; no new variant |

**Decision at completion of this pass:** attribution/mechanism materially improved; no product candidate yet passed the full net-prediction gate. The proposed next question was a scoped header execution-state design, not another cache micro-tweak. **Subsequent selection review:** [10-candidate-selection.md](10-candidate-selection.md) compares that scope against larger alternatives and recommends explicit unbound-local state, with a conditional pre-implementation model. The 154-instruction header opportunity above remains valid gross accounting, not the selected net prediction. Diagnostic budget is exhausted; further debugger sessions, measurements or implementation require a new bounded decision.
