# Candidate selection: explicit unbound-local state

Status at writing: **design recommendation and conditional pre-implementation estimate P1; no product edit, build or new runtime execution at that point.** Subsequently approved and executed; see [11-unbound-local-result.md](11-unbound-local-result.md). The original P1 estimate and decision record below are preserved. Date: 2026-10-30. Applies the candidate-selection gate in the global evidence-driven-improvement skill. Supersedes the header-first recommendation, not the measurements or accounting in [09-native-ledger.md](09-native-ledger.md).

## Focus, baseline and goal

Continue reducing the unchanged checksum loop's retired user instructions without weakening BoxLang, changing storage contracts, activating JIT, or adding a second interpreter. Existing reference: `139be94a18d7d6bda04078bf192485cf3f67e8e9`, executable SHA-256 `5787b8a92026d52d97ce97d18cf552d06fbfc8d324493bcd661b94aa8e220557` (rechecked in this pass). Source, compiler/profile, counters and limitations remain as recorded in 09.

- Existing million-batch normalization: MatchBox **2172.993764**, Node **254.269661** instructions/iteration; gap **1918.724103**, ratio **8.55x**.
- Executed selected MatchBox cycles: **2173** logical native instructions. Node late ordinary cycle: **247**, not its whole-batch normalization. Node's approximately 3.3% batch residual remains unassigned; no new GC explanation is supplied.
- Goal: materially close the measured gap, preserving general behavior. No agreed percentage or justified promise of Node parity. Gap-closure fractions below use Node as a comparison reference, not as an established achievable target under identical semantics.
- Scope of this pass: read-only source/reference inspection and reanalysis of saved traces, plus these analysis files. Exhausted campaign/debugger budgets remain exhausted. No new profiler sessions, builds, PMU batches, timing runs, variants, commits or pushes.

## 1. Compare scopes before choosing one

The ledger buckets are accounted work, not savings. The following scopes overlap; **do not add their opportunities**. Unknown net benefit is not zero.

| Scope | Existing work exposed by evidence | Potential effect / confidence | Risk, effort and selection |
|---|---|---|---|
| A. One fiber borrow inside the header | Two redundant lookup bundles per dispatch: **154/cycle** | Conditional gross ceiling for those bundles: 7.1% of baseline, 8.0% of gap, before replacement cost. Net unpriced. Even free replacement leaves about **7.94x** the Node batch total. | Narrow lifetime/implementation surface. Not selected: limited end benefit and not a prerequisite for C. It was easy to count, not demonstrated best. |
| B. Active execution context through header and handlers | **49** fiber-index calls; call + callee self **343**, before caller setup and repeated frame reconstruction. A one-acquisition-per-dispatch design would remove 42 of 49 calls: **294** call/callee instructions before new work. | Likely hundreds of instructions are addressable; caller/frame work may enlarge the opportunity. No defensible complete net range yet. Do not call all 343 unnecessary, or all 588 header instructions removable. | Strongest architectural alternative. High implementation/validation surface: helpers currently take `&mut VM`; whole-VM calls, callbacks and Vec relocation interrupt borrows. Potentially broader than C, but not clearly a greater first net benefit for its risk/effort. Defer, not reject. |
| C. Explicit state for frames with unbound stack slots | Three local-access sequences total **553/cycle**, including required stack access; detailed disjoint accounting below | Conditional P1 net estimate **310–449/cycle**: **14.3–20.7%** of baseline, **16.2–23.4%** of measured gap. Existing path known; replacement code generation not yet verified. | Selected for review. Removes repeated general scope resolution when a maintained frame invariant proves it cannot produce a binding. Limited to frame mode, local helpers and binding transitions; no bytecode, raw-pointer or concurrency redesign. |
| D. Arithmetic/constant/operand organization | Numeric helper self **88**, constant/vector helper self **80**, stack helper self **130**, plus relevant handler/dispatch work | Hundreds may be addressable through direct operands, stable-value constant access and shared arithmetic implementation. These 298 helper instructions are not all unnecessary, nor restricted to ADD; no combined net prediction. | Generic coercion/arrays/closures/constants, bytecode tooling/JIT consumers and safepoint cadence complicate a coherent operand redesign. Could ultimately exceed C; current evidence does not price that replacement. Not a reason to assign it zero value. |
| E. No change / further investigation | No product savings now | Appropriate if no safe replacement survives review or compiled cost invalidates P1 | Preserves the reference and authorization. It is preferable to a speculative implementation or automatic next variant. |

**Why C rather than B:** C targets not merely the 241 outlined binding-helper instructions but the entire repeated resolution layer, with a 553-instruction observed envelope. Its conservative structural model already exceeds A's zero-overhead ceiling and is comparable in scale to the directly established part of B. Unlike B, C does not require a mutable fiber/frame/stack reference to survive any VM callback or handler boundary. The proof is a maintained fact about bindings, not an assumption that current numeric values will stay numeric. B remains a worthwhile later architectural scope, including state work outside locals.

