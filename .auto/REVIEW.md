# Interpreter-instruction campaign review

This is an isolated campaign scaffold, not an active optimization loop.
No interpreter implementation changes have been made. Review `.auto/prompt.md`
and `.auto/baseline.json` after preparation completes, then explicitly approve
activation. Do not create an experiment log just to inspect setup.

## Files

- prompt.md: full frozen contract and stopping/acceptance rules.
- measure.sh / checks.sh: one-shot evaluator and mandatory correctness gate.
- campaign.py: builds, hardware counts, validates and compares per-test identities.
  It is NOT a loop controller; only the installed extension owns experiments.
- container.sh: pinned offline Rust builder with resource/time limits.
- examples/instruction_interpreter.rs (repository root): real compiler/prelude/VM,
  JIT inactive by assertions, default features and size-optimized release unchanged.
- workloads/: original and varying-input/control fixtures with observable results.
- baseline.json: new current-source interpreter baseline, Node --jitless reference,
  and all observed Rust test outcomes. Do not substitute old CLI/JIT measurements.
- frozen.json: hashes of non-editable tracked evaluator/source/test files.
- artifacts/: ignored raw reports, diagnostics, helper and session deadline state.

Preparation failed closed on missing cached dev dependencies (resolved from an
existing local Cargo cache, no downloads), a zero-test compiler lib target (replaced
with actual compiler integration-test targets), and nested test-runner output
(the integration suite reexecutes one test; parser now validates outer statuses,
including an explicit nested-success/outer-failure unit test). Failed reports are
preserved. These are evaluator calibration, not optimization attempts. No session
log/iteration count or active-budget timestamp should exist during preparation.

Run `python3 .auto/test_campaign.py` to validate evaluator arithmetic, sample
policy, complete identities, nested results and non-resettable budget behavior.
`python3 .auto/campaign.py dryrun` exercises unchanged measurement and checks
without starting the active budget; it refuses to run after activation.

## Established preparation baseline

- MatchBox interpreter: **27,981.564 instructions/iteration** (full workload).
- Node --jitless: **508.537 instructions/iteration**, approximately **55.0x** gap.
- Control/augment: MatchBox **3032.984 / 6444.976** instructions/iteration.
- Frozen harness executable: **8,492,616 bytes**.
- Nine interpreter probes and original y=10 check pass.
- Rust gates: **841 passing, 3 existing failures, 29 ignored**, 873 identities.
- All three existing failures require a JVM unavailable in the frozen offline
  builder: integration/test_java_import, integration/test_java_overloads, and
  compatibility/bf_dec_is_file_object. JNI remains enabled; these are not skipped
  or counted as passing. Every existing passing identity must remain passing,
  ignored identities must remain ignored, and no new failure is permitted.
- The compiler library has no unit tests; its actual error-reporting,
  member-shaking and CST tooling integration suites all pass (26 identities).

This is a new current-source no-JIT harness baseline, not the previous prebuilt
CLI result. Full raw sample/test reports and failed preparation diagnostics remain
under artifacts. This approval must acknowledge the three environment-limited
JVM checks; broader JVM-backed validation is not claimed.

## Activation after final approval

Open Pi **in this worktree**, not in the parent setup worktree:

    cd /home/jacob/dev/ortus-boxlang/matchbox-autoresearch-interpreter-instructions
    pi

The pinned project-local extension installation is already copied from the setup
worktree. No global installation or network operation is necessary. Grant project
trust only after reviewing the local setup. Then run:

    /autoresearch Run the approved interpreter-instruction campaign in .auto/prompt.md within its 10-run/60-minute budget. Establish and log the unchanged baseline before implementation edits. Use only the installed experiment tools.

The extension's init_experiment/run_experiment/log_experiment tools are required.
If unavailable, stop and explain; do not create another loop controller or manually
commit/revert candidate experiments. Research branches must be clean when starting.
Do not reset the log, run segment, or deadline to obtain more budget. Measurements,
builds and checks must stay within budget. No push or merge is authorized.
