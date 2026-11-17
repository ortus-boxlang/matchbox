# Autoresearch contract: interpreter machine instructions

STATUS: preparation only until final user approval of the prepared scaffold,
baseline and checks. That explicit approval authorizes activation without editing
this frozen document. Do not initialize/activate experiments before approval.

## Goal and metric

Reduce general MatchBox interpreter overhead on the small arithmetic/function-call
script. Primary metric `instructions_per_iteration`, lower is better, in retired
user-space machine instructions. Node v22.23.3 `--jitless` is fixed external
context, not an implementation to optimize or score to game. This is NOT JIT work.

Workloads are frozen in `.auto/workloads/`: original script yielding 10, full
varying-input loop, and control/assignment-augment loops. Full body:
`var x=i; x+=2; var y=add(x,3); sum+=y;`, where `add(a,b)` returns `a+b`.
Expected checksum is n*(n-1)/2+5*n; n=1000 and 10000. Control adds i; augment
adds i+2. Outputs must match exactly the expected final line and marker sequence.
No unused results, constant-folded replacement, workload-specific checks or
algorithm substitutions. Primary loop has no per-iteration wrapper call.

Metric = (median large-batch all-thread instructions - median small-batch
all-thread instructions)/9000. Primary uses five fresh processes per size;
controls use three. Warmup=100 batches of 1000 with 1ms idle after each batch,
outside measurement, with stdout primed before warmup. Idle uses sleep in BoxLang
and Atomics.wait in Node. Both measured engines execute natively on the host.
Region boundaries exclude startup/source compilation/warmup/shutdown; include
START write/resume, END dispatch, loop/checksum, normal GC and all runtime work.
Do not interpret instruction ratios as elapsed-time speedups or pure ADD costs.

## Interpreter-only target and fixed build

`examples/instruction_interpreter.rs` uses the real compiler, tree-shaken prelude
and VM. It NEVER calls enable_jit and asserts vm.jit is None before/after execution.
Keep all root default features enabled; the JIT remains available to production
and correctness tests, but inactive in the measured harness. No feature removal.
Harness omits CLI-only module/include setup unused by these frozen scripts;
startup is outside the measured region. Do not compare its new baseline directly
to previous CLI measurements that allowed add() to JIT-compile.

Native x86_64 Linux on AMD Ryzen 7 PRO 6850U. Frozen builder is local Rust 1.98.1
image digest in `.auto/container.sh`, no pulls or network. Cargo.lock frozen.
Release profile unchanged: opt-level=z, lto=true, codegen-units=1, panic=abort,
strip=true. No RUSTFLAGS/PGO/target-cpu/profile edits. Native embedded runner cache
is unused and hash-frozen; only its mtime/feature cache marker is refreshed to
avoid irrelevant recursive rebuilds. Generated tracked src/stubs.rs is preserved
byte-for-byte around builds. Measurements never execute inside the builder.

## Editable implementation allowlist (only these four files)

- crates/matchbox-vm/src/vm/mod.rs — interpreter dispatch and call/return machinery
- crates/matchbox-vm/src/vm/calls.rs — compiled function argument binding
- crates/matchbox-vm/src/vm/scopes.rs — scope/local read/write paths
- crates/matchbox-vm/src/vm/struct_scope.rs — scope backing implementation

Everything else is read-only, including JIT, compiler, opcode definitions, types,
GC, tests/fixtures/expected results, dependencies, build profiles, .pi, harness,
evaluator, workloads, .auto/config.json, baseline and frozen manifest. Keep notes
only in ignored `.auto/ideas.md` or artifacts. Evaluation changes require STOP,
review and a new baseline, not an in-flight scoring change. Scope expansion also
requires stopping and review. Never edit reference/.

## Commands

- `bash .auto/measure.sh`: integrity, build, counters/validation and control/size
  gates; emits METRIC lines. Build time is not part of the instruction metric.
- `bash .auto/checks.sh`: interpreter probes, per-test-identity regression checks,
  independent instruction confirmation, control/size and integrity gates.
- Frozen baselines: `.auto/baseline.json`. Raw reports: `.auto/artifacts/`.
- Counter methodology: benchmarks/instructions/README.md and the measure-instructions
  skill. Follow the project-local matchbox-autoresearch skill as well.

## Correctness and secondary constraints

