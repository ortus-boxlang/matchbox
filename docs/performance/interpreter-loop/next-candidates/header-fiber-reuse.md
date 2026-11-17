# Handoff: header-only current-fiber reuse

Status: **deferred, credible, no implementation or net savings prediction**. Read [shared baseline/rules](README.md) first. Saved before deeper active-context/composition investigation, 2026-10-30. This document does not authorize a run.

## Question and current evidence

Can one scoped acquisition of the current fiber replace the three ordinary dispatch-header acquisitions without broadening pointer/borrow lifetimes across VM calls?

Current source: `crates/matchbox-vm/src/vm/mod.rs`, run_fiber_inner at 4269, ordinary dispatch at 4316–4390. Current conditional native header is **91 instructions × seven dispatches = 637/cycle**, including 21 six-instruction fiber helper executions. This is a subtotal, not removable work.

The second and third acquisition each execute four caller-setup instructions, one call and six helper instructions. **14 bundles × 11 = 154/cycle gross**. Verified current static anchors: calls 0x40c36a (first, retained), 0x40c38b (second), 0x40c7e5 (third); helper 0x406a5a. These addresses apply only to the identified current executable, not future builds.

With a hypothetical free replacement, 154 million instructions would be **8.47% of the current baseline**, **9.84% of the fresh comparator gap**. This is NOT a prediction or guaranteed upper bound on arbitrary surrounding compiler changes. No complete replacement bill exists.

## Contracts / design boundaries

- Drain native completions before acquiring state; draining may invoke code/change storage.
- Preserve missing-fiber, empty-frame, yield/reset and out-of-range-IP behavior and ordering where observable.
- Frame-change reload calls prepare_unbound_frame and chunk/cache operations; a fiber borrow cannot simply survive an arbitrary `&mut VM` call. The ordinary frame_changed=false path and cold reload/reentry need explicit ownership boundaries.
- Refresh the stack-local raw base after potential prior opcode growth; reserve(256) is not an address-stability proof.
- Preserve GC prompt/periodic checks, completion polling, JIT state and quantum/timeslice semantics. This proposal is NOT fewer polls or a different interpreter subset.
- No unchecked indexing, cross-callback raw pointer cache or changed rooting/error behavior.

## Price the actual replacement

Eliminated: repeated argument/address/bounds acquisition bundles only if the compiled path removes them. Retained: one acquisition plus actual field reads/conditions and pointer refresh. Introduced: local/reference preservation, reborrow/cold-path joining, register pressure/spills/branches. Displaced: setup and reload work, nonordinary paths and code layout/size. Prior Unbound changed surrounding header cost; projection removed a copy but regressed. Inspect the whole affected cycle, not only vanished helper calls.

Potential design is a short-lived checked fiber view ending before handlers. Do not preselect that mechanism if a split-field design, compiler behavior or a merged active-context design makes it redundant. No speculative abstraction layer is requested.

## Why deferred / ranking-changing evidence

Active-context work could remove more repeated state access; composition could remove dispatches and operand interfaces altogether. Both overlap this scope. A's narrowness is a risk/effort advantage, not proof of best net effect. If either has landed, recount the remaining seven-header structure and acquisitions before doing anything. Do not claim header154 plus active-context238 as independent savings.

Smallest useful next evidence: a lifetime-safe ordinary/cold control-flow sketch with retained checks, then a priced replacement from current fragments. If net uncertainty still changes ranking, stop for review rather than choose this because it is easiest.

## Later approved validation / stop conditions

Before outcome measurements: tests for empty/truncated fibers/frames, yield, late frame reload/BIND_LOCAL, nested callbacks that relocate fibers/stack, GC safepoints/rooting, JIT-feature and non-JIT builds, exceptions/finally and host lifecycle. Confirm compiled removal of the two acquisitions and account for spills/caller growth and slow-path setup. Preserve polling frequency and state reload ordering.

Stop on an unproved borrow/address lifetime, invalidation obligation, negative/unsupported net bill, unavailable tests or exhausted explicit budget. No automatic variant. Outcome instructions/time require separately authorized comparable fresh processes; retain raw failures and original prediction. No current timing saving is predicted.
