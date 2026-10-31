# B1 implementation and compiled-mechanism validation

2026-10-30. **One approved candidate implemented and tested; no instruction-counter or timing outcomes run.** Worktree `matchbox-checksum-baseline-20261030T2203Z`, branch `investigation/checksum-baseline-20261030T2203Z`; uncommitted patch on582473adb6eb643f35ec1fe2e21da37520fb3ad0. Main checkout/local develop are unchanged. This is not a delivered performance-result claim.

Read [design D1](12-active-context-design.md) and [pre-implementation B1-P1 v1](15-b1-pricing-prediction.md). P1 is preserved byte-for-byte. A compiled-mechanism **B1-P2 v1** was recorded2026-10-30T23:43:15Z, before any outcome counters. Source/static inspection found more favorable code than P1 forecast; this report does not rewrite that earlier prediction.

## 1. Implemented boundary and semantic authority

Changes are limited to `crates/matchbox-vm/src/vm/mod.rs`, `vm/scopes.rs`, `vm/scopes/unbound_tests.rs` and `tests/integration_tests.rs`:

- Acquire a checked `&mut BxFiber` AFTER native completion draining and the existing fiber-index bounds guard. Keep frame-empty/yield checks and every original bytecode/polling boundary. End the view before cold whole-VM reload operations and reacquire afterwards.
- Reuse that view in callback-free local/global/ADD/loop paths. Whole-VM miss, array equality/string coercion, callbacks, exceptions, JIT entry and other handlers still use their authoritative VM paths after the view ends. CONSTANT/read_constant remains untouched as a whole-VM boundary.
- Move the ONE local resolver/setter implementation onto the borrowed fiber with explicit heap/shapes/interner arguments. VM accessors delegate to it for other consumers. There is no duplicate Unbound-only interpreter, context packet, unchecked lookup or detached fiber. Shared struct spelling/write-barrier primitives and numeric conversion likewise accept the fields they need; existing VM consumers use the same primitives.
- Preserve Binding/Stack/Shared/Unbound admission and exposure rules, bound dictionary authority, parameter misses, deletion fallback, present null, spelling/case sensitivity, shape invalidation, local stack bounds and GC barriers. No BIF or canonical-coercion compatibility repair is mixed in.

The compiler, bytecodes, loop shape/iteration count, arithmetic and value/frame layout are unchanged. The workload still has MatchBox's global `sum`, versus Node CommonJS wrapper-frame locals. No source wrapper or storage relocation manufactured parity.

### Borrow/root/pointer review

The view remains in `VM::fibers`; normal GC scanning sees its stack, frames, promotions and root_stack. No `mem::take`, detached roots, boxing or assumed stable Vec address. Rust field borrows delimit VM reentry; no context view survives the next completion drain.

The old persistent raw `promoted_ptr` cannot simply be assumed compatible with new mutable views: it was removed. Its two consumers now use checked current-frame promotion access. Frame relocation, aliasing and bounds do not rely on a previous pointer's physical address remaining unchanged.

The old header `locals_ptr` was also removed. Both JIT stack-pointer consumers derive a fresh pointer immediately at native invocation, AFTER sync_jit_locals. No pointer crosses a preceding stack-growing opcode/synchronization/reentry boundary. The supported compiled body/iterator invocation and post-call synchronization remain unchanged. The existing raw code_ptr still refers to the Rc-owned immutable bytecode allocation, not a borrowed fiber/frame/stack element; its existing frame_changed reload discipline is preserved.

Debugger inspection takes the whole VM, so the feature-enabled build releases/reacquires the view around it. Default release candidate has no debugger feature. Interrupt/GC/completion/yield ordering is not intentionally reduced or moved. No Miri/formal verification or Wasm/ESP32 target validation was performed; the lifetime claim is scoped Rust/type/source review plus the contract tests below.

## 2. Correctness validation

All run offline in the pinned cached Rust builder, serialized and logged. No tests were weakened or new ignores added.