**Purpose:** an end improvement with incremental gap closure, not a prerequisite, diagnostic instrument, or claim to solve the full interpreter gap. If P1 holds, roughly **76.6–83.8% of the current comparator gap remains**, and the ratio is still approximately **6.78–7.33x**. This is not a path to parity by itself.

**What would change selection:** a requirement for substantially more than 23% gap closure in one step would disqualify C as sufficient; a safe B design with priced substantially larger net savings at comparable effort/risk could outrank it. If C requires per-access binding lookup to validate the marker, cannot cover the reference root frame, or fails its native budget, revisit selection rather than default to A. A new benchmark/profile is not needed to decide these structural questions first.

## 2. The local-access envelope, recomputed from existing evidence

`compare-scopes.py` reads the original `native-ledger-20261030T201844Z` reports; it does not launch a workload or rewrite that evidence. All four reference cycles (i=3,4 and 100000,100001) have these same sequences. Anchors are **start/end positions along the recorded execution**, including intervening calls, not a reachability count or a contiguous address range.

| Executed sequence | First PC → last PC, inclusive | Logical instructions |
|---|---|---:|
| GET_LOCAL local read, before push | `0x418779` → `0x41be77` | 182 |
| FOR_LOOP_STEP local read, before increment | `0x40e958` → `0x411412` | 185 |
| FOR_LOOP_STEP local write, after numeric result preparation | `0x41cf69` → `0x41dd21` | 186 |
| **Disjoint total** | | **553** |

A second, disjoint partition of the same 553 instructions:

| Work within the sequences | Instructions |
|---|---:|
| Inlined caller/frame/state/result machinery, including stack accesses | 216 |
| Fiber-index helper self, 14 calls × 6 | 84 |
| Binding/Option helper self | 241 |
| Value-vector indexed-access helper self, 3 × 4 | 12 |
| **Total** | **553** |

The other **1620** instructions remain outside this scope. In particular, the 553 excludes all seven shared headers, operand-stack push/pop helpers, the ADDs, numeric increment/comparison/result construction, limit-constant retrieval, global caches and locks. It includes temporary/result preservation associated with these accesses; not every instruction is pure dictionary resolution or removable.

Overlap with B: **14 calls + 84 callee instructions = 98**, before their caller preparation and frame reconstruction, are already inside these sequences. Do not claim a C saving plus an unchanged B saving. A's header bundles do not overlap the selected sequences, but compiled combined benefits still would not be assumed additive.

The functional comparison in 09 remains the comparator authority: Node computes `i+2` through frame addressing and immediate arithmetic, and its increment/control segment does not perform MatchBox binding resolution. This does not mean the full 553 is excess over Node, or that the storage/language contracts are identical.

Recompute, with the ignored raw packet present:

```sh
python3 docs/performance/interpreter-loop/compare-scopes.py
# Or supply another location containing that same preserved packet:
python3 docs/performance/interpreter-loop/compare-scopes.py /path/to/native-ledger-20261030T201844Z
```

## 3. Why this work occurs, and a safe removal mechanism

Source-confirmed at reference revision:

- `vm/scopes.rs:161` — `slot_binding` first consults `frame.local_names`, then `frame.function.params`. With no binding entries and no parameters, it necessarily returns `None` without consulting a scope dictionary.
- `scopes.rs:213` — `local_value` nevertheless reconstructs current fiber/frame for mode/special-case checks, resolves an optional binding, then reconstructs frame/stack for the fallback.
- `scopes.rs:245` — `set_local_value` resolves the binding again and tries the parameter-insertion alternative before writing the stack fallback.
- `vm/mod.rs:3448` — the reference root frame has zero arity, no parameters/bindings and initial `Binding` mode. `compiler/mod.rs:3154` — `add_local` emits BIND_LOCAL for named function locals, not ordinary script block locals. The existing trace has no BIND_LOCAL for `i`.
- `scopes.rs:93–105` — existing `Stack` mode has different preconditions and implicit-arguments exposure semantics. Merely calling the root frame `Stack` or broadening that mode is not this proposal.

**Attribution gate:** approximately 553 logical instructions of the observed MatchBox path perform three local access/resolution sequences. They occur because an unbound stack slot is routed through the same machinery that supports scope-bound locals and parameters. This contributes to the comparator gap through repeated state and binding reconstruction absent from Node's corresponding frame-operand path. The isolated removable portion is smaller; its net size depends on the replacement, not the envelope.

### Proposed frame invariant (design, not implemented)

Add a distinct `Unbound` variant to the existing private `ScopeMode`:

> While Unbound, the active frame has no named slot bindings, no parameters or implicit arguments slot, and is not a ComponentBody. Every local read/write therefore uses the already-existing stack fallback.

Suggested minimal lifecycle:

1. Keep existing frame constructors initially `Binding`. In the existing `frame_changed` reload block (`mod.rs:4333`), admit a frame to Unbound only if its current mode is Binding, `params.is_empty()`, `arity == 0`, kind is not ComponentBody, and `local_names.iter().next().is_none()`. This avoids changing twelve frame construction paths or inspecting metadata on every dispatch. Classification is at frame reload, not necessarily only once per invocation.
2. At the start of both local helpers, if the frame is Unbound, use checked `stack[stack_base + slot]` access directly. Inline the decision in the existing helpers. Do not add an outlined result-projection/Option adapter. Do not cache a value, raw address or scope reference.
3. **Before `bind_local` installs the first binding**, disarm Unbound to Binding. Preserve the existing wide-slot/alias transition to Shared. A binding is never installed while retaining the certificate.
4. Preserve `expose_frame_scopes` as a permanent transition to Shared. Do not convert Shared back to Unbound merely because names later disappear. Existing Stack entry/exposure behavior stays unchanged.
5. Binding insertion through parameter fallback cannot occur in Unbound because parameters are empty. POP/truncation can only remove entries. Captured/global/request values do not acquire local slot bindings by being mutated; their existing paths stay authoritative.

This is a general representation invariant for eligible frames, not a check of script name, source text, checksum, loop shape, iteration count or value types. Ordinary zero-declared-argument UDFs have the implicit arguments slot (`arity = params.len()+1` in `compiler/mod.rs:2753`) and **do not qualify**. This first version does not optimize their bound local/parameter representation. Constructors or other zero-arity synthetic frames may qualify until a binding/exposure invalidates the state.

### Ownership and semantic boundaries

| Boundary | Required behavior |
|---|---|
| Fiber/stack/frame Vec relocation; nested synchronous callbacks | No reference/pointer retained between accesses or across dispatch/callbacks. Existing index and stack-base reconstruction remains checked. `call_function_value_named` (`mod.rs:7882`) can push fibers and run nested scheduling; no stable-Vec-address assumption. |
| Frame changes, yields, GC safepoints, interrupts | Existing header, `frame_changed`, IP flush and stack-pointer refresh remain. Do not hoist/remove polling, change the quantum cadence or move live values off VM roots. |
| Binding and scope exposure | BIND_LOCAL disarms the state before installation. Scope exposure/capture stays Shared; later alias writes, nulls, deletion fallback and reinsertion remain on the existing bound path. |
| GC and JIT | Values remain in the same fiber stack rooted by `collect_garbage` (`mod.rs:9619`). Keep existing write barriers for dictionary-bound locals. JIT synchronization still uses the local helpers; do not infer JIT irrelevance from the measured harness disabling it. |
| Global/request namespace and concurrent Chunk use | No cache ownership/type/locking changes; sum stays a global. `Chunk: Send + Sync` and namespace guards unchanged. |
| Exceptions/finally and malformed bytecode | Retain checked indexing/error behavior; handlers/truncation must not leave an installed binding with an Unbound marker. No unchecked raw-local opcode. |

Canonical reference inspected read-only: `FunctionBoxContext.java:306–411` searches live local/arguments scopes; `ScriptingRequestBoxContext.java:232–256` searches script variables. Those contracts do not permit skipping observable scopes. This proposal bypasses only a resolver proven to return no binding, not those scope searches. Existing MatchBox deletion/reinsertion fallback differences are explicitly tested in `tests/scripts/vm_scope_bindings.bxs`; they are not changed or represented as JVM equivalence. No fresh JVM execution occurred.

Remaining proof obligation before retention: every binding-insertion and scope-mode transition must preserve the invariant. A source audit found insertions in `bind_local`, `slot_binding` parameter resolution and `set_local_value` parameter-write resolution; JIT reads these bindings rather than adding a separate insertion path. Tests and compiled verification are still required.

## 4. Pre-implementation prediction P1

Recorded **2026-10-30, before any implementation/build**, for this one design only. This is a conditional engineering estimate, not a measured saving, confidence interval or guaranteed upper/lower bound.

### Price replacement work instead of subtracting 553

The saved trace provides concrete reusable native fragments:

| Existing fragment | Instructions | Use in the estimate |
|---|---:|---|
| First frame/fiber acquisition and mode check, `0x418779` → `0x4187c2` | 23 | Budget one such prefix per local access, including the new state decision; not zero-cost dispatch |
| Read fallback, `0x41bdf2` → `0x41be77` | 43 | Conservative repeated-lookup case: retain the full existing checked fallback after the prefix |
| Write fallback, `0x41dc82` → `0x41dd21` | 48 | Same conservative treatment for a write |
| Indexed read tail, `0x41be57` → `0x41be77` | 11 | Reference-reuse case: after the prefix retain slot/base/address checks and actual load |
| Indexed write tail, `0x41dcfa` → `0x41dd21` | 12 | Reference-reuse case also needs one stack-base load, charged separately |

