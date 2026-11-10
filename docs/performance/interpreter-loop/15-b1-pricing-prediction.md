# B1 pricing and pre-implementation prediction — B1-P1 v1

Recorded **2026-10-30T22:53:44Z, BEFORE any B1 product implementation, candidate build or outcome measurement**. Status: conditional engineering prediction for review; no product edit. This prediction is named B1-P1 to distinguish it from the historical Unbound P1/P2. Preserve this version; append explicit revisions rather than overwrite it.

Read [B1 design D1](12-active-context-design.md), [selection D1](14-design-selection.md) and [baseline/provenance](next-candidates/README.md). D1 correctly stopped without a net forecast. This pass adds a per-site caller audit and explicit replacement cases; it does not retroactively claim those terms were priced in D1.

## 1. Scope and prediction type

B1 is the same checked per-dispatch active-fiber design: no new opcode, arithmetic/storage change, detached fiber, GC-root relocation, cache-ownership change or reduced polling cadence. CONSTANT/read_constant remains a whole-VM boundary. Ordinary-path acquisition target is **41→10**, with cold release/reacquisition and noneligible paths preserved.

Current source582473adb6eb643f35ec1fe2e21da37520fb3ad0, current executable SHA-256 `abe03005c7464ecb3b396d8844a06e6692522ce5c36a5700e1c43a0dd6a73d11`; current conditional ordinary cycle1819. Fresh million-batch median1818993839, Node --jitless254279176, comparator gap1564714663. Scope is retired user-space instructions in the same marked direct loop, not arithmetic-only cost or elapsed time.

**This is a conditional engineering planning range, NOT a measured saving, confidence interval, guaranteed bound, or claim that future compiler output is known.** Important code-generation assumptions below can falsify it. There is no timing prediction. This pass authorizes no implementation or measurements.

## 2. Newly audited eliminated work:338/cycle

D1 credited only31 call/helper pairs×7=217 and explicitly left caller work unpriced. The new audit selects individual instructions preparing the removed acquisitions' RDI/RSI/RDX/RCX arguments, including panic-location metadata. It does NOT count frame-base construction, coupled index spills, numeric/result preservation, or hypothetical CSE gains.

| Current path | Targeted calls/cycle | Call + helper self | Audited caller setup | Total gross |
|---|---:|---:|---:|---:|
| Header, second/third acquisition ×7 | 14 | 98 | 56 | 154 |
| GET_GLOBAL | 2 | 14 | 8 | 22 |
| GET_LOCAL | 3 | 21 | 11 | 32 |
| Two ADDs | 6 | 42 | 24 | 66 |
| SET_GLOBAL_POP | 2 | 14 | 8 | 22 |
| FOR_LOOP_STEP | 4 | 28 | 14 | 42 |
| **Total** | **31** | **217** | **121** | **338** |

The121 is NOT blanket31×4. Three less-local/coupled index-argument preparations stay charged, along with associated spills/reloads and VM/frame-base setup. Shared source values, index/error checks, actual stack/value accesses and result preservation remain charged. No credit for removing ADD's double-result spills or for reusing frame addresses.

`caller-audit.json` lists every credited PC/assembly instruction and execution multiplier. PCs are current selected static-path evidence; provenance/use review is not a general x86 liveness theorem. The prediction requires the replacement to remove these source acquisition arguments without needing them elsewhere. Compiled review must check that, not assume the121 survived.

## 3. Explicit replacement budget:7–66/cycle

Two concrete code-generation cases bracket the modeled state/interface work:

| Introduced work | Register/reuse case | Stack/reconstruction case | Basis / assumption |
|---|---:|---:|---|
| Preserve active fiber pointer | 7 | 38 | One pointer copy/dispatch; or7 stack stores plus31 reloads replacing removed acquisitions. Existing consumer/result instructions stay charged. |
| Prepare scope heap/shapes/interner references | 0 | 9 | Shared local accessors remain inlined; at most3 extra field-address constructions×3 local accesses. Unused field prep can disappear on Unbound path. No outlined context adapter/packet. |
| Prepare numeric helper heap reference | 0 | 4 | Up to1 extra address instruction×4 conversions; keep existing numeric helper fast-path bill. |
| Checked loop-limit frame/promotion access | 0 | 15 | Reuse valid current frame; or charge a full existing13-instruction frame fragment +1 pointer reload +1 field-address construction. No credit for the old cached-pointer load. |
| **Total introduced I** | **7** | **66** | Conditional source/native-fragment cases, not observed B1 code. |

The13-instruction frame fragment is preserved in `frame-fragment.json` (current0x41da5d through0x41da95). It deliberately includes work not required merely to locate the promotion field, providing a conservative fragment reservation. It is not an assertion that a new frame accessor compiles to exactly15.

Thin local borrows, shared inline scope semantics and direct field interfaces are part of this mechanism. A wide context object, projection Option/result packing, newly outlined accessors, extra acquisitions or more reloads are NOT silently covered by these cases. They invalidate/reprice the model before counters. The active view and existing raw promoted_ptr/locals_ptr require an alias/provenance proof; checked rederivation costs are included rather than assuming unchanged physical address is enough.

## 4. Surrounding work G: explicit0–49 planning allowance

This is the **weakest assumption**, not an independently established upper bound. In the previous Unbound change with the same compiler/profile/interpreter, the shared header grew7 instructions per dispatch×7=**49**; other work offset5, for net+44. Here the FULL49 positive bucket is used as an additional surrounding-code risk allowance, with no credit for beneficial collateral changes.

This historical event is calibration for a compiler-risk assumption only. It is NOT a current cost ledger, sample distribution, or evidence that B1 cannot grow more than49. A broader interface/liveness change might exceed it substantially. B1's G excludes pointer/interface/frame work already counted in I; actual candidate instructions must go into disjoint buckets, not both I and G. Negative collateral changes also require reconciliation, not an opportunistic rewrite of P1.

