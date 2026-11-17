# S2: guarded live binding reads — validated and measured

## Result and integration

**Unchanged AWFY Sieve5000 uses 15.51% fewer scoped retired user-space instructions than fresh S1 controls in the JIT-inactive interpreter.** One candidate was implemented, validated and retained. No elapsed-time speedup or normal CLI/default-JIT result is claimed.

Implementation and sixteen new contracts are committed as **`033013a1968c864837c80d345acdcbc9f1b75b40`**, parent local S1 merge `b0413045015ea364f3faa3ea17aefde6c88f5a05`. The user separately authorized this documentation commit and a local `--no-ff` merge into existing `develop`. Exact source/docs/merge identities and preservation verification are recorded in the ignored finalization packet below. No incoming server history is adopted or pushed.

## Mechanism and preserved authority

`crates/matchbox-vm/src/vm/scopes.rs` adds a read-only `cached_binding_value` helper using existing binding shape/index metadata. It obtains the **current** frame and actual current local/arguments scope, checks GC liveness and actual Struct type with `Heap::get_opt`, compares shape and checks the property index, then copies the **current live value**. No BxValue, scope identity, persistent pointer or privacy certificate is cached. `Some(null)` is a hit, not absence.

`local_value` attempts this helper only **after unchanged Unbound/Stack mode admission, implicit argument exposure and ComponentBody handling**. A miss ends borrows and invokes the unchanged resolver. Same-shape mutation, aliases/captures, scope replacement, deletion/absence/reinsertion and current value types remain authoritative. Checked invalid/dead/retagged/short-record probes do not introduce an earlier unchecked panic. Malformed-host panic fixtures discard the VM; resumability after panic is not promised.

No new production field, opcode, persistent allocation/root, pointer lifetime, thread-pinning assumption or broader Stack eligibility. Reads do not intern/upgrade spelling, create a scope, change write readiness, allocate or invoke callbacks. Original setters, `Heap::get_mut` barrier/order, argument case handling and JIT synchronization remain unchanged. The helper returns a value to the existing producer; it does not bypass C3, ticks, IP/catches, interrupts, scheduler, GC, debugger or JIT policy. Existing immutable-program sharing, Send+Sync and VM/root/reentry/host synchronization contracts remain intact.

## Correctness and compiled validation before measurement

- **1,104 top-level test passes across selected configurations; 29 unchanged existing ignores.** Sixteen unique new contracts are repeated across feature configurations, not forty-eight distinct contracts.
- `crates/matchbox-vm/src/vm/scopes/binding_read_tests.rs` covers live aliases and null/current value types; shape growth, deletion/reinsertion and negative cache refresh; no-scope misses; argument spelling/readiness; current local/argument scope replacement; GC roots and fiber/frame/stack relocation; actual type/bounds/invalid/collected/scalar cases; Dense/Sparse/high/maximum slots; block aliases; mode/exposure/component/captured scope authority; nested frames; actual changed-number/string `GET_LOCAL + CONSTANT + ADD` dispatch.
- Retained selected integration, no-JIT, JIT/debugger, compatibility, lifecycle/cache/rooting and web contracts remain passing. Complete saved serial integration logs preserve all **197 parent identities/statuses**; inherited child output needs four explicit block completions, and nested Native Fusion summaries are not extra parent passes. Full workspace/Miri/Wasm/device/all-interleaving proof is not claimed.
- One matching-profile release interpreter build, one correctness-only invocation returned **669**, original pre/post JIT-inactive assertions passed. No counters/timing existed during compiled pricing.
- Source/actual-native route audit supports conditional R/C/G charges below. **198 actual direct main-callee target pairs** have equal normalized bodies; 52 reviewed loaded-data operands are byte-equal. Source-conditioned CFG pairing covers 13,554 states, with nine remaining gaps outside nonzero-source opcode traversals and no unresolved indirect jump. Twenty-seven retained indirect calls are not comprehensively followed. This is not dynamic tracing or whole-binary equivalence.

Only one product candidate was used, with no product revision or expectation weakening. Static audit corrections preserved their failed scripts/logs: the final selected-stack calculation is **96 bytes local /96 bytes argument**, including seven helper saves, three Dense-child saves and two return addresses. All push/pop rows were already in instruction bills; P2 versions changed resource prose only. Saved-log parser completion also required no test/build/target retry. The exact tested bytes are committed here; finalization reruns no suites or workloads.

## Fixed fresh instruction comparison

