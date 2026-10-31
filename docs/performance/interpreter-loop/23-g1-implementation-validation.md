# G1: owning-entry private caches — implementation and compiled validation

**Status:** one G1 candidate is implemented and contract-tested on the matching C3 source base. Its identified release artifact supports the frozen conditional **120-instruction/cycle** saving: **1093 → 973**. **There are no G1 hardware-instruction or timing outcomes, and no commit/merge/push.** This is not an autonomous optimization sequence or a general server-speedup claim.

## Authorization and source base

The user approved “Cool, lets go for it” after the [ownership/pricing audit](22-global-cache-ownership-pricing.md). This phase began **2026-10-31T04:23:54Z**, capped at90minutes, hard deadline05:53:54Z. Scope: one implementation, correctness tests, same-profile release build and static native validation; no tuning variants, performance-counter/timing outcomes, diagnostic sessions, installs, CPU/security changes or repository integration.

Work remains in `/home/jacob/dev/ortus-boxlang/matchbox-checksum-baseline-20261030T2203Z`, HEAD `7882dfa8bc2a824ac8717fab579d934b70b6d5fa` plus the identified patch. Its committed tree equals local main's C3 merge `a99b9263ae13887b648b4d0c68ecba807b74ed9a`. Main stays clean. Already-fetched `origin/develop` is `c2b6ec87609a1293389e7317e0061f9ba41f6a14`, three commits ahead; none were adopted, built or tested here. Prior incoming lifecycle review remains source evidence, not matching-base dynamic server validation. No refs were changed.

The exact million-iteration workload, arithmetic, output and storage are untouched. MatchBox's `sum` remains a VM global; Node CommonJS uses wrapper-frame locals. Expected benchmark checksum remains **500001500000**. No wrapper, literal/loop recognition, scaling, storage relocation, bytecode or evaluator change was made. The original benchmark was not executed in this phase.

## Implementation

Only two product/test files differ from the committed C3 tree:

- `crates/matchbox-vm/src/vm/mod.rs`: owning `VM::interpret(chunk)` now calls `self.interpret_chunk_borrowed(&chunk)`. The obsolete host-test-only description of that path is replaced with its actual script-entry ownership policy.
- `tests/route_cache_tests.rs`: seven additional regression tests; the original four remain intact.

The reused construction creates a private execution Chunk, eagerly ensures its slots, **moves it into Rc without cloning its initialized vector**, and creates separate fresh pseudo-root metadata. It uses the same scheduler as the old owning entry. No new accessor/adapter, IC representation, field, VM phase, persistent pointer, GC root, opcode, source-program mutation or hot-handler change was introduced.

Normal user functions and recursive calls keep their shared `execution_chunk()` caches and program/namespace checks. `interpret_sync`, spawn and debugger-start construction policies are unchanged. Mode-aware global/property/JIT access remains authoritative. No assumption of permanent VM thread pinning is made: the existing ESP32 serialized outer VM mutex is preserved.

## Correctness evidence

**1030 top-level test passes; 29 existing ignores; zero new ignores or weakened tests.**

| Final configuration | Passed | Existing ignores |
|---|---:|---:|
| VM library, JIT + debugger |85|0|
| VM library, no default features/JIT |83|0|
| Full integration |197|0|
| Selected no-JIT integration |18|0|
| BoxLang compatibility + web/debugger |543 +40|29|
| Root library + lifecycle/namespace/request/route-cache/rope |53|0|
| Route/entry cache suite, no default features/JIT |11|0|

The preliminary11-pass entry suite is excluded from this final total because it is repeated in the final lifecycle run. Native Fusion's nested child-test summary is also excluded from the integration total. No capabilities were disabled to obtain the release result; no-JIT configurations are additional correctness coverage, not performance builds.

New tests check:

1. A live owning entry has populated private Global ICs, an empty inactive shared store, independent empty pseudo metadata, and unchanged source/program sharing. Metadata equality against a fresh cache-free Chunk detects an accidentally copied private vector.
2. Private Monomorphic/Polymorphic/Megamorphic/Global lookup/set/slice behavior, shape transitions/deduplication, Global nonconversion, OOB lookup/growth, full-Chunk clone isolation/PartialEq, and serialization that omits runtime caches.
3. Reusing one source across independent VM layouts and isolated namespaces, retained reader callbacks after GC, error restoration and source-cache isolation.
4. Four live recursive function frames share normal function caches; another same-VM spawn's construction reuses that cache. Separate capture-free host metadata exercises the weak-program identity guard and completes the replacement program correctly.
5. Native current-task reentry releases Chunk borrows, restores the parent Chunk identity, preserves results, and explicitly retains an execution Chunk/cache beyond frame pop and GC through an Rc handle.
6. Ordinary shared Chunk clone/poison behavior remains unchanged; the shared path still locks and rejects poison.
7. Independent host threads share immutable source programs, not mutable execution caches. Existing compile-time Send+Sync and serde tests still pass.