| Suite/configuration | Passed | Existing ignored |
|---|---:|---:|
| VM library, jit+debugger | 69 | 0 |
| VM library, no JIT/default features | 69 | 0 |
| Full integration suite, default features | 196 | 0 |
| Full BoxLang transfer compatibility suite, default+debugger | 543 | 29 |
| Web runtime, default+debugger | 40 | 0 |
| Selected non-JIT integration contracts | 17 | 0 |
| Root library + host lifecycle/namespace/request/route-cache/string-rope | 46 | 0 |
| **Total top-level passes** | **980** | **29** |

The integration suite's internal native-fusion child-test pass is not counted twice. Non-JIT integration filtered147 unrelated tests; it was not a second full integration run. This was not `cargo test --workspace` across every ancillary target.

New tests exercise explicit borrowed access after stack/frame/fiber capacity growth and GC, live shared aliases/shape changes/deletion fallback/null, and parameter read-only lookup versus write-time spelling/case rules. Capacity growth can reallocate in place; these tests do not prove every owner physically moved in each run. A new compiled-source integration case grows320 locals in each of25 recursive frames, queues64 producer fibers and waits through nested native map/future.get callbacks, then checks all frame-relative sentinels. It passes with and without JIT features.

Existing coverage includes bound/unbound scopes, closure/argument exposure, exceptions/finally, GC/OOM, stack reallocation, JIT loop/function/iterator/deopt/OSR contracts, resumable debugger line/budget behavior, native completion/cancellation, lifecycle roots and namespace/cache isolation. These are correctness tests, not performance comparisons; incidental test-run durations are not outcome measurements.

## 3. Candidate provenance

ONE identified release candidate: `benchmarks/instructions/results/b1-implementation-20261030T225918Z/instruction_interpreter`.

- Executable SHA-256: `9af288307e2705365708940eec170e790c33c5ff94dd88c0e65829bd6cb14741`.
- Product/test patch SHA-256: `8f98810941fb7601f11a89be4c1d0db8362c0f5d23724f2a0c94e995aff283fd` (`candidate.patch`). No product changes after that build.
- `cargo build --offline --release --example instruction_interpreter`; original default features, opt-level=z, LTO, one codegen unit, panic=abort; `RUSTFLAGS=-C strip=none` as the baseline.
- Cached builder `docker.io/library/rust@sha256:f31fa9eaac4e417505e10009786ec0c636558e02dff3fa662cc92c10894ba065`, Rust1.98.1 / LLVM22.1.8. New `target/b1-build`, never baseline-build. Build-generated src/stubs.rs restored, same preexisting packaging asset policy as baseline.
- Baseline executable remains `abe03005c7464ecb3b396d8844a06e6692522ce5c36a5700e1c43a0dd6a73d11`. Prior baseline/static/design/P1 packets and source/compiler/workload identity are retained separately. Git revision alone does not identify the modified candidate.

Necessary test builds included native-fusion correctness artifacts. They are not optimization variants or additional measured candidates. No tuning loop, new benchmark/profile/debugger session, installs/security changes, commit or push.

## 4. Artifact-confirmed conditional native mechanism

This is a STATIC ordinary-cycle model using reviewed continuing-loop state assumptions, **not newly observed execution/frequencies or hardware counts**.

| Disjoint executed-operation model | Baseline | Candidate | Reduction |
|---|---:|---:|---:|
| run_fiber_inner self | 1058 | 848 | 210 |
| All contributing helper self | 761 | 575 | 186 |
| **Ordinary cycle** | **1819** | **1423** | **396** |

Acquisitions meet the **41→10** target: seven in headers, two nested inside read_constant and one CONSTANT result-push reacquisition. There are no ordinary-path local projection/adapter/packing calls. CONSTANT's handler+helpers stays150 (excluding its header). Locks/cache representation,80-byte cache copy, numeric conversion and stack/value helpers were not replaced.

All13 contributing helper bodies have identical normalized operation sequences after relocation and the intentional numeric_value→numeric_value_in rename. The numeric helper even has the same promoted field ABI/code as before; its numeric fast path remains22 instructions. Selected historical helper offsets were mapped and checked, including the tail-jump drop/poison closure, loaded data assumptions, the actual six candidate dispatch-table targets and the f64 increment constant. Panic-location arguments legitimately carry changed source metadata; valid-path checks do not dereference it. This is not whole-binary or all-path equivalence.

