# Post-B1 composition pricing: C3-P1 v1

**Status: read-only design/pricing completed; no implementation, build or new runtime observations.** This is a new post-B1 design, not a rewrite of [D1](13-expression-composition-design.md). The approved ≤45-minute phase began **2026-10-31 00:29:14 UTC**, with a **01:14:14 UTC** stop. C3-P1-v1 was frozen at **01:04:18.110237 UTC**, before any C3 implementation or outcome. Implementation and later measurements require separate approval.

## Decision and price

Select **C3, guarded runtime composition of an existing `GET_LOCAL; CONSTANT(Number); ADD` sequence**, for the next implementation/native-validation decision. Keep the original bytecode, all standalone entrypoints and generic behavior. Do not implement unconditional atomic fusion, C2, constant reuse and cache redesign together.

For the identified post-B1 ordinary path:

```text
383 audited gross instructions/cycle
 −86…116 replacement/guard instructions
 − 0…49 surrounding-growth review allowance
=218…297 fewer instructions/cycle, conditionally

1423 → 1126…1205 instructions/cycle
```

For a million ordinary cycles, this is **218–297 million fewer instructions**, about **15.32–20.87%** of the current MatchBox batch and **18.65–25.41%** of the remaining Node gap. The approximate candidate batch, *if all other work were unchanged*, would be **1.126–1.205 billion** instructions. Cold/first-hit/boundary/residual changes remain an additional unknown signed term. Do not assume the old −6662 batch residual persists, or subtract it from a future outcome.

**This is a conditional engineering forecast, not compiled evidence or a guaranteed interval.** In particular, 49 is a historical calibration/review allowance, not a demonstrated C3 upper bound. The native-validation gate must test the assumptions and reprice before outcomes if they fail. No timing prediction is made. The alternatives are not all net-priced; this does not establish global optimality.

## Baseline and scope

- Merged B1 source: local `develop` **85383be78cda2397207cc51736c3be11c96c1dcd**. Investigation HEAD **4957e5e** has the same source tree.
- Existing measured B1 executable SHA-256: **`9af288307e2705365708940eec170e790c33c5ff94dd88c0e65829bd6cb14741`**. No rebuild or inference that a checkout identifies a binary.
- Existing scoped totals: MatchBox **1,422,993,338**, Node `--jitless` **254,277,011**, gap **1,168,716,327**. Seven-dispatch conditional MatchBox cycle **1423**; expression envelope **525**; other cycle work **898**.
- Same direct million-loop checksum **500001500000**. Arithmetic, iteration count, loop and storage unchanged. MatchBox `sum` remains global; Node CommonJS `sum`/`i` remain wrapper locals. No benchmark/literal-2/loop-bound recognition or source wrapper.
- No new execution, native trace, profile, debugger session, build, test, installation, commit or push in this phase. All seven prior packet manifests were checked intact. Main `develop` remains unmodified and unpushed.

## Why not simply remove two headers?

The three original dispatches have separate completion, GC, debugger, trace and interrupt opportunities, with different rooted stack prefixes. A fused opcode plus `counter += 3` is not equivalent. Budget-one debugger stepping must still execute exactly one instruction and resume correctly; completion work can allocate or reenter; interrupts can arrive after an initial guard.

The current `safe_point_count` is also incremented at certain backward branches. Changing its dispatch contribution changes which backedges hit the modulo-1024 clock check, not merely a nominal average polling interval. This review does not diagnose a new executed fairness failure or repair that existing counter design. It requires preserving its state/phase.

A fully resumable three-phase fused instruction would need phase-aware IP/stack/root state and tooling/JIT support. Its net cost remains unknown. C3 avoids that state machine by certifying a much smaller interval and retaining the observations that cannot be certified away.

## C3: runtime composition with an explicit quiescence proof

The original bytecode and compiler/JIT decoders stay unchanged. At `GET_LOCAL`, obtain the value **once using the authoritative existing local resolver**. Then a checked, directly inlined fast branch may replace the following two operations only when all these facts hold:

1. Trace is off; debugger stepping is inactive in debugger builds.
2. Both `pending_native_futures` and `native_completions` are empty.
3. `heap.should_collect()` is false, not merely “not the 1024th opcode.”
4. A checked view of the current chunk has the same code pointer/length as the active stream. Checked lookahead finds the next one-word `CONSTANT` and generic `ADD`.
5. Checked current constant lookup says **Number**; the current frame has a populated promoted slot for that index; the cached value and the resolved left operand are both **floats**.
6. Frame/chunk/cache borrows end before any whole-VM operation. The fast interval performs no whole-VM call or heap mutation.

The result is the existing f64 addition in the same operand order, through **`BxValue::new_number()`**. No raw constructor that could manufacture a GC tag, reassociation, integer-only semantics or BoxLang compatibility repair is introduced. Numeric strings, ints, arrays, structures, captures and other cases keep the existing interpreter behavior; they simply fail this optimization's admission checks. This is not a second numeric-only execution engine.

On any failed guard, perform the original `GET_LOCAL` push using the already resolved value and continue at the original `CONSTANT`. Do not reread/reimplement local binding semantics, disable JIT or route a feature to a less capable engine. First-hit cache misses also take this normal path, which maintains the cache through the existing authority.

### Why the two internal completion/GC checks are no-ops here

This is **not** an “empty receiver once means no producer” argument. The actual dispatcher calls `drain_native_completions()` only if one of the two VM-owned pending/deque fields is nonempty. Those fields cannot change across the certified interval: local resolution has explicit fiber/immutable-heap/shapes/interner access; metadata lookup and float arithmetic do not have `&mut VM` or mutate these fields. An external sender changes the channel, not either VM-owned field. Therefore both omitted dispatcher tests would still skip draining. Normally native handles remain counted/rooted until terminal messages drain; cancellation also keeps that protection. No weakening of that lifecycle is proposed.

Similarly, with `heap.should_collect() == false` and no heap mutation, both intervening GC checks would return false, including the external-pressure case (`should_collect_promptly()` also requires collection to be due). Frame count, yield flag and frame identity cannot change without the excluded reentry/mutation. Ordinary array/string/coercion work is not moved into this certified interval.

This proof is specific to these source invariants. A future callback, allocator/heap mutation or completion-registry redesign inside the fast interval invalidates it; do not hide that behind a generic “active context” abstraction.

### Interrupt, stack, IP and counter contract

**Keep two actual interrupt observations**, at the logical `CONSTANT` and `ADD` points. A single initial load or quiet guard is insufficient.

| Path | Required rooted evaluation state before existing throw/reentry | Required next IP | Additional logical ticks |
|---|---|---|---:|
| Interrupt before CONSTANT | Original prefix + left value | Original CONSTANT address + 1 | 1 |
| Interrupt before ADD | Original prefix + left + cached literal | Original ADD address + 1 | 2 |
| Normal fast continuation | Original prefix + computed result | Address after original ADD | 2 |
| Admission failure | Original prefix + left; ordinary continuation | Original CONSTANT address | 0 here; later headers do it |

End all active borrows before the existing flush/throw path. Cold interrupt materialization must use the original error/handler authority and preserve error order. Safe lookups/`try_borrow` must **decline optimization**, not introduce an earlier constant-index or borrow panic before the original checkpoint.

On normal continuation, wrapping counter addition by two preserves the two proven no-op headers' counter effect. Cold interrupt branches need the one/two distinction above. This is not weighting away unproved observers. Debug/trace-active execution still uses three ordinary operations, with original budgets, line locations, rooted intermediate states and resumption. No persistent phase field, detached fiber, raw promotion pointer or new hidden instruction entrypoint is needed.

## Disjoint native removal audit

These are selected paths in the **existing verified B1 binary**, not fresh dynamic frequencies. `removal-audit.json` lists every credited main PC with its assembly and helper costs; identities are scoped by opcode execution, because a common tail PC can execute more than once.