Require no regression of ANY previously passing test identity. Keep ignored
identities ignored; existing failures may pass but do not authorize other failures.
Capture all identities and complete test-runner summaries for VM unit tests,
canonical_keys, compiler error_reporting/member_shaking/cst_tooling integration
tests, root integration_tests, boxlang_compat_tests. The compiler lib target has
zero unit tests; it is not presented as a meaningful correctness gate.
Features are frozen in campaign.py. Compile errors, missing summaries, changed
identity sets or partial runs fail. Baseline failures must be reported explicitly,
not silently exempted by totals. Standalone interpreter probes cover original
arithmetic, calls, scopes, closures, required/typed/wide arguments.

Control and augment marginal instruction counts must not regress >5% against
preparation baseline. Interpreter harness executable size must not grow >5%.
No unsupported RSS/GC/latency claims. Each runtime process timeout=30s; build/test
command timeout<=600s. Per-process virtual address limit=8GiB, Cargo jobs=2 (not
an aggregate RSS cap). Entire active campaign limited to 60 minutes; evaluator
bounds commands by remaining time, with up to 10s forced-cleanup grace. Checks and
builds count against the active budget; preparation does not. No production access,
credentials, network, pushes, publishing or merging. API spend is not bounded by
an iteration cap; use provider-side limits separately.

## Acceptance and operation

Use only the installed pi-autoresearch extension for init_experiment,
run_experiment, log_experiment and rollback/commits. No replacement loop controller.
Run from this dedicated clean worktree on autoresearch/interpreter-instructions,
never the normal development worktree. Scaffold must be committed before first
experiment. Do not share the worktree with unrelated work; stop if it appears.

After final approval:
1. Activate `/autoresearch Run the approved interpreter-instruction campaign in
   .auto/prompt.md within its budget.` in Pi opened in THIS worktree.
2. init_experiment(name=interpreter-instructions, metric=instructions_per_iteration,
   unit=instructions/iteration, direction=lower), using the installed tool's schema.
3. Run unchanged baseline FIRST with `bash .auto/measure.sh`, benchmark timeout
   600s and checks timeout 1800s (internal command limits and 60min deadline apply).
   Log the actual result; no implementation edits until baseline logged.
4. Try one simple general hypothesis at a time, then run measure and mandatory
   checks. Keep only a >=1% improvement versus incumbent AND >3x the larger
   MAD-derived noise estimate, confirmed independently. Equal/noisy gains discard.
   Independent confirmation must agree within 1% or 3x noise. Never keep failed
   benchmark/checks. Record all results with secondary metrics and useful ASI.
5. log_experiment owns experiment commits/reverts; never manually commit/revert
   candidate implementations. Do not touch frozen evaluator to rescue a failure.
6. Stop at 10 logged runs INCLUDING baseline or 60min, whichever first; never reset
   segment, timestamps or increase limits to evade budget. Obey cancellation.

Preserve BoxLang semantics, including reflective local/arguments scopes, case
insensitivity, closures, aliasing, undeclared/named arguments, dynamic mutations,
events, coercion, GC rooting and error behavior. Consult JVM reference before
semantic changes. A read-only local copy of reference/boxlang at commit
02b3218aaafc37977aa227d90df15f98cd4e9754 is provisioned. The reference-boxlang
skill is not present in this checkout; inspect the canonical Java function/scope
implementations directly rather than inventing semantics. Never edit the reference.
Do not delete capabilities for performance. Favor avoiding unnecessary work safely over complex
speculative fast paths. No JIT edits or JIT-dependent wins.

## Starting hypotheses (not implemented)

1. Ordinary positional UDF call setup: temporary argument vectors, eager arguments
   dictionary population, execution chunk/frame creation and return bookkeeping.
2. Stack-local read/write and repeated scope binding: avoid redundant operations
   only when aliasing/reflective scope semantics remain correct.
3. Short numeric paths inside existing handlers; opcode/compiler changes are out
   of scope unless a later reviewed campaign expands the allowlist.

Prior CLI measurements found call-and-result block dominates the delta versus
Node --jitless, but those differences are NOT a sampled cost attribution. Establish
this harness's new interpreter-only baseline before drawing conclusions.

## Finish

Report unchanged baseline, best confirmed metric, Node comparison, tests, control
and size tradeoffs, rejected ideas and commits. Archive ignored logs/artifacts
before clearing. Do not push/merge. Keep learned results in .auto/ideas.md without
changing this frozen contract.
