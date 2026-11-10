# C3: one guarded-composition implementation and native validation

## Status and authorization

On “Go for it,” the user approved **one C3 implementation, contract-test, release-build and static native-validation phase**, not performance outcomes. Start: **2026-10-31T01:11:26Z**; self-imposed 90-minute hard stop: **02:41:26Z**. Final preservation/sealing time is recorded in the packet's `final-state.json`.

**One candidate is implemented and tested. Its verified conditional ordinary cycle is 1093 instructions versus B1's 1423: 330 fewer/cycle. No checksum counter, checksum timing, profiler/debugger session or optimization loop was run in this phase.** This is compiled pricing, not a measured performance result. There is a material cost on noneligible local reads, so this is **not an adoption/merge recommendation for all workloads**.

The original [C3-P1-v1](18-c3-composition-pricing.md) remains unchanged. A separately dated **C3-P2-v1 was frozen at 2026-10-31T01:56:54.341289+00:00**, before outcomes. Its 330/cycle point is a pre-outcome refinement, not a rewrite of P1's conditional 218–297 range. Neither the source-only range nor its historical 0–49 growth allowance was a guaranteed bound.

## Candidate and preserved scope

Worktree: `/home/jacob/dev/ortus-boxlang/matchbox-checksum-baseline-20261030T2203Z`, HEAD `4957e5eb6c3c03134c56d31b6142167d3da3a527`. Its committed tree equals main `develop` at `85383be78cda2397207cc51736c3be11c96c1dcd`. Main remains clean, ahead 13, unmodified; no commit, merge, push, reset or cleanup was authorized here. The previously uncommitted pricing report is preserved.

Product/test changes:

- `crates/matchbox-vm/src/vm/mod.rs`: a direct checked branch in the existing `GET_LOCAL` handler; test-only probe wiring is `#[cfg(test)]`.
- `crates/matchbox-vm/src/vm/composition_tests.rs`: direct contract tests and per-VM, test-only observations/injection.
- `tests/scripts/vm_expression_composition.bxs` and `tests/integration_tests.rs`: one registered, general local-expression/capture/callback script.

No compiler, bytecode, JIT decoder, source workload, iteration count, storage location, literal value or loop-bound recognition changed. There is no new opcode, hidden interpreter, cache certificate, production VM/frame field, persistent pointer, phase state or new cache allocation. Normal stack-vector growth remains possible through the existing pushes; it is not claimed allocation-free or newly introduced storage.

### Authority, admission and fallback

The handler resolves the local **once**, through the existing authoritative `fiber.local_value()`. It then admits composition only for a float local, tracing off, debugger run inactive, both VM-owned native dispatcher collections empty, and collection not due. A short checked `try_borrow` verifies current stream pointer/length, the next two original one-word instructions (`CONSTANT`, then the exact generic `ADD` word), current `Constant::Number` classification, and a populated current-frame promoted slot holding a float. Failed checks decline to the old stream, pushing the already-resolved local. Ordinary `read_constant()` remains responsible for misses, other classifications and captures.

The successful interval has no whole-VM call, callback, VM heap mutation or completion-registry mutation. External senders can change the channel, not the two VM-owned fields deciding whether the next headers drain it. `!heap.should_collect()` with no heap mutation also proves the internal prompt/periodic collection checks false. This is the specific proof for omitting **two no-op completion/GC checks**, not permission to combine arbitrary observer opportunities.

Both interrupt loads remain in source. The cold branches materialize the original rooted evaluation prefixes and use existing `vm_throw!`/IP flushing/handler authority:

| Path | Stack above original prefix | Next IP | Extra logical ticks |
|---|---|---|---:|
| Before CONSTANT interrupt | left | after original CONSTANT word | 1 |
| Before ADD interrupt | left, literal | after original ADD word | 2 |
| Normal success | numeric result | after original ADD word | 2 |
| Admission failure | left | original CONSTANT word | 0 here |

Wrapping counters preserve the GC/backedge phase. Debugger/trace-active execution retains ordinary instructions, intermediate stacks and resumption. Result arithmetic is the source expression `left + right` through `BxValue::new_number()`, with numeric strings, ints, arrays and captures retaining generic semantics. The release compiler commutes the physical XMM addition operands; no stable NaN payload-bit or floating-environment contract beyond existing Rust `f64` behavior is claimed. Constructor/tag normalization remains, and finite values, signed zero, infinity and NaN behavior are tested. This is not a new JVM/BoxLang high-precision/coercion parity claim.