Every selected main conditional branch has an explicit source/state reason in native-branches.json; the full main PCs/calls/helper closure are saved. Non-1024th GC checks, warm promotion/cache hits, finite f64 operands, no frame reload/yield, JIT inactive, no timeslice and trace absent are inherited/reviewed assumptions, not a new debugger trace. Periodic/cold/boundary/runtime residuals remain separate.

## 5. P1 reconciliation and pre-outcome B1-P2 v1

P1 predicted223–331 fewer instructions/cycle. The verified compiled model says396: **65–173 more reduction than forecast**, NOT a measured prediction success.

The helper delta is exactly the intended31×6=186. The main delta210 exceeds P1's planned31 calls+121 argument writers=152 by58. A disjoint bridge, with PCs/frequencies in prediction-B1-P2-v1.json:

| Native change beyond/within P1 gross | Reduction/cycle |
|---|---:|
| P1 planned call/helper/setup removal | 338 |
| Remove unused per-dispatch JIT pointer preparation | +21 |
| Remove cached promotion-pointer spills/restores/read | +15 |
| Remove redundant code-pointer state store | +7 |
| Introduce explicit active-view preservation copy | −7 |
| Introduce checked current-frame derivation for loop limit | −10 |
| **Unassigned net other main-code change** | **+32** |
| **Conditional net** | **396** |

The32 is a NET difference after all other replacement/surrounding work in the fully counted candidate. It is not32 freely removed instructions, proof of zero new cost, or a claim all register/layout effects are understood. Other preservation moves, caller/result organization and control-flow reshaping are included in848 but not fully causally partitioned. P1's0–49 growth allowance was a weak planning assumption, not an empirical compiler bound; a favorable total does not retroactively prove each of its replacement cases. P2 replaces that coarse forecast with the complete conditional compiled bill and keeps the causal residual explicit.

**B1-P2 v1 forecast:** approximately396 million fewer instructions for the unchanged million batch, around**1.423 billion**, conditional on this ordinary-path model extrapolating. That is**21.77% of the fresh MatchBox baseline /25.31% of the fresh comparator gap**. The point using an unchanged residual would be1422993839, but the residual has NOT been proved unchanged. No invented confidence interval, guaranteed bound, elapsed-time prediction or measured performance claim.

### Retained/displaced costs and limits

Cold reload still releases/reacquires; obsolete promotion-pointer initialization disappears. Cold misses/array/string/reentry paths can reacquire through the whole VM. Their exact net bills and periodic/final/marker changes have not been fully priced. Future raw counters must include them without overhead subtraction.

JIT pointer work is derived at actual active invocation rather than every interpreter dispatch; its acquisition/derivation costs are displaced to those users, not globally eliminated. The debugger-feature build also reacquires around inspection, including when its debugger state is inactive. No JIT/debugger/other-workload or ESP32/Wasm performance prediction is made. Explicit local field interfaces can change bound/Shared-path preparation; functional coverage is not a performance guarantee there.

The unchanged instruction marker policy may include fixed reload/exit costs. Do not count work moved before START as whole-process saving. MatchBox global versus Node wrapper-local storage remains an acknowledged comparator difference. Instructions are not elapsed time.

## 6. Decision and remaining approval gate

Keep ONE validated candidate in the dedicated worktree **pending outcome validation**. No fallback candidate, retuning or optimization loop is authorized. P1 and P2 are frozen before counters; main checkout remains untouched.

Proposed next, separately approved, bounded outcome phase: at most30 minutes, no new builds, three instruction and three separate unprofiled timing processes for EACH of baseline MatchBox, candidate MatchBox and native Node --jitless (18 workload processes). Keep exact source/iteration/storage/checksum/marker/thread policies, serialize measurements, preserve every raw failure and inspect counter validity. Fresh baseline/Node controls check environmental drift. Then reconcile against P1/P2, including the32-instruction causal residual and any whole-batch residual, before a metric-based retain/reject decision.

Evidence packet: `benchmarks/instructions/results/b1-implementation-20261030T225918Z/` (ignored/local, not transferred by Git documents alone). Scripts, logs, original parsing/normalization failures/refinements, source/patch/binary hashes, tests, selected paths, helper/data/dispatch checks, original P1 and compiled P2 are preserved there.
