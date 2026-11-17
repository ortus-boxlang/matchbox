# Five-candidate handoff and two-design comparison — D1

Status: **three future handoffs saved; two deeper source/static designs complete; no product implementation selected or performed**. 2026-10-30. This is not an optimization loop. The user approved handoffs plus an at-most45-minute design/pricing pass, not builds/workload runs or implementation.

## Deliverables and stable entry points

Deferred, credible candidates:
1. [Header-only current-fiber reuse](next-candidates/header-fiber-reuse.md)
2. [Promoted-constant access](next-candidates/promoted-constants.md)
3. [Global-cache ownership/access](next-candidates/global-cache.md)

Investigated more deeply:
4. [Active execution context B1](12-active-context-design.md)
5. [Local-plus-constant composition C1](13-expression-composition-design.md)

All five share [current baseline, artifact provenance and future-run rules](next-candidates/README.md). They are a queue of opportunities, not five guaranteed independent improvements. In particular, B1 can subsume header-only reuse and C can subsume parts of constant reuse. After each retained change, refresh the remaining ledger; do not add stale savings or automatically start the next handoff.

## Fresh baseline retained unchanged

Local develop/source582473adb6eb643f35ec1fe2e21da37520fb3ad0; current MatchBox binary `abe03005c7464ecb3b396d8844a06e6692522ce5c36a5700e1c43a0dd6a73d11`. Same direct checksum500001500000; interpreter asserted JIT-inactive. Node v22.23.3 --jitless; native same host; different VM-global versus CommonJS-frame sum storage retained.

| Metric | MatchBox | Node --jitless |
|---|---:|---:|
| Fresh aggregate retired instructions, median / MAD | 1818993839 /131 | 254279176 /1532 |
| Batch-normalized instructions/iteration | 1818.993839 | 254.279176 |
| Separate loop ms median / MAD | 118 /0 (raw118,118,132) | 13 /1 (raw12,13,16) |

Current comparison gap1564714663/batch. All prior samples/commands/counter timing/output checks remain in the baseline packet. No new target process or measured sample was run in D1. Counter ratio is not elapsed-time speedup; normalization is not isolated arithmetic cost.

## What deeper inspection changed

### B: a bounded ownership design, not a seven-acquisition promise

A per-dispatch checked fiber borrow can span callback-free handling, but must end before completion draining, frame reload, callback/reentry, cold whole-VM operations and JIT execution. Do not detach roots, cache arbitrary raw addresses or rely on reserved Vec headroom.

The existing local accessors' scope semantics can be shared through explicit fiber/heap/shapes/interner inputs, rather than duplicating an Unbound path. Their struct_set is the ordinary field write path, not the callback-bearing struct_assign path. Number conversion needs its heap, not an arbitrary &mut VM. Those are source-supported field boundaries, not a compiled Rust/type proof.

Keeping CONSTANT's current whole-VM read_constant untouched makes the plausible first scope **41→10 acquisitions**, NOT41→7: seven headers, two nested constant-helper acquisitions, one post-constant reacquisition. That targets **217 call-plus-helper instructions** before caller savings, context/interface work, surrounding growth or cold displaced work.

### C: a concrete literal-expression design, with a missing observation policy

A general local-plus-numeric-literal opcode can retain a dynamically typed left and generic addition. It must reuse the compiler's exact existing identifier-resolution choice and retain coercion/fallback/root semantics. A three-word preboxed literal could eliminate runtime generic constant access, but must preserve BxValue::new_number bit/NaN behavior and price decode/compiler/consumer work.

Two fewer bytecode dispatches do NOT prove two free header removals. Native completion callbacks, GC/rooting, interrupts and debugger pause/budget behavior occur at these boundaries today. A counter weight3 is not three observation opportunities. A phase-aware or guarded equivalent needs explicit resume/IP/root state and priced checks; merely dropping boundaries was not approved or proven equivalent.

JIT/construct width, stack-effect, eligibility and translation consumers also need coherent support; inactive JIT in the workload is not permission to disable existing JIT capabilities for other users. Canonical Plus source/tests were checked read-only; current MatchBox's broader nonnumeric fallback/representation differs, so this pass makes no new JVM-parity claim and performs no arithmetic compatibility repair.