| Credited ordinary work | Main self | Helpers | Gross |
|---|---:|---:|---:|
| Two internal headers | 110 | 12 | 122 |
| Entire CONSTANT body, header excluded | 27 | 123 | 150 |
| GET_LOCAL intermediate push/call | 1 | 16 | 17 |
| Both ADD numeric-conversion interfaces/bodies | 8 | 44 | 52 |
| ADD array-pair classification, impossible for admitted floats | 10 | 0 | 10 |
| Two ADD pops/calls and successful unwrap checks | 6 | 20 | 26 |
| Conversion-result saves/reload and Option-status combination | 6 | 0 | 6 |
| **Total** | **168** | **215** | **383** |

This exceeds the earlier **223** skeleton for explicit reasons: it prices the rest of CONSTANT, intermediate stack materialization, both now-replaced conversions and related status work. It does **not** declare all 383 free. The new guarded literal/cache lookup and float checks are charged below.

Conservative retained expression budget: first header **61**, local body after push credit **40**, ADD body after listed credits **41** = **142**. Some old routing/register work in that template may disappear too; it is not credited speculatively. Real local resolution, result normalization/push, the other ADD, global semantics/cache guards, loop and all other cycle work remain charged.

## Replacement, retained and displaced work

The following are **source/native-template planning cases**, not counts of an unbuilt candidate:

| Introduced/reconstructed work | Instructions/cycle |
|---|---:|
| Repeat pending/deque emptiness guard | 5 |
| Prove collection not due | 4–6 |
| Trace gate; no debugger field in priced default-feature binary | 0–2 |
| Checked lookahead, opcode tests, constant-index decode | 11–15 |
| Current stream code pointer/length identity | 6–8 |
| Checked frame + current constant classification + promoted hit | 48 |
| `try_borrow`/checked-get/branch-interface adjustment | 0–6 |
| Two float admission checks | 6–8 |
| Two retained interrupt observations | 0–6 |
| Wrapping logical counter update | 1–3 |
| Advance IP over existing words | 1 |
| Scalar extraction/preservation and additional transfers | 4–8 |
| **Replacement total** | **86–116** |
| Separate surrounding-growth allowance | **0–49** |

The **48** reference has its own native bill: current-frame fragment **10**, chunk/type/guard main work **14**, existing borrow/index helpers **15**, promoted bounds/tag/value fragment **9**. Keep the current constant-class check: a cache hit alone is not a noncapture certificate. The 0–6 adjustment covers changed checked lookup/borrow/result handling; the native gate must detect any excess or outlined adapter rather than assuming a projection is free. The earlier cache-projection regression (+21/cycle) remains relevant negative evidence.

The interrupt low case reflects folding in the existing JIT-inactive harness binary, not permission to delete source checks. The high case budgets retained false-branch loads/tests. New result packing, extra helpers, spills/reloads or clock/GC work must be counted, wherever the compiler places them.

The growth allowance is calibrated to the earlier Unbound change: seven shared headers grew by seven instructions (**49**), partly offset elsewhere. **It is neither a C3 compiler-output bound nor a confidence interval.** Favorable surrounding reductions are not assumed; growth above 49 invalidates this pricing case and requires a dated refinement before outcomes, not an automatic source-tuning variant.

Independent budget check: main self **848 → 751–830**, helpers **575 → 375**, total **1126–1205**. Intended fiber acquisitions **10 → 5**. Helper reference **15** is included once, not added again to replacement costs. Global cache/lock bill **170** remains; B1's 32/cycle causal residual is not reclaimed.

Cold materialization, first-hit/miss guard attempts and fallback work remain real. So do new guards on **noneligible GET_LOCALs**, which may regress other workloads. No other-workload or whole-process benefit is predicted. There is no new literal payload, opcode, per-frame state, persistent cache or warmup to move work outside this region. Code size, native stack pressure and noneligible-site guard tax still need compiled review; unchanged data structures do not imply zero resource impact.

## Alternatives and reselection

