# Active execution context: design/pricing D1

Status: **source/static design only; preferred next mechanism frontier, not approved implementation or a numeric savings prediction**. 2026-10-30. Read [current baseline and rules](next-candidates/README.md), [composition competitor](13-expression-composition-design.md) and [selection](14-design-selection.md). No product code changed and no new builds/workload/profile/debugger sessions ran.

Follow-up: [B1-P1 v1 pricing](15-b1-pricing-prediction.md) subsequently itemizes caller elimination/replacement cases and records a conditional engineering forecast. The D1 unpriced state below is preserved as the earlier decision record, not retroactively rewritten.

## 1. Focus and current account

Keep the original compiler, bytecode, arithmetic, storage, roots, JIT configuration and every existing dispatch/polling boundary. Replace repeated acquisition of the same fiber with a short-lived checked borrow, not a second interpreter or a raw pointer cached across callbacks.

Source baseline582473a, executable SHA-256 `abe03005c7464ecb3b396d8844a06e6692522ce5c36a5700e1c43a0dd6a73d11`. Verified current conditional cycle1819, fresh median1818993839, comparator gap1564714663. These identify the evidence; future binaries need revalidation.

Current ordinary cycle has **41 fiber-index helper calls**: 21 in seven headers, 20 in handlers/nested helpers. Call + callee self is **287**, before caller preparation/frame reconstruction. This is not a 287 saving. Header637 and other inclusive views overlap that subtotal.

The earlier ideal of seven acquisitions/cycle implicitly required refactoring read_constant's whole-VM interface. That is not free and overlaps the separately deferred promotion work. The concrete first scope below **retains that boundary** instead of pretending it vanished.

## 2. Source-confirmed ownership and invalidation

| Boundary | Evidence | Consequence |
|---|---|---|
| Native completion draining | vm/mod.rs2259–2307: resolves futures, may invoke handlers/Wasm thunk with VM | Acquire active borrow AFTER draining. Never retain it across draining. |
| Nested synchronous callbacks | 7884–7955: enqueue new fiber, recursively run scheduler, possibly swap_remove on return | Stable logical index is not stable Vec address. End every fiber/frame/stack borrow before call/reentry. Reacquire checked state afterwards. |
| Scheduler | 4095–4267: running flag, deferred sweeping; run_other_fibers skips suspended callers | Preserve running/finished/future identity and index discipline. Do not detach the active fiber from VM storage. |
| Frame reload | 4333–4360: prepare_unbound_frame, ensure_caches, Rc/RefCell state, reserve | Cold reload is an explicit release/reacquire boundary; ordinary no-reload path need not reconstruct three fibers. |
| Stack growth | 4362–4367; Vec push/reserve | A borrow of the fiber object does not make its stack buffer stable. Re-derive stack bases; never retain an element pointer over a push/reserve. |
| GC | 9620–9667 enumerates VM fibers/stacks/root_stack/frames/promotions; gc_alloc9677 can collect | Values stay in the same rooted stack/frame. No mem::take/detached active fiber or unrooted Rust-local operand surviving a collecting call. |
| Local bindings | scopes.rs130–320: arguments, binding indices, Unbound/Stack/Shared transitions | Preserve dictionary authority, checked fallback, exposure, write barriers, key/interner rules and bounds. |
| JIT entry | FOR_LOOP_STEP and other JIT consumers | End/reborrow before raw JIT execution; derive pointers with a documented provenance/lifetime. Do not assume inactive JIT makes feature consumers irrelevant. |
| Trace/debugger/interrupt | mod.rs4378–4492; debugger budget/line tests in web_runtime_tests | No new opcode or dispatch removal; preserve boundary, IP, counter and interrupt ordering. |

## 3. Concrete scope B1: checked per-dispatch borrow, delimited VM calls

This is a layer-removal design, not the proposal to keep arbitrary raw state for an entire quantum.

1. Preserve the native-completion check/drain and bounds behavior. Acquire a checked `&mut BxFiber` in the shared header, retaining frame-empty/yield checks. Use it for stack-base refresh and callback-free handling in the SAME dispatch.
2. On frame_changed, release before existing whole-VM reload operations and reacquire after them. These are cold costs, not zero. No assumption that frame_changed always stays false outside the benchmark.
3. Use that borrow through existing GET_LOCAL, GET_GLOBAL, SET_GLOBAL_POP, ADD and FOR_LOOP_STEP ordinary callback-free paths. Keep existing opcode match and semantic branches. Every path about to call whole VM code ends the borrow. Other opcode handlers remain authoritative, not a parallel fast interpreter.
4. Refactor shared local-access semantics to take the active fiber plus the actual required heap/shapes/interner fields. Source audit of arguments_scope, slot_binding, binding_index, local_value/set_local_value shows no need for arbitrary VM callbacks in those accessors. Do not duplicate Unbound-only logic in a second handler implementation or add an Option/result projection adapter. Existing scope tests and JIT accessors must use the SAME authority.
5. Numeric conversion can accept the heap it actually reads; its numeric-string/error behavior is unchanged. Avoid a broad mutable VM argument that forces ending the active borrow on the numeric path. Its existing number fast path does not touch heap.
6. Global cache hits need fiber/chunk plus namespace/global_values, not arbitrary mutable VM access. Keep the lock/cache representation and miss behavior. Release before an unchanged whole-VM miss path, then reacquire for any subsequent stack write.
7. **CONSTANT remains a whole-VM read_constant boundary**, including its existing closure-first/cache behavior. End the borrow before it and reacquire for pushing its result. This intentionally does NOT implement the deferred constant proposal.
8. End all borrowed views before the next dispatch's completion drain. Cache no frame/stack pointer across dispatches. Do not add RefCell/Rc/Box allocations, a multi-pointer result packet or a borrow guard to the ordinary path.
9. The old raw promoted_ptr/locals_ptr and a new active mutable fiber view need a joint alias/provenance proof. Prefer checked frame/promotion access through the current borrow while it is live, and derive JIT raw pointers only at the legal released/reborrowed boundary. Any repeated frame access or pointer derivation introduced by this requirement is charged to I/G; do not reuse an older raw pointer merely because physical storage did not move.