## Net pricing gate: gross numbers versus replacement

| Scope | Current conditional gross bill | Explicit net equation / unresolved terms |
|---|---:|---|
| B1, keep constant boundary | 217 call/helper instructions targeted | `217 + E_caller/frame − I_context/interfaces − G_surrounding − D_displaced` |
| C1b, conservatively retain old operand/ADD body work | 283 removed header+read_constant-subtree instructions | `283 − L_literal − R_observation/resume − G_codegen − D_displaced` |

For B1, caller preparation cannot be blanket-counted as31×4: some setup feeds retained frame/vector/result work. Active references are not free merely because they allocate no heap memory; live ranges, spills, field interfaces and branch layout can reverse net savings. For C, retaining the original headers in full would reduce that skeleton's gross283 to101; a coherent operand redesign could save other work, but its net is unknown, not zero. These scopes overlap with each other and the handoffs.

**Neither equation has every term priced. No P1 numeric whole-system savings prediction has been recorded.** Zero replacement gives B217 about11.93% of baseline/13.87% of gap, and C283 about15.56%/18.09%. These are conditional gross normalizations ONLY, not forecasts, confidence bounds or reasons to pick the larger number.

## Selection rationale and remaining unknowns

**Prefer B1 for the next mechanism-cost question under unchanged contracts**, not as a proved globally best optimization and not as approval to edit product code. It removes repeated addressing at existing semantic boundaries, with an explicit source ownership sketch. C's potentially larger benefit requires a less resolved observation/resume and bytecode-consumer design. C remains credible; it is not rejected or assigned zero benefit.

Why this is not simply the easiest patch: the easiest narrowly counted header scope was compared and deferred, while B1 includes the shared local/numeric/caller ownership needed for broader state reuse. We did not choose it merely because header637 is the largest visible subtotal, nor choose C merely because283 exceeds217. Net whole-system benefit and contract-preserving effort, not gross envelope size, must determine implementation selection.

Ranking-changing unknowns:
- Can the field-borrow scope interface remain one semantic implementation without introducing projection/result/outline machinery or repeatedly dropping the active borrow?
- What actual pointer preservation, caller simplification and surrounding code generation follow? The previous copy-projection regression and Unbound header growth make arbitrary spill allowances inadequate.
- What polling/debugger/host observation contract should C preserve, and what does the legal phase/guard/resume mechanism cost? A decision that permits a different observation unit could substantially change C's ranking, but is not implicit authorization here.
- Do the current compiler/JIT consumers admit a compact coherent C representation without lost capabilities? New decoding checks/code size/root state must be included.

The budgeted **static-only** phase has answered ownership/surface questions and exposed these cost blockers; repeating the same source pass will not measure register allocation or new surrounding native paths. If cost discovery needs a compiler experiment, it must be labeled and separately approved as a bounded diagnostic, not represented as an already-priced product optimization. The original requirement for a defensible pre-implementation prediction remains in force; this report does not waive it or authorize disguising implementation as research.

## Validation / approval boundary

No product implementation recommendation is ready for approval on net savings yet. Before any later optimization implementation: close the ranking-changing costs, write a dated/versioned P1 with all replacement/displaced terms, then request one bounded implementation/build/test/native-mechanism validation scope. Preserve P1 and version any compiled-path refinement BEFORE outcomes. Tests and release/native feature/profile provenance must remain equivalent. Outcome instructions and time are separate, explicitly budgeted runs, followed by reconciliation; no automatic variant or next candidate.

B/C documents specify the contract tests and falsification checks. No test was weakened or run in this source-only phase. New docs are uncommitted on the dedicated branch; local develop and the old reference/evidence remain untouched. Raw D1 artifact packet: dedicated worktree `benchmarks/instructions/results/design-comparison-20261030T223026Z/` (plan, pricing script/JSON, provenance/dependency/document hashes and final state). D1 is an investigation deliverable, not a performance improvement.
