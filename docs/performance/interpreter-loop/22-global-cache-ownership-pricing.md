# Post-C3 global-cache ownership audit and G1 pricing

**Status: bounded read-only audit complete; no G1 implementation, build, test or performance outcome.** Recommend considering one owning-entry construction unification, not another global-slot projection or a blanket removal of function-cache synchronization.

## 1. Scope, current position and goal

Separately authorized after [C3 instruction closure](21-c3-instruction-result.md). Audit started2026-10-31T03:59:02Z; at most45minutes, deadline04:44:02Z. Source, existing native artifacts and already-fetched Git objects only. No product edit, compiler/disassembler invocation, tests, workload/count/timing/profile/debugger run, install, CPU/security change, commit/merge/push/fetch/reset or cleanup. Documentation/evidence only; the exhausted campaign/debugger budget stays exhausted.

The unchanged million-iteration checksum remains500001500000. Current measured C3 median **1,092,997,730** instructions; fresh Node `--jitless` **254,277,191**; remaining gap **838,720,539**. MatchBox `sum` remains a VM global, Node `sum`/`i` CommonJS wrapper locals. The goal is less execution work with the same storage/semantics—not turning this global into a local or recognizing the loop/literal. Existing native host: AMD Ryzen7 PRO6850U, Linux7.2.8-1-default x86-64/glibc2.44. No new timing claim.

Local source: main `a99b926`, investigation `7882dfa`, same committed C3 tree. Origin is now `c2b6ec8`, three incoming server commits ahead; those fixed objects were read without checkout/fetch/merge. Relevant cache constructors/handlers/promotion/spawn regions, `chunk.rs` and `request_scope.rs` are unchanged there, but its persistent-app/worker lifecycles matter. Incoming source was **not built or tested**. Future different-base builds need matching baselines/provenance, not mixed-base attribution.

## 2. Ownership and contracts

Source anchors are at the local C3 revision; packet contains snapshots and the fixed incoming diff.

| Layer/path | Actual ownership / required behavior |
|---|---|
| Source/compiler/template metadata | `ChunkProgram` is `Arc`-shared immutable program data. `ChunkRuntime` has public `Arc<Mutex<RuntimeCaches>>`, constant interning map and weak cache-program identity. `Chunk`/`ChunkProgram` must remain `Send + Sync`; runtime/private caches are skipped by serde. |
| Owning `VM::interpret`, `mod.rs:3412` | Fresh shared runtime, ensure slots, then `Chunk::clone`: pseudo-script function metadata and execution chunk **alias the cache Arc**. Execution wrapper is `Rc<RefCell<Chunk>>`. |
| Borrowed/route entries, `mod.rs:3422–3458` | Existing `fresh_route_chunk()` gives execution Chunk a private vector; pseudo-function metadata is separate/fresh. Source caches remain untouched; program `Arc` shared. Same `interpret_chunk_shared` scheduler/frame builder for owning and ordinary borrowed entry. |
| User functions/recursion/classes | Promotion resets template caches for the current VM (`mod.rs:9441–9468`). **Nine** `execution_chunk()` call sites preserve same-program shared cache state across calls; weak program association rejects sharing after program replacement (`chunk.rs:379–397`). Per-invocation constants/scopes stay in the frame. |
| Other entry policies | `interpret_sync` uses `spawn`/`execution_chunk`; debugger start builds its own shared root. G1 below **does not change these** or pretend every script/function invocation benefits. |
| Namespace/global slots | Process-unique checked namespace allocator; each hit still checks `IcEntry::Global.namespace`, then bounds-checked global value access. `with_isolated_globals` restores maps/roots and advances namespace, including error/panic paths. Retained callbacks may therefore need refills in later requests. |
| Native incoming routed app/worker | VM constructed/used on its runtime thread; source programs and owned request/JSON/channel data cross boundaries, not live VM cache references. Retained middleware/routes/timers/job/socket callbacks survive request GC. Isolated request cleanup, detached-work rejection/cancellation, native terminal-message roots and worker/module shutdown order remain mandatory. |
| ESP32 runner | **Not universally thread-pinned:** `web.rs:66–98` has an unsafe Send/Sync raw-VM wrapper under an outer `Arc<Mutex<ApplicationVm>>`. Startup moves the completed VM into this container; scheduler and HTTP access lock it (`590–637`, `804–859`). Existing borrowed routes already use private caches under that serialization. Do not remove the outer mutex or infer safety merely from native single-thread execution. |