Live-frame inspection uses a deliberately caught Rust panic, not a production hook. Those stopped VMs never resume instruction execution; this is **not** a recovery guarantee after arbitrary native Rust panic. Regular tests cover futures, roots/GC, callbacks, classes/modules, request restoration, JIT/OSR/deopt and debugger behavior. Source/type and native checks supplement them; there is no full-workspace/Miri/Wasm/ESP32/device/exhaustive-interleaving claim. External direct-runtime consumers and live pseudo-root re-invocation are not exhaustively dynamically covered.

One small **release correctness** script produced `63`, with the unchanged real compiler/tree-shaken prelude harness's JIT-inactive assertions. It used a different small generic function/property workload solely to check the executable: **not a reduced benchmark, counter sample, timing result, warmup or substitute for the unchanged million-iteration case**. No elapsed time was collected.

Canonical `ScriptingRequestBoxContext.java` and its tests were checked read-only at reference revision `02b3218aaafc37977aa227d90df15f98cd4e9754`. No reference fetch/build/JVM execution or arithmetic/storage parity expansion occurred.

## Build provenance

Same pinned offline/network-disabled Rust1.98.1 / LLVM22.1.8 builder and C3 release profile: root default features, `opt-level=z`, LTO, one codegen unit, panic abort, `RUSTFLAGS=-C strip=none`.

```text
cargo build --offline --release --example instruction_interpreter
CARGO_TARGET_DIR=/work/target/g1-build
```

The new target was independently seeded from C3's cache (no source-target hardlinks). Baseline/B1/C3 targets and original executables are preserved. Generated `src/stubs.rs` was restored after every builder invocation; the packaging asset already existed and was not touched. No formatter/toolchain component was installed; `git diff --check` passes.

| Identity | SHA-256 |
|---|---|
| G1 executable |`ad2ec4ebd6aa468b9c0a8ed6707bcecdc28ea27cd761b7fa6fbe8057f8842e74`|
| Product/test patch |`af2af2f7cc1b25c89b1d91d74a0d1d8bd504ff4ae14572df6f73dcee68cdd532`|
| Changed `vm/mod.rs` |`b092dac96d6317c326dfeb3bcdb11c43bfb88d2b7b9b31ec974947458a1c4c44`|
| Changed `route_cache_tests.rs` |`8f5d4664aa43db16db67bfa1bf7819bbf9cb5c9fcdbfd4bbaae5019fb332a77d`|
| Frozen C3 executable |`72585b4106c8876b0ed45c3dff1f2948ed1549c016698069c976b9a567f60f3a`|

A commit name alone does not identify this dirty-source build. Input manifest, exact patch, image/toolchain/profile/flags/commands and independent executable hash are recorded together in `build-input.json` and `build-provenance.json`.

## Native mechanism and net bill

Static executable inspection validates the new constructor's private mode, distinct metadata construction and lack of initialized-vector cloning. The ordinary global accessors now select the existing private arm. This is compiled transfer evidence, not merely an inference from unchanged hot source.

- All **18617 normalized instructions** in `run_fiber_inner` match C3's complete instruction sequence; selected actual CFG edges and calls are checked separately.
- All **95 dispatch entries** decode to the corresponding handler PCs in both ELF files.
- All **13 whole helper bodies**, selected offsets/call graphs, relevant data and the loop1.0 constant are checked. Ordinary callers are unchanged, and there is no new hot adapter.
- The accessor's shared hit remains85instructions with lock/guard/drop completion; the private hit is25 with **no nested helper**. The wide80-byte result copy, caller ABI and namespace/index/value checks remain paid. Identical-code-folded regex-cache drop naming is not regex execution.
- Shared/private empty81/20 and OOB77/16 are verified lookup alternatives, **not full miss/refill costs**.

### Disjoint two-hit ledger

| Work | Instructions/cycle |
|---|---:|
| Current C3 cache subtrees |170|
| Retained common-prefix PCs |16|
| Gross old-exclusive removal:54 accessor self +100 sync/drop helper self |154|
| Introduced private-arm replacement beyond common prefix |34|
| Candidate cache subtrees:16 +34 |50|
| **Net:154 −34 =170 −50** |**120**|

Do not claim170 saved, call replacement free, remove RefCell's18/cycle again, or stack old projection/B1/C3 opportunities. Relocation earns zero credit.

| Ordinary segment | Frequency | G1 total |
|---|---:|---:|
| Header |5|61 each|
| Composed GET_LOCAL producer |1|134|
| Remaining ADD |1|135|
| GET_GLOBAL |1|101 (C3:161)|
| SET_GLOBAL_POP |1|97 (C3:157)|
| FOR_LOOP_STEP |1|201|
| Standalone Number-CONSTANT |0|151, unchanged|

