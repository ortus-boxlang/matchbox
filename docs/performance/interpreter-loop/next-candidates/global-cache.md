# Handoff: global-cache ownership and access redesign

Status: **deferred, credible, no implementation or net savings prediction**. Read [shared baseline/rules](README.md) first. Saved before deeper active-context/composition investigation, 2026-10-30. No automatic retry of the rejected cache projection and no authorized experiment loop.

## Question and current attribution

Can a coherent cache ownership/access design remove synchronization/result machinery without weakening reusable-program, VM-namespace, routing or concurrency contracts?

Source: `vm/mod.rs:5503–5593` GET_GLOBAL/SET_GLOBAL_POP and related global handlers; `vm/chunk.rs:93–165` ChunkRuntime and cache access; program/runtime split and clone_without_runtime_caches; global_namespace in VM and cache entries. Read existing [ownership review](../07-cache-design-review.md), [rejected projection result](../08-cache-projection-result.md), [executed reconciliation](../09-native-ledger.md), then the NEW current static packet described in the shared README.

Current conditional GET_GLOBAL and SET_GLOBAL_POP cost **275 and 274** including one header each. The full sum update (including ADD) is **806**, versus Node's late frame-local sum-update74. That full difference is NOT a cache-only bill and may not be semantically attainable: MatchBox sum is a VM global and Node sum a wrapper-frame local.

Current disjoint cache/mutex/poison/drop helper self is **170/cycle**: cache_get70, mutex lock42, poison guard24, guard drop20, poison done14. RefCell borrow, fiber/frame resolution, caller result preservation and namespace/index/value operations are separate ledger buckets. Not all 170 is removable. Generated generic symbol names may be identical-code-folding artifacts, not regex activity.

## Historical negative evidence to retain

The rejected projection removed a wide cache result copy but introduced an outlined projection/helper chain. Historical executed reconciliation: accessor +2, new helper +24, caller -5 = **+21 instructions/cycle**, matching the earlier ~21-million regression. Two REP instructions are two logical retired instances in that model; payload width is not an instruction-savings bill. Preserve and cite the old prediction failure, not a rewritten success story.

That failure does not rule out a genuinely different ownership/layer-removal design. It DOES rule out choosing another copy tweak without pricing caller, result, guard drop/tail calls, poison handling and replacement work.

## Contracts / mandatory ownership audit

- Arc<Mutex<RuntimeCaches>> sharing across clones/calls; VM-owned function runtime versus reusable immutable ChunkProgram. Establish every actual sharing/transfer path before changing Arc/Mutex or assuming single-threaded access.
- Chunk Send/Sync, route/script entry cache isolation, compiled/native modules and function/class promotion boundaries. A single observed thread is not proof the type's synchronization contract is unnecessary.
- Namespace verification must prevent one VM's global index from being trusted in another. Preserve stale-cache reset/program association and program edits/serialization behavior.
- Keep ordinary/fast/fallback GET/SET behavior, missing globals/nulls, per-VM cache lifecycle, key resolution and panic/error/poison behavior. ESP32 has sparse caches and must not accidentally inherit a dense-only design.
- Reentrant callbacks and nested execution may observe the same VM-owned function state even if an outer handler has a short cache view. Do not hold a guard across arbitrary VM calls or use stale index/value pointers.
- Public/native/JIT consumers and Chunk clone/equality/debug contracts need a source audit, not only one global opcode.

## Price alternatives before choosing

Compare a whole ownership separation (immutable program/shared metadata versus explicitly VM-owned mutable caches), a narrow typed lookup preserving existing synchronization, and no change. Prefer existing abstractions/libraries where adequate; do not add a generic cache abstraction or compatibility layer speculatively.

Eliminated: only demonstrated repeated ownership/synchronization/result work. Retained: valid cache/index/namespace/value operations and any truly shared synchronization. Introduced: lookups/handles, lifetime plumbing, initialization/reset, guard/result projection, spills or larger caller state. Displaced: cache construction/clone/promotion/route startup, memory use and other opcode/users. A setup cost moved outside the marked loop is not free.

## Why deferred / ranking changes

Active-context and composition scopes have credible broader effects and overlap caller/frame/operand work; their investigation is not a judgment that caches have zero benefit. If they land, recalculate cache and caller envelopes. Do not sum cache170, global549 and sum806, or add their potential to unchanged broader predictions.

Next discriminating evidence: a complete ownership/shareability map with a concrete legal replacement and compiled-fragment net bill. A finding that caches must remain shared/locked changes the design; do not weaken the contract to preserve a predicted saving.

## Later approved validation / stop conditions

Required tests include multiple VMs over shared/reused programs, repeated route handlers/cache warmup, cloned/promoted functions/classes/modules, namespace reset/stale guards, concurrent program use where supported, reentrant host callbacks, error/poison handling and ESP32 sparse-cache compilation/behavior within available tooling. Verify unchanged code-generation consumers/JIT contracts. Unavailable target validation stays explicit.

Before outcome counters, inspect full accessor/caller/drop paths including replacement tail calls and result/spill code. Record a versioned net prediction before implementation and explicitly reconcile it afterwards. Stop on an unproved sharing contract, hidden setup/memory tradeoff, unsupported net ranking or budget exhaustion. No automatic projection variant, profiler budget reset, commits or pushes. Separate instruction and time validation require new explicit authorization.