`current_chunk()` clones an **Rc**, not the whole Chunk. VM-local callbacks/handles can retain it beyond frame pop: private caches are **execution-Chunk-owned**, not guaranteed destroyed at the exact pop. No new reference may escape a checked borrow across arbitrary callbacks. Existing global and shape consumers use mode-aware `Chunk::cache_*` APIs. JIT shape snapshots read `frame.chunk.cache_slice`, exclude namespace-bound Global entries, and retain immutable program ownership; no production direct `runtime.caches` consumer outside `chunk.rs` was found. Two direct inspections in lifecycle tests concern immutable source caches.

`Chunk` Clone/PartialEq/Debug/serde implementations and shared-cache poison behavior would remain unchanged. However, moving owning entry to the existing borrowed policy **intentionally stops pseudo-root metadata mirroring active frame ICs**. External Rust code inspecting/poisoning `frame.function.chunk.runtime.caches`, or invoking that pseudo-root as a function, needs explicit review/validation: the active private cache is accessed through `frame.chunk.cache_*`, and a pseudo-root invocation warms its function cache independently. No external compatibility promise is invented. Ordinary user-function sharing stays intact.

## 3. Attribution and candidate selection

C3's verified conditional ordinary cycle is1093 =733 main +360 helper self. The two global accesses still incur:

| Disjoint helper self | Per cycle |
|---|---:|
| `Chunk::cache_get` | 70 |
| Mutex lock | 42 |
| Poison guard | 24 |
| Guard drop | 20 |
| Poison completion | 14 |
| **Total** | **170** |

This is two selected inclusive85-instruction cache subtrees, **not170 net savings**. RefCell borrow18/cycle, caller setup/result handling, global namespace/index/value access and all other work are separate. The misleading generic regex cache drop symbol is identical-code-folding, not regex execution.

Attribution: this170 is current MatchBox work because owning entry selects the shared-cache path despite having an entry-private execution owner. Source determines that mode, the saved native ledger prices the warm path, and C3's aggregate hardware result closely fits its ordinary model. This is not a new dynamic native trace or proof that precisely170 of the *Node gap* is cache overhead: Node uses frame locals, not an equivalent global cache.

| Credible scope | Net evidence / risk | Decision |
|---|---|---|
| **G1: unify owning entry with existing borrowed-entry construction** | Existing compiled private hit25 versus shared85; two root hits support a conditional120/cycle point. Same hot handlers/accessor/result ABI, no new cache abstraction or fields. Metadata aliasing and cold allocation costs explicit below. | **Best-supported bounded proposal for this fixed goal.** |
| Full VM-owned function execution-state separation | Could remove locks from retained function calls/recursion too, but mutable runtime-function ownership must be separated from shareable metadata; simply substituting Rc/RefCell in ChunkRuntime breaks Send+Sync. All nine entry paths, promotion/classes/modules, weak program identity, requests and JIT need coherent plumbing. Handle/borrow/init/memory and net costs unknown. | Credible broader architecture, not a guaranteed120 alternative; larger workload benefit unmeasured. This target has the same two root hits, so it does not justify that migration first. |
| Typed synchronized global projection | Historical measured reconciliation: accessor+2, new outlined helper+24, caller−5 = **+21/cycle regression**. Removing an80-byte result copy did not eliminate ownership synchronization. | Do not retry a layout/inline/projection variant. |
| No change/further investigation | Zero delivered saving; avoids product risk. Other ADD/loop/header totals are costs with unknown removable net, not zero-benefit alternatives or savings to stack. | Choose if ownership/metadata/resource/native verification invalidates G1. |

G1 is an incremental fixed-goal improvement, not a claim that cache architecture or the remaining Node discrepancy is solved. It reaches the attributed root accesses by removing duplicate entry initialization rather than adding a compatibility layer. Existing routes are already private; retained server callbacks still take the shared function path. **No request/worker throughput forecast.** A service/function-focused goal could change selection.

## 4. Proposed replacement—not implemented

**Owning `VM::interpret` → existing `interpret_chunk_borrowed` construction → same shared execution/scheduler.** Remove the redundant consuming-entry initialization. Do not change global opcodes, bytecode/program representation, values/storage, C3 guards, ticks/observer opportunities, JIT configuration, namespace checks or general Chunk/cache APIs.

