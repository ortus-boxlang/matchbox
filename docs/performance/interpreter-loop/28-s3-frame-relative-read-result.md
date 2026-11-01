# S3: frame-relative live binding reads — validated and measured

## Result and integration

**Unchanged AWFY Sieve5000 uses 5.03% fewer scoped retired USER instructions than fresh S2 controls in the JIT-inactive interpreter.** One candidate was implemented, validated and retained. This is not an elapsed-time speedup or a normal CLI/default-JIT result.

Exact tested/built/measured source and six new contracts are committed as **`9b116af928f215f23d8445d89b6e6dc5b7a88169`**, parent local S2 merge `10bad28dce9bfd92f43dc111f774f0a85d51013c`. The user separately authorized this result/index commit and a local `--no-ff` merge into existing `develop`. The ignored finalization packet below records actual source/docs/merge identities, parents, trees and preservation checks. No incoming server history is adopted or pushed.

## Mechanism and preserved authority

Production changes are confined to `crates/matchbox-vm/src/vm/scopes.rs`. Checked cached-binding reads and one shared arguments accessor now belong to `CallFrame`. `local_value` reuses its existing **second/post-exposure current frame**, rather than reacquiring the fiber/frame through the old cached-reader interface. The ordinary `BxFiber::arguments_scope` wrapper delegates to the current frame; the obsolete fiber-level cached reader is removed.

The frame is a short current borrow, not persistent authority. Original Unbound/Stack admission, implicit-argument exposure and ComponentBody priority still precede the cached read. Metadata only locates the **current actual live Struct property**: GC tag/id/liveness, actual type, shape, index and property bounds remain checked; **`Some(null)` is a hit**. A miss releases frame/stack/heap borrows before the unchanged mutable resolver. Aliases/captures, same-shape mutation, deletion/reinsertion and scope replacement remain live, not snapshots.

The shared arguments accessor preserves no-frame `None`, captured ComponentBody scope before ordinary arguments, arity before stack indexing, current stack values and original malformed-stack panic behavior. Malformed-host panic fixtures discard the VM; no post-panic resumability is promised. Reads do not allocate, invoke callbacks, create scopes, intern/upgrade spelling or change readiness. No new production fields, state, roots, allocation mechanism, opcode, unsafe code, cached values/scope identities, persistent pointers or broader Stack admission.

Setters and `Heap::get_mut` barrier/order, argument case/readiness, JIT synchronization, rooting/GC/reentry/relocation and current-program authority remain unchanged. Existing C3, IP/ticks/catches, both interrupt checks, debugger, scheduler and observer policies are not bypassed. Immutable program sharing/Send+Sync and serialized host movement remain intact; no permanent host-thread pinning is assumed.

## Validation before measurement

- **1,122 top-level passes across selected configurations /29 unchanged existing ignores**, retaining every previous S2 identity/status. Six unique new contracts run under three feature selections produce eighteen additional passes, not eighteen new contracts.
- `scopes/binding_read_tests.rs` retains all sixteen old contracts and adds no-frame, arity/current-slot, captured-priority/no-fallback, malformed-stack panic/discard, nested relocation/GC/return and post-exposure live-read coverage. **22/22 focused tests pass.** Only private hit plumbing in old fixtures was adapted; old assertions were preserved.
- Selected integration, no-JIT, JIT/debugger, compatibility, lifecycle/cache/rooting and web contracts pass. Saved serial output preserves **197 integration parent identities/statuses**, excluding nested Native Fusion child summaries. No full-workspace/Miri/Wasm/device/all-interleaving proof is claimed.
- One matching pinned offline release build and one correctness-only unchanged Sieve5000 invocation: result669, exact stdout, empty stderr, exit0 and original pre/post JIT-inactive assertions. Generated/packaging stubs were restored; independent build targets share no protected file inodes. No counters or timing ran during pricing.
- One product candidate, no source variant or weakened expectations. Finalization verifies the exact bytes without rerunning suites, builds or workloads.

## Fixed fresh instruction comparison

Original class-based BoxLang port, AWFY pin `74306fec151070fd07157cefeacf19e7e0bcdc89`, unchanged `benchmarks/runtime/awfy/boxlang/{Sieve.bx,Benchmark.bx}`. Each invocation allocates a fresh5,000-element Boolean array, uses native `arraySet(flags,1,5000,true)`, one-based indexing and marking from `i+i`, and verifies **669**. No Boolean-only shortcut, benchmark recognition, native-prime replacement, arithmetic stand-in or enlarged tiny-operation experiment.

Exactly **three fresh S2 controls, then three fresh S3 candidates**, sequentially, **one verified warmup and one measured invocation per process**,30-second timeout. Same sealed absolute `sieve-baseline-20261031T055014Z/count.bxs`/expected output, cwd `benchmarks/runtime`, real compiler/tree-shaken prelude and original JIT-inactive assertions.

Imports/construction/console priming/warmup precede START. Fresh allocation/native fill/class calls/Sieve/global-result assignment and ordinary allocation/GC/marker-resumption/boundary work count; verification/output follow END. Warmup does not fix GC phase or prime bindings in newly created method frames.