| Alternative | Post-B1 price/evidence | Decision |
|---|---|---|
| Unconditional compiler fusion | Two headers + read subtree skeleton **223**, before additional layer removal; observation/resume replacement unknown | No permission to change observer policy; not selected |
| **C2: keep three physical dispatches, specialize numeric producer/right operand** | Audited gross **137** (101 reader + 26 right conversion + 10 array checks); illustrative replacement17–32, other-ADD selector0–8, growth0–49 gives conditional48–120 | Lower supported benefit, more bytecode/tooling work; overlaps C3 |
| Classified constant-hit reuse only | Retain current type/capture classification and one checked frame/cache view **48** from old reader101: conservative gross **53**, not net | Credible smaller alternative; no free 101 credit |
| Global-cache ownership/access | Unchanged helper self **170** = cache-get70 + mutex/poison/drop/done100; net unknown | Strong independent alternative, but ownership/sharing/namespace proof remains |
| Header-only fiber reuse | Already subsumed by B1 | Retire as separate candidate |
| No change | Current1,422,993,338; zero new saving/cost | Retain if compiled mechanism/contracts fail |

C2's checked two-word numeric payload adds two code and two line words (**16 bytes of program vector payload**) per occurrence. Existing dense runtime-cache indexing is by code word; the identified native representation adds **two 80-byte initialized slots per dense cache instance**. Capacity/allocator changes, sparse ESP32 behavior and any removed/deduplicated constant offsets are separate; do not call this a universal +160-byte net allocation. C3 avoids that representation expansion and all new-opcode JIT/decoder support.

A naive constant-cache-first check is not justified by the public mutable frame/chunk/cache structure: a populated numeric slot can coexist with a subsequently different/capturing program constant. However, **retaining current classification before reuse**, and sharing its authority, is a valid narrower alternative without inventing a permanent cache certificate. Its smaller gross price does not establish its net result.

Cache redesign must preserve `Chunk: Send + Sync`, immutable program/runtime separation, reusable functions/recursion, fresh execution/route caches, program identity, namespaces and lifecycle. Existing route-owned vectors already avoid locks. Neither170 nor100 is net removable work, and the rejected projected accessor must not be repeated on faith. These opportunities overlap neither arithmetically nor architecturally with all of C3's costs; do not sum them into a combined forecast.

C3 is preferred for this objective because it removes more fully priced layers, supplies a concrete observer proof without phase/metadata state, and leaves program/JIT representation intact. It is not proven preferable for arbitrary noneligible workloads; broad guard tax is an explicit potential rejection reason, not an optimization opportunity silently deferred to another variant.

## Next approval gate

A separate bounded authorization could cover **one C3 implementation**, contract tests, one same-profile candidate build in a separate target/artifact directory, and static native validation. No outcome measurements are implied.

Required tests include guard failures; authoritative Unbound/Stack/Shared local behavior and exposure; numeric/string/int/array fallback and operand order; NaN/signed-zero/infinity; cache miss, changed constant class, fresh captures and forced GC; nested frames/callbacks/native completion and cancellation; interrupt prefix/IP/handler state at both logical points; counter wrapping/GC/backedge phase; budget-one debugger resumption/trace; JIT eligibility/entry and existing lifecycle/namespace contracts. No weakened tests, disabled capabilities or ignored unsupported opcode.

Compiled validation must reconstruct the complete cycle, inspect all new guard/borrow/scalar/helper work, detect adapter outlining and displaced work, and explicitly bill other GET_LOCAL sites, code size and native stack growth. If guards, borrow lifetimes, observer proof or costs fail, stop or reselect: no autonomous variants. Preserve C3-P1-v1, write any P2 before outcomes, then seek separate approval for counters and separate timing. No campaign/debugger budget is reopened.

## Evidence

Ignored local packet in the dedicated worktree:
`benchmarks/instructions/results/c-pricing-20261031T002914Z/`

It contains the bounded plan, clean-state/seven-packet preflight, source-before/after hashes, source/reference snapshots, input identities, `removal-audit.json`, frozen `prediction-C3-P1-v1.json`, `pricing-summary.json`, arithmetic script/log, and final manifest. The initial audit script failed on manually mistyped native PCs; the executed version/failure are preserved, corrected against saved path text, and the corrected exclusive audit passed. There was no implementation or runtime candidate hidden behind that correction.

Documents alone do not transfer original binaries, raw counts or this ignored packet. Previous packets/build outputs and local history remain intact; no commit, merge, push or cleanup occurs in this phase.