Original class-based port pinned to AWFY revision `74306fec151070fd07157cefeacf19e7e0bcdc89`, unchanged `benchmarks/runtime/awfy/boxlang/{Sieve.bx,Benchmark.bx}`. Fresh 5,000-element Boolean array, native `arraySet(flags,1,5000,true)`, one-based indexing, marking from `i+i`, verified result **669**. No Boolean-only shortcut, compiler recognition, arithmetic stand-in or native prime implementation.

Exactly **three fresh S1 control processes, then three fresh S2 candidate processes**, sequentially, with **one verified warmup and one measured `bench.benchmark()` per process**, 30-second target timeout. Same sealed absolute `count.bxs`/expected stdout, cwd `benchmarks/runtime`, real compiler/tree-shaken prelude and original JIT-inactive assertions.

Imports/construction/console priming/warmup precede START. Fresh allocation/fill/class calls/Sieve/global result assignment and ordinary allocation/GC/boundary work remain measured; verification/output follow END. Warmup does not establish a fixed GC phase or prime bindings in newly created method frames.

| Artifact | Raw retired USER instruction samples | Median | MAD |
|---|---|---:|---:|
| S1 control | 67,666,725; 67,666,939; 67,666,741 | **67,666,741** | 16 |
| S2 candidate | 57,174,225; 57,174,369; 57,174,421 | **57,174,369** | 52 |

**Saving: 10,492,372 instructions /15.50595143927502% of the fresh S1 control.** All six warmups/results verified 669, exact stdout, empty stderr and exit0. Every sample has one counted thread, aggregate=main, and every record satisfies **`time_running_ns == time_enabled_ns >0`**. No multiplexing/scaling, retries, deletion, batching, slopes or overhead subtraction. Evaluator tests: 13 passed, five existing opt-in live tests skipped; no extra live diagnostics.

Hardware retired USER instructions, kernel/hypervisor excluded; not bytecodes, cycles, code bytes or elapsed time. AMD Ryzen7PRO6850U, Linux7.2.8-1-default x86-64/glibc2.44; no affinity/governor/security/settings change. Three samples in block order are not a confidence interval or randomized crossover. Counter/controller/whole-process time fields are diagnostic, not timing outcomes.

Fresh S1 median is **150 instructions below** the earlier S1 result 67,666,891. Report drift without attributing or subtracting it; observed percentage uses the fresh denominator. Do not compound historical results as a new matched comparison.

## Frozen prediction and reconciliation

The prior investigation attributed a disjoint old warm helper core **197/local, 232/argument**, weighted by source-inferred **61,278 local /32,806 argument** reads: **19,682,758** instructions. This was not all removable work or a complete dynamic component bill. Required live dictionary authority, checked shape/type/bounds and full replacement/ABI work remain charged. Missing/uncached/shape-changing reads are separately excluded; a previous invocation's frame does not prime the next frame.

Selection compared C3 rejection suffixes, array-read probes, native bulk fill/scalar constants and broader bound-method authority. Larger authority redesign may remove more overlapping work but has unresolved visibility/materialization/invalidation/JIT/cold costs; it was not assigned zero benefit. This is a bounded, supported improvement, not a global-optimum claim or permission to run alternatives.

All original predictions remain immutable:

- **S2-P1-v1**, frozen `2026-10-31T22:28:38.014404Z`: prospective Rlocal100..140, Rargument140..180, C0..100,000 and G0..400,000 give **4,698,758–8,962,118 +U−D** conditional partial planning cases, approximately 6.94–13.24% of prior S1 median67,666,891.
- **Current S2-P2-v3**, frozen `2026-10-31T23:36:51.214656Z`: compiled source/route/CFG-conditional upper charges **Rlocal137, Rargument172, C98,000, G66,663**, giving **5,480,377 +U−D**, approximately 8.099052% of that historical denominator. P2v1/v2 remain preserved; selected-stack resource prose corrections did not change instruction charges.

These are **partial conditional cases, not complete outcome intervals, hard floors or universal bounds**. R includes the entire new main read protocol with zero retained-mode/frame credit; C caps679 excluded/uncredited reads at700×140; G charges positive growth/alternative paths conservatively and grants zero removal credit. Successful eligibility, general misses/Sparse and active-JIT behavior are not universally priced.

```text
19,682,758 −61,278×137 −32,806×172 −98,000 −66,663 =5,480,377

10,492,372 −5,480,377 =5,011,995
= U +61,278×(137−Ractual_local) +32,806×(172−Ractual_argument)
  +(98,000−Cactual) +(66,663−Gactual) −D
```

