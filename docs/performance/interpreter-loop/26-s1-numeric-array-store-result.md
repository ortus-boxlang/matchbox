# S1: numeric array-store routing — validated and measured

## Result and integration

**Unchanged AWFY Sieve5000 uses 20.48% fewer scoped retired user-space instructions in the JIT-inactive interpreter.** One candidate was implemented and retained; no elapsed-time measurement or normal CLI/default-JIT speedup is claimed.

Implementation and thirteen new contracts are committed as **`534cee4958434dac2fe8e1e3ebbce3a8daec4ab7`**, parent local G1 merge `fa55653076a462d185b074650fd644a8ed9da5cd`. The user separately authorized this documentation commit and a local `--no-ff` merge into existing `develop`. Exact implementation/documentation/merge identities and final verification are recorded in the local finalization packet below. No incoming server history is adopted, rewritten or pushed.

## Mechanism and preserved contracts

`crates/matchbox-vm/src/vm/mod.rs` classifies an indexed assignment before property conversion only when:

1. The index is an actual numeric `BxValue`.
2. A checked `Heap::get_opt` finds a live, actual `GcObject::Array` receiver.
3. The receiver has **no assignment hook**.

That route omits both numeric-to-property-name conversions and class-reference routing. A local `Option<String>` supports the unchanged generic path and **one authoritative array mutation arm**; no duplicate setter, early quantum continuation, Boolean-only restriction, compiler recognition, new opcode or persistent certificate/pointer is added.

The hook condition is essential: public safe host mutation can retag a watched Struct into an Array while its observer/root remains. Checked classification must not introduce an earlier unchecked panic for invalid/collected IDs. Noneligible receivers retain original repeated observable key conversion, class/native/proxy/struct ordering, callback roots/reentry/errors and assigned-value results.

Array mutation still uses `Heap::get_mut` with its generational barrier **before validation**, original integer/float casts and one-based errors, null-padding growth and normal IP/catch/frame-change/handler epilogue. Ticks, polls, interrupts, GC, debugger and JIT policy are unchanged. Numeric-string, fractional, negative, NaN/infinity and huge-index policy is not normalized in this optimization.

## Correctness and compiled validation before outcomes

- **1,056 final test passes across the selected configurations; 29 unchanged existing ignores.** Existing test identities/statuses were compared with G1, not merely aggregate totals.
- Thirteen contracts in `tests/index_assignment_tests.rs` cover numeric/cast/growth/alias/result/value behavior, errors, old-to-young array references, observer transformation and retagged-array hooks, catch/resume, rooted GC/reentry, native/class-reference dispatch, repeated nonnumeric coercion, bound-method captures and existing malformed-host/cast behavior. Malformed-host panic tests discard the VM; resumability is not claimed.
- Default, no-default/JIT-inactive and JIT/debugger VM configurations plus selected integration, compatibility, lifecycle/cache/rooting and web/debugger suites passed. Full workspace/Miri/device/all-input/all-interleaving validation is not claimed.
- One matching-profile release `instruction_interpreter` build; one uninstrumented correctness process returned **669**. No performance outcomes existed during compiled pricing.
- Actual-address native transfer verifies 279 matched direct callee pairs and all 16 previously charged non-main groups. Conditional CFG pairing covers 17,962 states and reviewed loaded data; two off-source states (`SET_PRIVATE`, `POW`) remain unpaired. This is not whole-binary equivalence or a dynamic trace.
- Main stack reservation remains 2,808 bytes. Main code grows 532 bytes, ELF `.text` 576 bytes, executable 816 bytes. No production fields or persistent allocations are introduced; local optional-state setup/drop is charged, not declared free.

An initial contract fixture retained host values incorrectly across forced GC; explicit retained roots corrected the fixture, with failed source/logs preserved and no production change or weakened expectation. A pre-build source freeze caught a test-generated copied packaging stub; the copy was restored and builder restoration fixed before the single release build. Analysis corrections are retained in raw evidence; no product variants or performance retries occurred.

## Fixed fresh instruction comparison

Original class-based port pinned to AWFY revision `74306fec151070fd07157cefeacf19e7e0bcdc89`, unchanged `benchmarks/runtime/awfy/boxlang/{Sieve.bx,Benchmark.bx}`. Fresh 5,000-element Boolean array, `arraySet(flags,1,5000,true)`, one-based indexing, marking from `i+i`, result **669**. No simplified arithmetic or native prime substitute.

Exactly **three control processes followed by three candidate processes**, sequentially, each with **one verified warmup and one measured `bench.benchmark()`**. Both artifacts use the same sealed absolute `count.bxs`/expected output, cwd `benchmarks/runtime`, real compiler/tree-shaken prelude and existing pre/post JIT-inactive assertions.

Imports/construction/priming/warmup precede START. Fresh array allocation/fill, class calls, Sieve, global result assignment and normal allocation/GC/boundary work are charged. Verification/output follow END. One warmup does not establish a fixed GC phase or warmed JIT.

| Artifact | Raw retired USER instruction samples | Median | MAD |
|---|---|---:|---:|
| G1 control | 85,090,588; 85,090,606; 85,091,215 | **85,090,606** | 18 |
| S1 candidate | 67,667,159; 67,666,667; 67,666,891 | **67,666,891** | 224 |

**Saving: 17,423,715 instructions / 20.4766611% of the fresh control.** All six warmups/results verified 669, exact expected stdout, empty stderr, exit 0. Each sample had one counted thread, so aggregate equals main-thread count. Every counter satisfies **`time_running_ns == time_enabled_ns > 0`**. No multiplexing/scaling, retries, sample removal or overhead subtraction. Thirty-second per-target timeout; evaluator tests: 13 passed, five opt-in live tests skipped.