The borrowed constructor already supplies a private execution vector and **fresh separate metadata**. Do not create the private entry, resize it and then `Chunk::clone()` into metadata: owned-vector Clone would duplicate the dense cache. Normal function `execution_chunk` remains shared; do not replace all function/recursive invocations with fresh per-frame vectors. `interpret_sync`, spawn and debugger-start policies remain outside this narrowly priced change, retaining existing capabilities.

## 5. Existing native-template net price

Artifact SHA-256 `72585b4106c8876b0ed45c3dff1f2948ed1549c016698069c976b9a567f60f3a`. Whole65-instruction `cache_get` body was already normalized/verified in C3. This audit walks its alternative private arm from saved text; **no new disassembler invocation or execution**. Shared unpoisoned warm hit85 =35 self +33 lock/guard +17 drop/completion. Private warm hit25 self, no nested call. Both still perform the wide80-byte entry copy; one REP instruction per hit, not80 independent retirements.

Exclusive executed-PC accounting across two accesses:

```text
Current170 = retained common-prefix16 + replaced old exclusive154
Removed old exclusive154 =54 cache_get self +100 sync helper self
Replacement34 = entered private-arm work outside the same common prefix
Candidate50 = retained16 + replacement34
Net120 =154 gross removal −34 replacement =170 −50
```

The common prefix is eight identical native PCs/access. Replacement includes retained *semantic* bounds/tag/entry-copy/return work at different PCs; it is not34 new source instructions. Caller call/result ABI and RefCell work remain charged. No new hot projection/adapter is expected, but that is a compiler-transfer hypothesis, not a verified G1 binary.

| Existing template lookup | Shared inclusive | Private inclusive | Difference |
|---|---:|---:|---:|
| Populated slot | 85 | 25 | 60 |
| In-bounds empty slot | 81 | 20 | 61 |
| Out of bounds | 77 | 16 | 61 |

Empty/OOB rows price only lookup, **not** the full global miss/name resolution/refill/resize path. Poisoned/contended shared paths remain outside the ordinary selection.

If the template/callers transfer unchanged: helper360→240, main733 unchanged, **1093→973/cycle**. GET_GLOBAL body161→101 and SET_GLOBAL_POP157→97; the other775 and all five61-instruction headers remain. Do not also subtract cache170, lock100, old global549, B1 acquisitions or C3 CONSTANT/header removals.

## 6. Cold/resource/other-user costs and frozen prediction

- Current owning root allocates one fresh shared cache handle and aliases it into metadata. Existing borrowed construction uses **one additional empty Arc<Mutex<RuntimeCaches>> source-level allocation**, plus another fresh HashMap initialization; weak/clone/ensure/drop work also differs. Private ensure/set can remove some cold locking. Exact signed whole-phase instruction change is unpriced, not free because it occurs before START.
- Both constructions eagerly ensure the entry slots before executing the script; this is **not moving initialization/warmup across the boundary**. Native dense slot stride is80 bytes; one entry vector remains proportional to code length. Source/program/constants/line tables are not copied. Actual capacities, allocator overhead and RSS are unmeasured. Avoiding dense duplication is a correctness/resource gate.
- A **full Clone of an active private Chunk copies its vector**, unlike cloning a shared cache Arc. Inventoried `current_chunk()` users clone Rc or inspect metadata, not that vector, but external Rust/native use is unknown. Short scripts, cloning-heavy hosts and pseudo-root re-invocation can have different tradeoffs.
- No production VM/frame/program field or GC root addition. IcEntry contains scalar indices/guards, not BxValues; promoted constants and heap roots stay in their existing owners. Native code size, stack reservations, constructor inlining and other workload effects require a future build/measurement, not an assumed zero.

**G1-P1-v1**, frozen2026-10-31T04:11:02.601701Z, SHA-256 `81e588786ce09472be0f99e9a7d5042bed0e93788eca5297719bd05bfc747ed1`, remains intact. Continued source audit found the serialized ESP32 unsafe wrapper; the initial “cross-thread owner” falsifier was overbroad. **G1-P1-v2**, frozen04:14:07.371641Z, SHA-256 `5d4da2da51b77a1bc08a7f9c05dcf90a619366706a025c16c68f4018b2372702`, clarifies that unsynchronized concurrent aliasing—not serialized cross-thread use—is disqualifying, and records Rc-extended cache lifetimes. **Numeric pricing is unchanged; no G1 code/build/outcome exists.**