**All5,011,995 instructions above P2's conservative partial case remain unallocated.** Deliberate full-main overcharge, uncredited old main interfaces and conservative cold/surrounding charges support why the partial case can understate net benefit; aggregate counters do not measure their individual contributions. Nonnegative additional removal U and signed retained-runtime/GC/allocator/boundary effects D are unresolved, not zero or noise. No post-outcome repricing, component allocation or revised forecast. Observed saving also exceeds P1's high partial case by1,530,254; P1 was not a complete upper bound.

The prior [S1 result](26-s1-numeric-array-store-result.md) and its **7,925,725** excess remain separate and unallocated. No double-credit for formatter/store work, checksum model, Node-gap denominator or old campaign threshold.

## Resources, artifacts and evidence

Prior static facts: main stack reservation **2,808B unchanged**; selected new extra call-chain depth96B local/96B argument above caller, not whole-program peak or an old/new peak saving. Main code+185B, ELF `.text`+448B, executable+624B. No new persistent production allocation/state. More general fallback/cold/resource costs remain unpriced; code bytes and stack bytes are not retired instructions or a speed bill.

Matching pinned Rust/Cargo1.98.1/LLVM22.1.8 release opt-z/LTO/one-CGU/panic-abort/default-feature/strip-none profiles. S1 executable was built from `fa55653` plus its exact S1dirty source later committed/merged. S2 executable was built from **`b041304` plus the recorded dirty source**, not from the later source commit above. Raw reports record the clean main checkout separately; checkout identity alone does not identify the candidate binary. Commit/merge verifies matching blobs, **not a rebuild or relabel**.

| Item | SHA-256 |
|---|---|
| S1 control executable | `ac4552466eba1487e2591124fef0c21e19d73ad1dbe314003a8d816ac8c6152d` |
| S2 candidate executable | `951e3ea8f0bcc6ff0f3d2b4162adf36b3b6e1e36e62f70d9a89f7870a5286a23` |
| S2 tracked scopes patch | `cf4fcc5ee29b3ad328c23eaa8137db85ad89cf87457d1182b41dfe922a12d2a5` |
| S2 scopes source | `8f510546676079ca8cc53d2449fd4f409db92a9870ef631231de774e39e596ef` |
| S2 new contract file | `08d4fd995d6110c45564912ce2ffafb575bd65d89cf115770fc1897f450d6cfe` |
| S2 build-source manifest | `5f09388497fc9c41055d8e6cb00cf1515dba54a1c55deb76f61ca03cbccf5c3a` |
| Frozen P1 | `cc42052a8a5ca8c92fe3ffd100cd559201d818a3ef32fe49fcbcadc1987ced1e` |
| Current frozen P2v3 | `bacd5a59117c1c1ec66bb694f39c54d36d69eabd6130147ab84ac6fa039960f3` |

The tracked patch alone omits the formerly untracked test; its separately sealed source/hash and the source commit include that file. Raw packets remain under main worktree's **ignored/local** `benchmarks/instructions/results/`:

| Packet | Manifest SHA-256 |
|---|---|
| `sieve-followup-pricing-20261031T215220Z` | `a9d65d063d61a19edb7c5f7a12647e9c022acc5eac40570aed45daaf72e37c0a` |
| `s2-implementation-20261031T223231Z` | `a32208b8f2d9dc7cc402d77a427400c54fc1740a3c6cce2a06450ee3cf9ec977` |
| `s2-instructions-20261101T001914Z` | `36bde3847247a479c2a65cf408b48d983a88cdadf4eb1f770b58a6e97f77df48` |
| `s2-repository-finalization-20261101T020000Z` | written last after commit/merge verification; excludes itself |

Before finalization all **25sealed packets/1,822listed files**, exact source/refs/worktrees/reference and protected artifact/target hashes/inodes were reverified. Final audit repeats preservation and verifies staged/committed/merged product blobs, parents and the exact merge tree. Original packets are never rewritten; versioned reports do **not** transfer ignored raw binaries/evidence.

No universal read improvement, elapsed-time, whole-process/startup/compiler/RSS, normal CLI/default-JIT, server/device/Wasm or Node/JVM claim follows. Incoming `origin/develop` at `c2b6ec8` remains untouched and unvalidated with S2. Finalization adds no implementation/testing/build/measurement/cleanup work. Old autoresearch/debugger budgets remain exhausted; further experiments, incoming-source integration, push or cleanup require separate authorization.