## Contract validation

Final successful configurations total **1012 top-level test passes, 29 existing compatibility ignores**:

| Suite | Passed | Existing ignores / notes |
|---|---:|---|
| VM, JIT + debugger | 85 | 0 |
| VM, no JIT | 83 | 0 |
| Full integration | 197 | 0; native-fusion child pass not counted twice |
| BoxLang compatibility | 543 | 29 |
| Web runtime/debugger | 40 | 0 |
| Selected no-JIT integration | 18 | 147 filtered, not ignored |
| Root/lifecycle/namespace/request/route-cache/rope | 46 | 27 + 5 + 2 + 6 + 4 + 2 |

Commands, toolchain identities, serialized logs and exits are retained. Final integration includes existing real JIT contracts (`jit_hot_fn`, `jit_iter`, `jit_osr_loop`, type guards, deopt/recompile and VM-local compiled metadata), not merely enabling a flag. The added test checks that composition itself does not disable JIT. Both default-feature and no-default-feature configurations were exercised.

Direct cases include cache miss then hit; current class changes and fresh closure/component captures despite a populated cache; integer/string/array fallback; float edge cases; stack/shared-dictionary authority; native handle exclusion until terminal drain; forced-GC/resumption; wrapping and expired-backedge quantum phase; both interrupt prefixes and catch-handler execution; truncated lookahead; isolated-process tracing; and debugger budget one/two with intermediate stacks and resumption. General integration exercises nested frames/captured callbacks; existing lifecycle tests cover cancellation, late native completions, request restoration, roots and namespace separation.

**Limits:** not every defensive owner-identity, borrow-count or malformed-index failure was dynamically forced through the complete interpreter. Those checked declines are source/native-reviewed; arbitrary public-state corruption, every actual signal interleaving and every physical allocator relocation are not exhaustively tested. The deterministic interrupt injection is test-only, not a profiler/debugger session or production phase field. No full-workspace, Miri, Wasm or ESP32 validation claim is made. There are no weakened tests or new ignores.

Preserved failures/corrections within this one design:

1. A debugger fixture crossed source lines, stopping on line stepping before budget two. The final fixture puts the three words on one line; original fixture and failing log remain.
2. The new script initially used nonexistent `assertEquals`; it was corrected to existing `assert` syntax. The 196 existing integration tests already passed; the final full run passed 197.
3. Reflink cache seeding was unsupported for some files. Original failure/empty-file evidence remains; Cargo rebuilt dependencies, and independent copies filled only missing/empty candidate-cache files. The B1 target was not built in or modified.
4. `rustfmt` was absent in the cached toolchain; the failed attempt is retained. No component was installed. `git diff --check` passed; there is no whole-workspace formatting/clippy claim.
5. Initial static walker boundaries/branch assumptions required review; failed/incomplete logs and script snapshots remain. They are analysis corrections, not product variants. Host `nm` was absent; existing pinned-image binutils supplied static inspection without installation.

## Reproducible executable identity

Same B1 pinned cached builder: Rust/Cargo **1.98.1**, LLVM **22.1.8**, image `docker.io/library/rust@sha256:f31fa9eaac4e417505e10009786ec0c636558e02dff3fa662cc92c10894ba065`. Offline/network-disabled build, root default features, `opt-level=z`, LTO, one codegen unit, `panic=abort`, `RUSTFLAGS=-C strip=none`. Separate `target/c3-build`; generated `src/stubs.rs` restored.

```text
cargo build --offline --release --example instruction_interpreter

four-file patch SHA-256:
52ea408f4925b66636a46b2e89e92c0147e69713183b6bf7c8e9e53352164e76

candidate executable SHA-256 (13,684,952 bytes):
72585b4106c8876b0ed45c3dff1f2948ed1549c016698069c976b9a567f60f3a
```

`build-input.json`, `build-provenance.json`, the patch and final source snapshots identify the build; product sources are unchanged since build input. A future commit would not establish provenance of a different preexisting binary.

### Interrupt code in compiled artifacts