Two structural estimates, NOT compiled replacement traces:

- Reusing the acquired fiber/frame within each access: `3×23 + 2×11 + 12 + 1 = 104` instructions/cycle.
- Conservatively retaining existing fallback reconstruction: `3×23 + 2×43 + 48 = 203` instructions/cycle.
- Add **0–40** instructions/cycle for new result preservation, branch arrangement or surrounding code changes. Forty is an explicit planning allowance (roughly ten per access plus ten outside), **not an empirically established bound on the compiler**. Unexpected code generation can invalidate the range entirely. No benefit is credited for broader layout/inlining changes.

Thus:

`estimated replacement = 104 to 243`

`P1 net reduction = 553 − (104 to 243) = 310 to 449 instructions/cycle`

The other 1620 instructions are provisionally retained; disassembly must check that they did not grow outside the allowance. Expected whole continuing cycle: **1724–1863** instead of 2173. Approximately **310–449 million fewer instructions** over the million-loop region, subject to existing periodic/boundary-path limitations. Normalized predictions are **14.3–20.7%** of baseline and **16.2–23.4%** of the comparator gap, NOT a corresponding latency prediction. The zero-overhead scope ceiling is 553 (25.4% of baseline), not the P1 prediction.

**Introduced/displaced costs:** frame-reload classification, a small extra branch on noneligible local accesses, disarming at BIND_LOCAL, code size and code-generation interactions. Classification uses existing metadata/enum storage and should need no allocation or bytecode changes. It is amortized work, not work made free by the loop counter boundary. The measured loop's root-frame setup precedes its region; end-to-end and short-invocation effects are not established by this P1 region estimate. Noneligible workloads and feature configurations need regression checks. No quantitative claim about their overhead is made yet.

**Why retain this ranking after pricing:** even the estimated low end exceeds A's conditional gross ceiling by about 2x, while targeting a coherent local-resolution layer without B's cross-handler ownership change. This is a constrained best-next-step judgment, not proof that C dominates every possible B/D implementation.

### Falsification and mechanism checks before outcome measurement

- Eligibility: unchanged root frame must actually enter Unbound, with no per-iteration admission/classification work.
- Selected local paths must bypass all three `slot_binding`, three `SlotBindings::get`, two `Option::and_then` and one `Option::or_else` executions. Having their symbols elsewhere in the binary is not a failure; retaining them on these paths is.
- Retain actual checked stack loads/store and numeric/global/header operations. Compare generated replacement, including spills and outlined helpers, against the **104–243** replacement estimate and unchanged-work allowance. The old projection experiment demonstrated why helper disappearance alone is insufficient.
- If the compiler introduces unexpected adapter calls, eligibility cannot be proved, or code inspection no longer supports net savings, stop. Preserve P1 and document any dated P2/rejection **before** counters; no source-tuning variants automatically follow.
- Validate transitions with new tests for eligible read/write, late BIND_LOCAL, exposure/capture, POP/truncation and frame reload, plus exclusions for implicit-arguments UDFs and ComponentBody. Run existing scope binding/lexical closure/script-loop visibility, exceptions/finally, nested future callback, lifecycle, route isolation and JIT-contract suites, followed by full relevant suites. No test weakening; unavailable coverage remains explicit.
- Only after an approved implementation and native check: isolated candidate artifact, original profile/features/JIT assertions, same marked workload/checksum/boundary and sequential fresh-process reference/candidate repetitions. Preserve both artifacts and raw failures. Instruction counters do not establish speedup. Reconcile gains outside the range as rigorously as regressions.

## Decision and next authorization boundary

**Recommend C: an explicit Unbound frame state with checked direct stack access and binding/exposure invalidation.** Do not implement A first as an unexplained convenience, combine it with C, redesign the bytecode, or create a raw-pointer execution subset.

This pass delivered a selection, invariant and falsifiable replacement model—not a performance improvement. Product/reference remain unchanged. Stage 8 has not begun; no candidate correctness/build/performance results exist.

Proposed next scope, **pending approval**: at most 45 minutes for one implementation of C, one release candidate build plus required test builds, isolated outputs, contract tests and compiled-mechanism inspection. Stop on a disproved invariant, unsupported native savings model, exhausted time budget or unavailable required validation; no tuning variants. No runtime profiling or outcome-counter batch is included. Only if the build/tests/native review support the prediction should a separately bounded comparable counter batch proceed. No renewed debugger budget or automatic variant search is requested by this document. If the invariant or native budget fails, return to the comparison rather than spend the exhausted campaign budget under another name.