Unlike an arbitrary undifferentiated spill reserve, the known pointer/interface costs are itemized above. Remaining compiler uncertainty is visible here. **If native inspection finds G>49, or any introduced cost exceeds its case, stop and preserve P1: explicitly revise/reselect BEFORE outcome counters.** Do not make the assumption disappear by calling all growth unavoidable/retained work.

## 5. Net prediction and independent accounting check

`net reduction =338 − I(7…66) − G(0…49) =223…331 instructions/ordinary cycle`

| Quantity | B1-P1 v1 forecast |
|---|---:|
| Current ordinary cycle | 1819 |
| Candidate ordinary cycle | **1488–1596** |
| Approximate million-batch reduction | **223–331 million instructions** |
| Reduction / fresh MatchBox baseline | **12.26–18.20%** |
| Closure / fresh comparator gap | **14.25–21.15%** |
| Approximate candidate batch | **1.488–1.596 billion instructions** |

The batch forecast extrapolates the ordinary-cycle delta. It does not claim startup, cold/miss/periodic/final/marker work disappears or that the existing −6161 residual remains unchanged. Exact boundaries must remain the same and raw total instructions must be measured without overhead subtraction.

A separate disjoint native view checks arithmetic, not a second opportunity:
- run_fiber_inner self:1058 −31 calls −121 caller instructions +I+G =**913–1021**.
- Fiber-index helper self:246 −31×6 =**60** (ten retained calls).
- Other helper self provisionally unchanged:**515**.
- Total:**1488–1596**. This overlaps the main equation; never add the views.

This partition provisionally assumes the ten retained acquisitions stay outlined at their current six-instruction helper path. If compilation inlines, folds or renames helpers, remap the executed path before comparing buckets; symbol disappearance is neither a free saving nor proof of failure. Any changed pricing is a documented pre-outcome revision, not a rewrite of P1.

## 6. Retained, displaced and other-user costs

Retain ten fiber acquisitions (including all three associated with CONSTANT), local/frame/value bounds, binding/exposure semantics, stack/value operations, locks/namespace checks, generic constant capture/promotion, numeric/coercion/result behavior and all GC/completion/yield/debug/trace/interrupt/JIT boundaries. Preserve values in the same roots.

Cold frame reload releases/reacquires and can add work. The root reload happens before START here; moving a cost there is not an end-to-end saving. Global-cache and loop-limit cold misses can require checked reacquisition; charge those fixed paths at compiled review and retain them in hardware totals. Noneligible/bound/Shared/JIT/callback consumers may gain field preparation or cold reborrows; no performance claim for them. Ordinary-cycle code may have different periodic/boundary residuals; do not silently assign them or hide failed assumptions behind the small historical residual. No numeric prediction for whole-process/compiler time, other workloads or ESP32/Wasm performance/resource use.

## 7. Selection rationale after pricing

The user chose B1 for pricing. Conditional on this mechanism, even its223 lower forecast exceeds the header-only154 zero-replacement fragment opportunity, while preserving bytecode/polling organization that composition still has to resolve. B1 addresses a coherent repeated-addressing layer, not simply the easiest redundant instruction. The separately deferred constant/cache work remains out of scope except for required field/lifetime plumbing.

Composition could still have a larger net result; its observation/resume/consumer work is unpriced, not zero. This is a best-supported bounded next choice under current constraints, not proof of global optimality. If B1 fails its lifetime or compiled cost gates, return to review rather than automatically start C or another variant.

## 8. Proposed implementation/validation approval boundary

**Pending approval:** at most90 minutes for ONE B1 implementation, necessary correctness/test builds plus ONE identified same-profile release candidate, contract tests and static compiled-path review. Mechanical compilation fixes within that design are not permission for tuning variants. Use new separate target/artifact paths, never overwrite the baseline build outputs or binary, and keep all existing evidence untouched. Record the candidate patch/hash, toolchain/flags/features, build command and executable hash rather than equating an unchanged Git revision with binary provenance. No outcome-counter or timing workloads, renewed debugger/profiling, installs/security changes, commits/pushes or loop in that proposed scope.

Before any outcome measurement:
1. Prove borrow/root/pointer boundaries, no detached fiber or unchecked path, no duplicated local semantics. Review callbacks, frame reload, stack/frame/fiber relocation, JIT and constant/cache miss exits.
2. Run the B1 contract suite specified in12, including GC/OOM, bound/unbound alias/exposure/deletion/null/spelling, nested yielding callbacks, exceptions/finally, lifecycle/cache isolation, JIT-feature/non-JIT and debugger contracts. State unavailable target coverage; no test weakening.
3. Verify current conditional branch assumptions against source/tests and candidate code, and compare complete selected helpers/caller/ordinary cycle. Confirm target calls10, caller elimination121, retained helpers515 or explicitly price changes, I≤66 and additional disjoint G≤49. Count new copies/adapters/spills, not only absent helpers.
4. Preserve B1-P1 v1. Record dated P2 if compilation revises pricing BEFORE counters; stop/review if the original mechanism or net rationale is unsupported. No source-tuning variant automatically follows.
5. Only a separately authorized outcome phase may run fresh comparable instruction and unprofiled timing samples, preserve raw failures and reconcile predicted versus measured effects and residuals. No timing prediction or inference from instructions.

Artifact packet: dedicated worktree `benchmarks/instructions/results/b1-pricing-20261030T224806Z/`. `prediction-B1-P1-v1.json` is the frozen machine-readable original; scripts/audits/frame fragment/logs/input and document hashes preserve derivation and the pre-implementation state. No delivered performance improvement is claimed yet.