Hardware retired user-space instructions, kernel/hypervisor excluded; not bytecodes, cycles, code bytes or time. AMD Ryzen 7 PRO 6850U, Linux 7.2.8-1-default x86-64/glibc 2.44. No affinity, CPU/security/settings changes. Three samples in block order do not establish a confidence interval or long-term environmental stability. Controller elapsed/counter-time fields are diagnostic only.

## Frozen prediction reconciliation

P1 and P2 remain byte-for-byte as frozen **before outcomes**. Source arithmetic expects 11,069 marking stores and 22,138 old format calls; those are inferred source frequencies, not measured dynamic traces. The 5,000 native `arraySet` initialization stores are separately included in scope, not extra marking-opcode credits.

Known disjoint removed subset: **898/store × 11,069 = 9,939,962** instructions. It includes selected formatter self costs, class routing, drops and caller interfaces, not a complete inclusive formatter/allocator bill.

- **S1-P1-v1:** conditional partial cases 8,733,441–9,729,651, with prospective replacement `R = 19..45/store`, surrounding `G = 0..708,416`, plus unpriced removal `U` minus signed runtime/boundary effects `D`.
- **S1-P2-v1**, frozen `2026-10-31T18:45:21.655582Z`: compiled/source-route-conditional upper charges `R = 37/store`, `G = 32,419`, giving **9,497,990 + U − D**, approximately 11.16% of the historical 85,091,158 baseline. Not a complete forecast, hard savings floor or universal array-store bound.

The observed saving exceeds P2's conservative priced-subset case by **7,925,725**:

```text
17,423,715 − 9,497,990
= U + 11,069 × (37 − R_actual) + (32,419 − G_actual) − D
= 7,925,725 unallocated instructions
```

This balance cannot distinguish additional removed work from unused conservative replacement/growth allowances or signed retained-runtime/boundary changes. Do not assign it all to formatting, infer `D` alone, call it GC/noise, or retroactively change the priced removal to fit. P1's partial-case interval was not a complete outcome interval/upper bound.

Fresh control is 552 instructions below the historical median; the primary result uses the **fresh** denominator without historical residual correction or causal explanation. No older checksum savings, Node-gap denominator, historical Sieve ~18% timing or old campaign 4% threshold transfers to this result.

## Artifact identity, raw evidence and limits

Both artifacts use the same pinned Rust/Cargo 1.98.1/LLVM 22.1.8 release opt-z/LTO/one-CGU/panic-abort/default-feature/strip-none profile. G1 control was built from `7882dfa` plus its exact validated patch; its product source later became G1's committed/local-merged source. S1 was built from **`fa55653` plus its exact dirty patch**, not retroactively from the later source commit above. Committing matching blobs links provenance; it does not rebuild/relabel an existing executable.

| Item | SHA-256 |
|---|---|
| G1 control executable | `ad2ec4ebd6aa468b9c0a8ed6707bcecdc28ea27cd761b7fa6fbe8057f8842e74` |
| S1 candidate executable | `ac4552466eba1487e2591124fef0c21e19d73ad1dbe314003a8d816ac8c6152d` |
| S1 validated patch | `ff75a71c8883ec40d107f497420e3d32b6d233ed442bfd1b4f4010b48a9d3b93` |
| S1 VM source | `dcfb63577af014fd83451b09b385fcf6070514043471744486011821c5c6de74` |
| S1 contract file | `06222bcf89ab49f9b8f7d842f0607aca5608c9a5b5db6e284b520993d2a5434c` |
| S1 build-source manifest | `84f603b54cfcc5eba8e58289696c240dba40bedfc6933dc645311b71b7e63f8c` |
| Frozen P1 | `f885442b2cf3ded394008bda506b8a4fcf2f04d561d5baf493ea5f34d98ab6b0` |
| Frozen P2 | `9656e82440720dc52aab30785ce065a5ba39932df925f403f593055ab656fbb0` |

Raw packets under the main worktree's **ignored/local** `benchmarks/instructions/results/`:

| Packet | Manifest SHA-256 |
|---|---|
| `sieve-baseline-20261031T055014Z` | `925c3ca287452cc88b2b925510b4d8245ea4ec2f659ef3b74d9b3b723c48aa6e` |
| `sieve-store-pricing-20261031T155523Z` | `6263c2547e1beb28c62f298fc969ec462c5344bdaf4e9a01a43462bf0b829dc6` |
| `s1-implementation-20261031T174444Z` | `b5b9c6feecf4ab1e20347a3398c434a4d829aba42424adae6663cca14d8ecf22` |
| `s1-instructions-20261031T205957Z` | `4ffde221eac4a5030cb520ead3f9ae704530cc2b12533bcadebe042e1891ca2c` |
| `s1-repository-finalization-20261031T212337Z` | written last after commit/merge verification; excludes itself |

Before finalization, all **21 sealed packets / 1,461 listed files**, protected artifacts/targets/inodes, canonical reference state, exact source and refs were reverified. Original packets are never rewritten. Versioned reports do **not** transfer ignored raw binaries/evidence.

Noneligible/watched stores pay the new predicate/optional-state costs and can incur an extra guard lookup; broader workload, cold/setup/drop/RSS and fallback performance remain unpriced. No universal store improvement, normal CLI/default-JIT, server/device/Wasm, Node/JVM or elapsed-time claim follows. Incoming `origin/develop` at `c2b6ec8` remains untouched and unvalidated with S1. Finalization performs no new tests, builds or measurements; exact previously validated bytes are committed. Original autoresearch and debugger budgets remain exhausted; timing, incoming-history integration, push or another improvement requires a separate bounded decision.
