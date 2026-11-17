---
name: matchbox-autoresearch
description: Prepare and operate bounded MatchBox optimization campaigns with the project-local pi-autoresearch extension. Load before setting up, starting, or resuming autoresearch experiments in this repository.
---

# MatchBox Autoresearch

Use the installed `pi-autoresearch` extension for execution, logging, and rollback;
do not build another loop controller. Read `../../README.md` for operator commands.
Resolve the campaign paths below from the repository/worktree root, not this skill.

## Authorization and isolation

- Installation/configuration is not authorization to run experiments. If no goal
  has been agreed, ask for one; do not invent an objective or start a loop.
- Preparing a campaign means defining its evaluator and playbook, not optimizing
  implementation code. Show the prepared contract and wait for explicit approval
  before initializing or starting the loop. The upstream create skill's instruction
  to start immediately does not authorize skipping this approval.
- Require a clean, dedicated Git worktree on an `autoresearch/<campaign>` branch.
  Check `git status --short`, `git branch --show-current`, and `git worktree list`.
  Never experiment on `develop`, `master`, or the setup feature branch, or share a
  worktree with normal development. Stop if unrelated changes appear.
- Commit the reviewed campaign scaffold before the first experiment. The extension
  stages all changes on keep and discards tracked/untracked changes on rejection;
  it does not isolate or protect unrelated work.

## Prepare the campaign contract

Agree with the user on:

1. One concrete goal and primary metric, including unit and direction.
2. Fixed workloads, expected outputs, target, Cargo features, build profile, and
   relevant environment/toolchain settings. Separate build time from runtime unless
   build time is itself the objective. Record the machine used for measurements.
3. An explicit implementation-file allowlist and read-only evaluation inputs.
4. Correctness checks, resource limits, and permitted secondary-metric regressions.
5. Repetitions/warmup, a noise-aware acceptance rule, per-command timeouts, and a
   session budget. `.auto/config.json` defaults to ten logged runs including the
   baseline. Agree API-spend limits separately; an iteration cap is not a spend cap.

Write and review these files before enabling the loop:

- `.auto/prompt.md`: the full contract, baseline procedure, commands, editable
  allowlist, constraints, and learned results. Include the rules in this skill so
  they survive the extension's compaction/resume workflow.
- `.auto/measure.sh`: fail-fast benchmark (`set -euo pipefail`) that validates all
  expected workloads ran successfully and emits `METRIC name=value` lines. For
  short/noisy workloads, repeat runs and report a median; retain raw samples under
  `.auto/artifacts/`. Missing binaries, outputs, or metrics must fail, not skip.
- `.auto/checks.sh`: mandatory correctness/regression gate for MatchBox campaigns,
  also fail-fast. Include file-scope/evaluator-integrity checks and enforce agreed
  resource limits here, not merely as advisory dashboard metrics.

Dry-run the evaluator and checks without editing implementation code. Establish
baseline test outcomes by test identity, not just totals: the compatibility suite
may contain existing failures or feature/environment-dependent ignores. Do not
require an unverified all-green baseline, silently skip failures, or accept a net
pass-count increase that breaks previously passing tests.

## MatchBox invariants

- Preserve BoxLang behavior. Consult the JVM reference for semantic changes; never
  modify `reference/`. Follow the project's BIF/reference workflow when applicable.
- Keep tests, fixtures, expected outputs, benchmark scripts, build settings,
  dependencies, `.pi/`, and `.auto/config.json` outside the editable allowlist.
  Freeze evaluator files after approval. Evaluation changes require stopping,
  review, and a new baseline, not an in-flight scoring change.
- Do not trade away required features or targets for a better score. Release builds
  currently use `opt-level = "z"`; changing that is not a runtime optimization.
- JIT work must prove native execution and preserve deopt/GC behavior. Native JIT
  measurements say nothing by themselves about WASM or ESP32 performance.
- Correctness and agreed memory/size/latency constraints override primary-metric
  gains. Recheck promising changes; the extension's confidence score is advisory,
  not a substitute for baseline/candidate measurements and validation workloads.
- Prefer simple, general improvements. No benchmark-specific behavior, weakened
  assertions, disabled checks, or speculative architecture for marginal gains.

## Run and finish

After approval, activate via `/autoresearch <approved goal>`. Use the extension's
`init_experiment`, `run_experiment`, and `log_experiment` tools. Run the frozen
`bash .auto/measure.sh` command with explicit benchmark/check timeouts. Establish
the unchanged baseline first; do not modify implementation code before recording
it. Do not manually commit/revert individual experiments: `log_experiment` owns
those operations.

Record every result and its hypothesis, evidence, failure reason, and next idea.
Persist learning in `.auto/prompt.md` or `.auto/ideas.md`; do not edit frozen
contract sections. Never keep a failed benchmark or failed correctness check.

Stop at the configured run limit, user interruption, or an unsafe state. Do not
reinitialize a segment, raise limits, or restart the loop to evade its budget.
Upstream "never stop" guidance applies only inside the approved budget. Do not
push, publish, merge, or access production services/credentials. Finish with the
baseline/best measurements, validation results, tradeoffs, rejected ideas, and
reviewable commits. Archive the ignored log/artifacts before clearing a session.

These rules are workflow instructions, not a security sandbox. Use OS/container
isolation and narrowly scoped credentials for unattended or untrusted execution.