This is a conceptual ownership sketch, not Rust-compiled proof. Prefer ordinary local field borrows; introduce a private access component only where genuinely needed to keep one shared scope implementation. Constructing/copying a wide context structure every dispatch is an introduced cost, not an assumed optimization. A reference's zero allocation does NOT imply zero native instructions.

### Why not detach or box everything?

Taking the active fiber out breaks the current GC root scan and scheduler/reentry index model. Boxing fibers would address one relocation but not frame/stack relocation or Rust aliasing across &mut VM callbacks, and would impose representation/allocation changes on other users. Neither is justified as a prerequisite here. B1 does not change frame/value layout or program ownership.

## 4. Conditional frequency target and replacement equation

These are **design targets**, not observed candidate paths or an implementation prediction:

| Work per current ordinary cycle | Current calls | B1 target calls |
|---|---:|---:|
| Seven headers | 21 | 7 |
| GET_GLOBAL | 2 | 0 additional |
| GET_LOCAL | 3 | 0 additional |
| CONSTANT: generic helper + push | 3 | 3 (two inside helper, one reacquisition) |
| Two ADDs | 6 | 0 additional |
| SET_GLOBAL_POP | 2 | 0 additional |
| FOR_LOOP_STEP | 4 | 0 additional |
| **Total** | **41** | **10** |

Nominal eliminated **31 × (call1 + helper self6) = 217 instructions/cycle**. The three retained CONSTANT acquisitions are deliberately charged. Cold paths can acquire again; no zero-cost assertion for them.

Four-argument preparation bundles are NOT uniformly removable: some caller instructions also establish later frame/vector inputs or preserve results. Do not blanket-price 31×4 as extra savings. The current header's independently verified154 includes caller setup and overlaps this B1 opportunity.

The reviewable net equation is:

`net = 217 + E_caller/frame − I_context/interfaces − G_surrounding − D_displaced`

- **Eliminated:** only the 31 call/helper pairs IF the target is compiled, plus separately identified caller/frame work.
- **Retained:** ten acquisitions, local bounds/mode checks, frame/slot arithmetic, actual stack/vector accesses, cache locks/namespaces, constant/closure behavior, arithmetic/coercion, GC/completion/yield/interrupt/trace/debugger cadence and JIT decisions.
- **Introduced:** preserving/reloading active state over outlined stack/cache/numeric helpers, field-argument preparation, any scope-access interface/result glue, conditional reacquisition and spills.
- **Displaced:** cold reload/miss/noneligible/reentry costs, prologue/epilogue, code-size/layout changes and non-loop consumers of refactored helpers.

**No E/I/G/D bill exists yet.** Zero replacement would make217 **11.93% of baseline / 13.87% of fresh gap**; those are gross-normalization context, NOT a net prediction or confidence interval. The existing ledger prices call/helper work; it does not determine register allocation or outline decisions after interface changes. An arbitrary spill allowance would not close that gap.

The intended mechanism is falsified if ownership needs persistent raw pointers/detached roots, if the ordinary path must release/reacquire repeatedly to retain semantic authority, or if generated interface/state work erases the target. These are reasons to revisit selection, not automatically try B2 or the deferred candidates.

## 5. Validation plan for a separately approved implementation

Before outcomes:
- Source/type/lifetime review of every release/reacquire point; no unchecked path or mem::take root bypass. Add a relocation/nested callback case that grows fibers AND frames/stack, including a yielding future.get callback.
- Run scopes/unbound unit tests, bound alias/deletion/null/argument exposure, closure capture, scope spelling, exceptions/finally, GC/OOM/rooting, host lifecycle/future callbacks, route/cache isolation and existing full VM/integration/compatibility suites. Check both JIT-feature and non-JIT builds; preserve debugger line/budget tests and target limitations.
- Inspect current reference versus candidate compiled ordinary paths and helper closure, including caller result preservation and surrounding header. Confirm target acquisition count, unchanged constant/cache/arithmetic paths where promised, and explicit cold work. A removed symbol alone is not evidence of net savings.
- Preserve any pre-implementation P1; if native review changes the priced model, record a dated P2 BEFORE outcome counters. If no defensible P1 is available, do not call a product implementation an already-priced optimization.

Separate instruction/time outcome budgets, fresh-process policy and exact workload/checksum must be approved before execution. No instruction-count ratio is a time prediction. No automatic second variant/candidate.

## Decision at D1

B1 has a concrete callback/root-preserving boundary and a more realistic **10**, not seven, acquisition target. It is preferred over naive composition for the next mechanism/pricing question because it leaves bytecode and polling boundaries intact. It is **not yet selected for product implementation**: the introduced/surrounding bill remains ranking-relevant. See [selection gate](14-design-selection.md) for the bounded next decision and competing composition scope.