As in B1, the JIT-inactive instruction harness's release LTO folds its unwritten interrupt flag, so **the two new loads cost zero in this particular priced executable**. They were not removed from the engine. Separate **static inspection only** of the production CLI already built by the final no-default-features correctness command shows two actual `Atomic<bool>::load` calls at the two new source locations, separate conditional branches, both stack/tick/IP materializations and calls to existing `throw_error`. Its hash, symbol, line-annotated native function and review are retained. That CLI is a debug/no-JIT artifact, not the release timing/count candidate and not evidence for release-CLI cost. No additional CLI build or target execution was needed for this check.

## Verified native mechanism and complete cycle

`verify-c3-model.py` checks selected main paths, all 13 whole normalized helper bodies against B1, selected helper offsets/call graph/self bills, relevant loaded data, dispatch targets and the complete cycle. Whole-function/whole-binary identity is **not** claimed. This remains a conditional static model, not a freshly observed trace or measured frequency distribution.

| Ordinary segment, B1-comparable boundaries | B1 | C3 | C3 frequency |
|---|---:|---:|---:|
| Header | 61 | 61 | 5 |
| GET_LOCAL / composed producer | 57 | 134 | 1 |
| Standalone CONSTANT | 150 | 151 | **0** |
| Remaining generic ADD | 135 | 135 | 1 |
| GET_GLOBAL | 161 | 161 | 1 |
| SET_GLOBAL_POP | 157 | 157 | 1 |
| FOR_LOOP_STEP | 201 | 201 | 1 |
| **Full ordinary cycle** | **1423** | **1093** | |

The new physical layout puts two existing counter-snapshot instructions in each handler tail rather than at the raw header start. Raw header cost is 59 and raw composed body 136; comparison above assigns those two instructions back to the header (61/134). **This relocation receives zero saving credit**. The verifier retains both actual PC paths and this explicit normalization.

- Main self: **848 → 733**; helper self: **575 → 360**; total **1423 → 1093**.
- Fiber acquisitions: **10 → 5**, as intended.
- The composed body calls only the existing authoritative local index helper and final stack push helper; **no new hot projection/packing/composition adapter helper** exists.
- Existing numeric, stack, cache/lock/poison/drop and reader helper bodies transfer under normalized whole-body verification. Relevant helper load data and the loop's `1.0` datum are checked. Cache/lock/poison/drop self **170** remains charged; no cache ownership saving is claimed.
- Outside the local/literal/ADD envelope, the ordinary **898** instructions remain **898 net**, including the real global access, outer ADD, update and loop. Equal cost is not an assertion of unchanged instruction scheduling or microarchitecture.

### P1 → compiled P2, with costs rather than gross subtraction

P1's disjoint gross removal remains **383**. The compiled code additionally avoids **22** previously retained setup/routing instructions: eight from the old producer's push/control interface, fourteen from old ADD operand/result routing and a duplicated threshold materialization now shared with a paid admission guard. Every extra PC is audited against P1's credits, by opcode context, without overlap.

Actual new work is **75 main instructions**, no new helper self:

| Introduced replacement | Compiled instructions |
|---|---:|
| Pending/deque admission | 4 |
| Collection-not-due admission | 3 |
| Trace admission | 2 |
| Lookahead, opcode tests, index decode | 12 |
| Stream pointer/length identity | 7 |
| Checked frame/borrow/current Number/promoted hit | 31 |
| Two float admissions | 6 |
| Interrupt checks in this harness | 0 |
| Wrapping logical ticks | 3 |
| IP advance | 1 |
| Scalar extraction/state transfer/routing | 6 |
| **Total** | **75** |

The literal-access template is **48 → 31**, not free: all current classification/cache/borrow checks remain charged, but inline checked access avoids old helper/interface cost. Relative to P1's lower replacement case 86, the complete replacement is **11 cheaper net**; other rows include both favorable and unfavorable changes. Do not also subtract the old 15 helper instructions: their interface/implementation changes are already in this comparison.

Independent expression partition: **61 header + 32 real local resolution + 27 arithmetic/boxing/result push/control + 75 new work = 195**, versus 525. Therefore:

```text
383 planned disjoint removal
+22 additional audited setup/routing/reuse credit
−75 actual replacement/guard/state cost
 −0 net ordinary surrounding growth
=330 instructions/cycle

195 expression + 898 other ordinary work = 1093
733 main self + 360 helper self             = 1093
```