Let **G** be signed compiled per-cycle deviation from the transferred template, including helpers/callers/spills/adapters/surrounding code; **D** signed changed nonordinary/first-hit/periodic/boundary/final/residual work inside the original region; **S** signed changed process work outside it:

```text
Candidate ordinary cycle =973 +G
Scoped million-batch saving =120,000,000 −1,000,000×G −D
Whole-process instruction saving =120,000,000 −1,000,000×G −D −S
```

Conditional ordinary reduction120million is **10.97898% of measured C3** and **14.30751% of the remaining Node gap**, not a timing forecast. The illustrative batch point972,997,730 requires G=D=0; **none of G/D/S is a proven bound or assumed zero**, and C3's old−2270 residual is not inherited as a correction. G≥120 would erase the ordinary saving before cold terms. Source hot-code sameness does not prove compiled transfer.

## 7. Decision and future validation gate

**Stop here: recommend considering G1, not implementing it automatically.** Delivered work is C3 counter closure and a source/artifact-backed proposal, not a new product speedup.

If separately approved for one bounded implementation/test/build/native-validation pass:

1. Identify source base first. Preserve incoming commits; if adopting them, use a same-base reference, separate targets/artifacts and exact source/patch/toolchain/profile/feature/flag/binary hashes. Do not overwrite frozen C3 or infer executable revision from checkout.
2. Test owning/borrowed source-cache isolation over independent VM layouts, all IC variants, namespace restore/error/panic, regular function recursion/cache reuse and weak program changes, classes/modules/retained callbacks, GC/root/native-future cleanup, nested callbacks and pseudo-root reuse. Verify intended metadata separation and no duplicate dense vector. Keep Chunk Send+Sync/serde/Clone/PartialEq and shared poison contracts. Do not weaken unsupported external/native assumptions into success claims.
3. Preserve existing JIT/OSR/deopt/shape-snapshot behavior and debugger entry/budgets; both features remain enabled where tested. Review sparse ESP32 behavior and outer VM locking; unavailable ESP32/Wasm builds/device/interleaving coverage stay explicit. Incoming persistent-app/job/socket/timer/module shutdown contracts need matching-base validation, not credit from old C3 tests.
4. Inspect complete helper/caller/drop paths: private25, two accesses, unchanged C3/storage/observer paths and no new adapter is the falsifiable expectation. Reconcile actual G, resource and cold changes; preserve original P1 and freeze any compiled P2 **before outcomes**. Reject or revisit on nonpositive net, hidden cloning/ownership change or failed contracts; no automatic variant loop.
5. New hardware outcomes and separate timing/affected-workload checks require their own bounded decision. No diagnostic budget reset, commits/push or cleanup is authorized by this audit.

## Evidence and confidence

Local ignored packet (documents alone do not transfer it):

`/home/jacob/dev/ortus-boxlang/matchbox-checksum-baseline-20261030T2203Z/benchmarks/instructions/results/global-cache-audit-20261031T035902Z/`

`preflight.json`, source snapshots/local+incoming diff, consumer inventories, `selection.md`, `ownership-review.json`, saved C3 helper/model, `native-template-pricing.json`, six exact selected paths, preserved P1v1/P1v2, analysis scripts, final verification and `files-sha256.json`. All12 prior packets/727 listed files preserved/reverified; no sealed packet overwritten.

| Claim | Evidence level / limit |
|---|---|
| C3 total/remaining gap | **Measured**, prior sealed nine-process counter phase, not new audit measurements |
| Ownership/mode/namespace/shareability map | **Source-confirmed**, fixed local/incoming revisions; no new dynamic contract tests |
| Shared85/private25 and gross154/replacement34/net120 | **Artifact-confirmed conditional templates**, exact saved native paths and helper closure; private path was not dynamically traced here |
| Legal/best-supported owning-entry reuse | **Inferred design selection**, metadata/public-native/lifecycle acceptance still gated |
| 973-cycle/120million saving | **Conditional prediction**, no G1 artifact/outcome, compiler/cold terms unknown |
| General target/service/short-script/external-host benefit | **Unresolved**, no universal performance or compatibility promise |