| Runtime | Raw scoped retired USER counts, in order | Median | MAD | Range |
|---|---|---:|---:|---:|
| Fresh S2 control |57,187,459;57,174,169;57,174,420|**57,174,420**|251|13,290|
| S3 candidate |54,300,214;54,300,303;54,299,995|**54,300,214**|89|308|

**Reduction:2,874,206 instructions /5.0270837902684455% of the fresh S2 median.** All six warmups/results verify669, exact stdout, empty stderr and exit0. One counted thread each, aggregate=main; every counter record satisfies **`time_running_ns == time_enabled_ns >0`**. No scaling/multiplexing, retries, exclusions, batching, slopes or overhead subtraction. The first valid control is13,039 above the control median; it was retained and its cause remains unresolved, not assigned to GC/noise/controller effects. Evaluator tests:13 passed, five unchanged opt-in live tests skipped; no extra live hardware targets.

Hardware retired USER instructions exclude kernel/hypervisor work; they are not bytecodes, cycles, code bytes or elapsed time. Native host AMD Ryzen7PRO6850U/Linux7.2.8-1-default x86-64/glibc2.44; no affinity/governor/security/settings changes. Three sequential block samples are not a randomized crossover, confidence interval or universal improvement claim. Controller/process timestamps are operational evidence, not timing results.

Historical S2 median57,174,369 versus fresh57,174,420 is **+51 instructions drift**, reported without causal attribution or subtraction. The observed percentage uses the fresh denominator; forecasts retain their historical denominator. Do not compound historical percentages as a new matched comparison.

## Frozen conditional native price

The read-only proposal compared overlapping C3-rejection, scalar-constant, native-fill, numeric-read, binding-write and wider scope/header alternatives without treating unpriced work as zero. This is one supported bounded improvement, not a global-optimum claim or authorization to run alternatives.

Source-inferred61,278 local /32,806 argument warm reads weight the old S2 exclusive core **104/local,139/argument =10,932,946 instructions**. Old33 retained main rows are not removable core. New complete selected protocols **107/134** include main84/97, Dense child23 and the argument's outlined `bool.then` child14. Credit only **31 current retained rows**, giving **R76/103**, weighted8,036,146. No removal credit for old33/current31 differences. The entire outlined library child/ABI is paid, not ignored because its source looks small.

| Disjoint conditional planning charge | Instructions |
|---|---:|
| Replacement R:61,278×76 +32,806×103 |8,036,146|
| Cold C:700×150 |105,000|
| G:positive surrounding CFG sites |135,540|
| G:complete C3 predicate/push/phi/exit growth |206,548|
| G:shared arguments-wrapper users,400×64 |25,600|
| G:entire new33-row Chunk constructor×10 |330|
| **Total G** |**368,018**|

Cold source-local failure prefix≤99 plus the entire27-row slow main interface≤126; excluded successful107/134 paths also fit150. No old-probe/interface credit. Initially unprimed parameters decline before the selector;673 body exclusions concern local metadata/shape transitions. This closed program does not later mutate argument shape. A general late argument-shape miss, Sparse/shape-changing/callback/panic or active-JIT path is **not** bounded by the selected126-row cold route.

All sixteen C3 decline routes grow by2; success grows by5 through actual push/phi/exit. Source caps give `(94,757−5,668)×2 +5,668×5 +6×5 =206,548`, a conservative bill, not measured component growth. Shared-wrapper ordinary work is28 self+14 child=42;64 includes full VM ancestor6/fiber index6/caller6 and explicit margin4, without old-wrapper credit.400 calls is a reviewed closed-Sieve source-flow cap, not a general API bound. Repeated local setters/cold reads and native arraySet's5,000 actual Array mutations do not request arguments scopes per element.

Conditional CFG pairing:12,139 states,95/6/9 jump tables, no unresolved indirect jump. Six residual gaps are source-zero specialized inc/literal-bound/foreach opcodes. QuantumNone and class receivers excluding Stack-mode BIND_LOCAL are explicit workload conditions. Full GET_LOCAL/C3 cuts are separately priced; positive alternatives are summed, negative credits zero.171 actual direct target pairs have equal normalized bodies and51 reviewed retained loaded operands are byte-equal;25 indirect main calls are not exhaustively followed. These are conditional source/native correspondence checks, not dynamic traces, formal native-alias proof or universal library equivalence.

Static correction remains preserved: the first wrapper audit stopped on a mistaken inclusive46; actual enumeration is28+14=42. Failed original/log/exit and correction02 remain sealed;64-per-call charge, source/build/tests and frozen P1 were unchanged. The original P1 selector allowance19–24 also did not anticipate the outlined14-row thunk: complete argument R103 passes105, but that does not retroactively validate every template component estimate or justify tuning a second candidate.

## Predictions and outcome reconciliation

All pre-outcome versions remain immutable:

- **S3-P1-v1**, frozen `2026-11-01T02:35:53.405283Z`: prospective Rlocal70..85/Rargument85..105, C0..105,000/G0..400,000 give **1,774,686–3,854,976 +U−D** partial engineering cases, approximately3.10–6.74% of historical S2 median57,174,369.
- **S3-P2-v1**, frozen `2026-11-01T04:21:47.611602Z`: full compiled R76/103, C105,000/G368,018 give **2,423,782 +U−D**, approximately4.23928% of that historical median. The649,096 difference from P1's conservative case is compiler-stage R/G slack reconciliation, not measured component savings.

These are **partial conditional cases, not complete outcome intervals, guaranteed savings or universal bounds**. U≥0 denotes additional unpriced removals; signed D denotes unresolved retained-runtime/GC/allocator/boundary/data-dependent discrepancy, **not zero or noise**.

```text
10,932,946 −61,278×76 −32,806×103 −105,000 −368,018 =2,423,782

2,874,206 −2,423,782 =450,424
= U +61,278×(76−Ractual_local) +32,806×(103−Ractual_argument)
  +(105,000−Cactual) +(368,018−Gactual) −D
```

**All450,424 above P2's priced partial case remain unallocated.** Full-new cold/wrapper/constructor charges, summed positive alternative sites and zero old-main/removal credit support directional conservatism; aggregate counters do not measure their individual shares. No post-outcome repricing, causal allocation, per-read saving inference or assignment to D alone. Prior [S1](26-s1-numeric-array-store-result.md) excess7,925,725 and [S2](27-s2-live-binding-read-result.md) excess5,011,995 remain separate and unallocated. No old checksum/Node-gap/campaign-threshold transfer.

## Resources and artifact provenance

Main stack reservation **2,808→2,808B** and CallFrame216B unchanged. Successful cached-read selected additional call chain **96→32B**: new Dense saves24+return8; argument thunk's sequential alternative16B is not additive. **Shared ordinary arguments-wrapper direct-call chain24→48B (+24B)** remains a separate tax. Neither route establishes whole-program peak stack, RSS or device savings. Main code91,495→93,635B (+2,140); ELF `.text`+2,304B; executable+2,360B. Code/stack bytes are not retired instructions or timing.

Same pinned Rust/Cargo1.98.1/LLVM22.1.8 release opt-z/LTO/one-CGU/panic-abort/default-feature/strip-none profile. S2 control was built from `b041304` plus its recorded dirty source, subsequently committed as033013a and merged10bad28. S3 was built from **10bad28 plus exact recorded dirty bytes**, not from the later9b116af source commit. Source/blob verification at finalization is **not a rebuild or retrospective binary relabel**.

| Item | SHA-256 |
|---|---|
| S2 control executable |`951e3ea8f0bcc6ff0f3d2b4162adf36b3b6e1e36e62f70d9a89f7870a5286a23`|
| S3 candidate executable |`0227f5c0dadd7bad584913249386d269d6f7c8eb0179aef20fb5a1ed75fbe689`|
| S3 two-file patch |`a613433594b58048304b870d968dc8ae2be56145c96153f63674cc43046f911c`|
| S3 scopes source |`86a8294e782ed690df23f940cc1e6fa5df6aebdda410e9435cbde2d5499128cf`|
| S3 contract source |`2083d9732d2c44d67fd92b4df189ce7433d8793878b817c2f206531fb3ac9540`|
| S3 build-source manifest |`910156c64e194d6908784d6d441774c50f1f4d2ed4f47d580b02c7c5e7f4ec43`|
| Frozen P1 |`346d3f7f1496236ef11b13f05aa65addca98a38f04ab644e81b65e925654dba7`|
| Frozen P2 |`7d1d80d37e1f5b84bb7b11dc83248ae89e3e9565d4ed94d7424ebb2cb049e229`|

Raw evidence remains in main worktree's **ignored/local** `benchmarks/instructions/results/`:

| Packet | Manifest SHA-256 |
|---|---|
| `sieve-post-s2-pricing-20261101T022121Z` |`cfc582a718c3e01937bac0f7f824094e056ed263e72ff481962976a71654942c`|
| `s3-implementation-20261101T032902Z` |`5dc507344ec145630a9311b1d8782d48c62c876c032225b29f829c07312f938a`|
| `s3-instructions-20261101T043658Z` |`8a9b2e637880979f67f2cef7dd6ca20c4a810d904e987aa25a0d9d3ebd610e3e`|
| `s3-repository-finalization-20261101T045444Z` |written last after commit/merge verification, excludes itself|

Preflight reverified **29 sealed packets/2,198 listed files**, exact candidate/refs/worktree states/read-only reference and protected artifact/target hashes/inodes; final audit repeats preservation and verifies staged/committed/merged blobs, parents, tree and exact four-file allowlist. Original packets are never rewritten. Committing these reports **does not transfer ignored raw binaries/reports**.

No elapsed-time, whole-process/startup/compiler/RSS, normal CLI/default-JIT, Node/JVM, server/device/Wasm or all-input speedup claim follows. Incoming `origin/develop` at `c2b6ec8` remains unchanged and unvalidated with S3. Finalization adds no implementation/testing/build/measurement/cleanup. Existing exhausted campaign/debugger budgets remain closed; further experiments, incoming-source integration, push or cleanup require separate authorization.