**Main733 + helpers240 =973**, versus C3's733 +360 =1093. Actual selected compiled deviation from P1's template is **G=0**. This is still conditional on the original ordinary warm float/Unbound/continuing-cycle, quiet-observer/no-GC/JIT-inactive assumptions—not an observed dynamic-frequency or hardware-count result.

## Cold, resource and other-user costs

The new entry invokes fresh-runtime construction twice instead of once plus a shared-Arc Chunk clone. Native code requests **one additional48-byte Arc/Mutex allocation, alignment8**, plus fresh HashMap initialization. That is a native allocation request, **not total allocator/RSS cost**. Initial direct private ensure may remove cold locking, but full setup/drop/first-hit/refill/periodic/boundary/final changes are unpriced signed terms, not zero.

| Native artifact/resource | C3 | G1 |
|---|---:|---:|
| `run_fiber_inner` stack reservation |2808B|2808B|
| Entry routine stack reservation |728B|728B|
| `run_fiber_inner` code |90778B|90778B|
| ELF `.text` |5175779B|5176099B (+320)|
| Executable file |13684952B|13685432B (+480)|

No production program/frame/VM fields or GC roots were added; frame stride216B is unchanged. Full call-chain peak stack, allocation overhead/RSS and short-script/time effects remain unmeasured. A new outlined entry helper has its own stack work; unchanged routine reservation is not unchanged process peak-stack proof.

One eagerly initialized dense80-byte-slot vector serves the initial native execution Chunk; pseudo metadata has no dense vector and construction never clones a populated one. That is **not** a universal one-vector-per-program claim: full cloning of an active private Chunk copies the vector; re-invoking pseudo metadata through the unchanged shared-function path can initialize its separate shared store while an old private execution Chunk is retained. Rc cloning alone does not copy it, and cache lifetime follows that Rc-held Chunk, not necessarily frame pop.

Pseudo metadata no longer mirrors the active owning-entry ICs, already the borrowed-entry policy. External Rust/native direct-runtime inspection/poisoning or root-metadata reuse may observe this difference; inventoriable production consumers do not require the old alias, but external callers are not proven absent. The supported Chunk cache APIs remain mode-aware. This is not silently claimed to preserve the old metadata alias contract.

Normal function/recursive shared access is still85/hit; existing borrowed routes were already private. **No ordinary-function, per-request server or incoming persistent-app/worker throughput benefit is predicted.** Incoming3 commits, ESP32/Wasm targets and host migration/interleaving behavior were not dynamically built/tested here. Preserve ESP32's outer serialization.

C3's noneligible-local45–65 handler-instruction taxes and standalone-CONSTANT +1 remain, as does its unmeasured broader-workload impact. G1 does not reclaim them or resolve B1's32/cycle attribution remainder or C3's4165 aggregate prediction shortfall.

## Prediction freeze and stop

Original G1-P1v1 and concurrency/lifetime clarification P1v2 remain unchanged. **G1-P2-v1** was frozen at **2026-10-31T05:01:12.222763+00:00**, after compiled/correctness validation and before any G1 performance outcome:

```text
SHA-256 9be86c1ff3be775b39e54bba3464e7a6d129e7ac09037a2097f87cfef4e00e11
ordinary cycle973; net120/cycle; compiled G=0
scoped million-batch saving =120,000,000 −D
whole-process saving =120,000,000 −D −S
```

D is signed changed nonordinary/cold/periodic/boundary/final/residual work inside the original region; S is signed changed process work outside it. Neither is bounded or assumed zero. The ordinary973-million bill is **not** a prediction of the exact scoped hardware total. No B1/C3 residual is inherited/subtracted, no absolute candidate batch total is asserted, and there is no timing forecast.

One preliminary `git diff --check` caught a trailing space and prevented test launch; it was fixed. NativeCallback construction and Constant metadata fixture types were corrected by source inspection before compilation. The first launched contract suite, final regressions, release build, native verifier, cold/resource review, small release correctness process and prediction freeze all passed without product variants or workload retries. Known missing rustfmt was not installed; native binutils came from the existing pinned image.

Evidence: `benchmarks/instructions/results/g1-implementation-20261031T042354Z/` in the investigation worktree. It includes preflight/protected-packet verification, source/patch/provenance, all test/build commands/logs, identified executable, complete disassembly/symbols, conditional model/CFG/helper/data/dispatch checks, constructor/resource review, original P1s, frozen P2 and the release correctness record. All13 prior sealed packets/809 listed files, frozen binaries, reference and worktree heads are preserved. Raw binaries/packets remain ignored/local; this documentation does not transfer them.

**Stop after sealing this phase.** G1 delivered benefit is unmeasured and the source is uncommitted. Hardware outcomes, separate timing, affected-workload/resource studies, source-base integration, commits/merge/push or cleanup need a separate bounded decision. Do not silently switch to incoming code, reset exhausted diagnostic budgets or run another candidate.
