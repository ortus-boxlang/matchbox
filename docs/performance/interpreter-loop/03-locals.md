# Analysis handoff 1: stack-local access

Status: **historical source/native handoff, supplemented by an executed native-path ledger.** See [current accounting](09-native-ledger.md): local binding/Option helper self work is 241 instructions/cycle; inline local work and shared fiber indexing are separate. Sample shares below are historical observations, not exact costs.

## Scope and question

Explain the actual read/write path for loop index i. Why does a known stack-local traverse general scope machinery, and which work survives native optimization? Do NOT assume it mutates a dictionary each iteration.

Read [brief](00-brief.md), [evidence](01-evidence.md), [profile prerequisites](02-shared-profile.md). Source anchors below were checked at 139be94; line numbers are navigation hints, function names authoritative.

## Source-confirmed path

- `crates/matchbox-compiler/src/compiler/mod.rs`, `StatementKind::ForClassic` around 1008-1213: loop introduces compiler scope; i occupies slot 0. Initial test emitted separately; recognized increment/bound becomes FOR_LOOP_STEP.
- `crates/matchbox-vm/src/vm/mod.rs`, `interpret_chunk_shared` around 3448: synthetic root frame has scope_mode=Binding, zero params/arity, no local scope initially. Trace has no FUNCTION_ENTER or BIND_LOCAL.
- `vm/mod.rs`, GET_LOCAL around 4898: calls `local_value` and pushes its result.
- `vm/scopes.rs`, `local_value` around 213: checks scope mode, component-body special case, then slot binding; if no bound value, falls back to stack[base+slot].
- `vm/scopes.rs`, `slot_binding` around 160: look for cached local name; otherwise try parameter name. For this root frame the name table is empty and params.get(0) is None. No arguments-scope lookup/index lookup is needed after that return.
- `vm/mod.rs`, FOR_LOOP_STEP around 4545: reads i via local_value and writes increment via set_local_value.
- `vm/scopes.rs`, set_local_value around 245: tries slot_binding and parameter fallback. Both miss here; writes stack slot. No bound dictionary mutation on this path.

## Source-derived steady-state frequencies

Per iteration: **two local_value reads** (body GET_LOCAL plus fused step) and **one set_local_value write** (step). They belong to different bytecodes but one subsystem. Outer read/write helpers are inline(always) from the retained optimization; helper source size is NOT their executed native cost.

No new local struct, per-iteration local-name binding, closure capture or argument dictionary is required by this particular trace. Do not import ordinary-UDF costs into this root-script path.

## Unresolved native questions

- Which root-mode/binding guards survive inlined call sites under size optimization?
- Are the common misses implemented as helper calls, repeated frame/fiber bounds checks, or a small inlined branch sequence?
- How much read/write work is already shared/eliminated by the compiler inside FOR_LOOP_STEP?
- Is absence of reflective binding represented only as a repeated runtime miss rather than explicit slot metadata?

## Contracts to preserve in any later design

Other frames can have observable local/arguments dictionaries, aliases, captured scopes, parameter writes/deletion and component-body behavior. A fast representation must not infer safety merely from current value type or the benchmark's absence of reflection. Existing Stack/Shared transitions and shape guards preserve those contracts; this root path is only one case.

## Reviewer assignment

Once the shared profile exists, inspect actual GET_LOCAL and FOR_LOOP_STEP native paths and relevant callees. Distinguish checks eliminated by compilation from work actually executed. Produce a source-to-native path and evidence-backed cost range/share (not fabricated exact counts). State whether generic root-frame fallback is a major contributor or a minor one.

Propose any instrumented diagnostic to the coordinator; do not change scope mode, suppress reflection, add special benchmark cases or compare patched product code on your own.

## Overlap / accounting ownership

Charge helper-body work to this area, attributed under GET_LOCAL or FOR_LOOP_STEP call sites. Shared dispatch/fiber checks belong to area 3. Count the FOR_LOOP_STEP arithmetic/limit comparison only once, coordinated with area 2. Area 4 synthesizes, not adds another copy.

## Shared-profile update

Use [shared evidence](02-shared-profile.md), not a new independent benchmark. Three accepted instruction profiles (6,513 samples, zero lost/throttled records) show actual out-of-line helper work: slot_binding 2.99%, set_local_value Option::or_else 2.87%, SlotBindings::get 1.04%, local_value Option::and_then 0.28% self instruction-sample shares. These disjoint helpers total about 7.18%, excluding inlined local-access code and shared fiber indexing. They are approximate distributions, not exact instruction bills or safe-removal estimates.

The general fallback is not completely optimized away for the synthetic root frame. However, there is still no evidence of per-iteration local dictionary mutation. Follow-up direct-branch mapping identifies GET_LOCAL body PCs at 4.47% of all instruction samples, excluding outlined helpers. FOR_LOOP_STEP's 11.88% includes its inline local read/write as well as increment/bound/back-edge work; do not charge that whole share to locals. Native stack fallback at 0x41dc82..0x41dd24 repeatedly resolves fiber/frame/base before writing the incremented value. See the [follow-up review](07-cache-design-review.md). Detailed shared-helper cost ownership remains partial. Failed DWARF unwind output must not be used to assign caller percentages; rely on real call sites and explicit self shares.