P1's best conditional saving was 297. The additional **33** is **22 additional credit + 11 cheaper net replacement**, not observed savings, a rewritten prediction, or a reuse of B1's unexplained batch residual. Historical B1's 32/cycle causal residual is not resolved by this pricing.

## Real noneligible/cold and resource costs

All following are **selected static GET_LOCAL handler-only costs** for an Unbound local with the stated branch assumptions; the same counter-boundary normalization applies. They exclude additional trace output, native draining, collection, subsequent CONSTANT miss work, other scope-mode costs and timings. They are **not whole-workload regression measurements or universal bounds**.

| Reason to decline | New body | B1 body | Extra instructions/access |
|---|---:|---:|---:|
| Trace active | 59 | 57 | 2 |
| Left nonfloat | 62 | 57 | 5 |
| Native pending map nonempty | 66 | 57 | 9 |
| Collection due | 69 | 57 | 12 |
| Float, next opcode not CONSTANT | 102 | 57 | **45** |
| Current constant not Number | 112 | 57 | **55** |
| In-bounds promoted slot empty | 119 | 57 | **62** |
| Cached value nonfloat | 122 | 57 | **65** |

These charges can materially regress arithmetic with a local/variable RHS or other common nonmatching patterns. The warm standalone Number-CONSTANT path also grows **one body instruction** (150 → 151); it has zero ordinary-cycle frequency for successful C3 but remains real for other sites/fallbacks. First hits, misses, malformed metadata, non-Unbound resolvers and cold interrupts have additional work; their workload-level net is unknown, **not zero**. No further implementation variant was attempted to hide this tax.

Native `run_fiber_inner` stack reservation: **2808 → 2808 bytes**; no native stack-reservation growth on this artifact. Function symbol bytes: **90,393 → 90,778** (+385); entire ELF `.text`: **5,175,395 → 5,175,779** (+384, including alignment/layout effects). Code growth is real despite the ordinary instruction reduction. Existing frame stride remains 216 bytes; no production VM/frame field or program-vector payload was added. Native memory/cache/timing implications, other targets and broad workloads are not inferred from these counts. New guard temporaries, scalar moves and native spills/reloads on the selected path are included in the actual 75/retained budgets, not an invisible allowance.

## Frozen C3-P2-v1 and next decision

Under the verified ordinary-state assumptions, **one million ordinary cycles predict 330 million fewer instructions**, or **23.1905513%** of the existing B1 measured total and **28.2361076%** of its remaining Node gap. If *all other work were unchanged*, the batch point would be **1,092,993,338** versus B1's **1,422,993,338**. There remains an **unknown signed cold/first-hit/periodic/boundary/final/residual term**. B1's `−6662` whole-batch residual is not inherited, subtracted or declared GC/noise. **No timing forecast.** MatchBox's global `sum` versus Node CommonJS wrapper-frame locals remains an explicit comparator difference.

The compiled price supports considering a **separately approved outcome phase** for this one identified candidate. It does not establish its checksum result in the performance harness, actual instruction saving, timing benefit, broad adoption value or universal optimality. Nonmatching-site tax is a significant adoption risk and should not be hidden behind success on the fixed loop. Affected-workload evidence would need its own explicit scope before a general retention/merge decision.

No outcomes, further variants, commit/merge/push, cleanup, profiling/debugger session or automatic optimization sequence is authorized by this document. If separately approved, reuse the exact original scripts/checksum/iteration count/storage and identified executables, collect fresh B1/Node controls, and keep counters separate from unprofiled timing. Reconcile against **both preserved P1 and frozen P2**; keep any new residual signed and unassigned rather than forcing it into the cycle model.

## Evidence

Ignored/local packet, relative to the investigation worktree:

`benchmarks/instructions/results/c3-implementation-20261031T011126Z/`

Contains plan/preflight, eight prior-manifest checks, source-before/after identities, preserved failures/fixtures, final commands/tests/exits, build/patch/binary provenance, static extraction and reviewed branch paths, all helper/path/data verifications, guard/fallback cost partitions, resource comparison, frozen P1/P2, documentation copies and final manifest. `README.md` and `final-state.json` define the completed packet. The final manifest includes all listed files but not itself. Do not rerun scripts into this sealed packet. Documentation commits alone do not transfer these binaries or raw evidence.
