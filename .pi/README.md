# Project-local Pi workflows

MatchBox uses [pi-autoresearch](https://github.com/davebcn87/pi-autoresearch) for
bounded, measurable optimization campaigns. The package is pinned to **1.9.0** in
`settings.json`; no global Pi package installation is needed. This setup was
smoke-tested with Pi 1.4.0 and Node.js 22.

This is campaign infrastructure, not an active experiment. The standalone runtime
suite provides a first verified workload, but no optimization objective or campaign
has been approved. Installing the package, running a benchmark, or loading this
project must not be treated as permission to start optimizing code.

## Install or reload

From the repository root, with Pi installed and Node.js 22 or newer:

```bash
pi install --local --approve npm:pi-autoresearch@1.9.0
pi list
pi
```

Alternatively, Pi installs a missing declared package when the project is trusted
at startup. Review the package declaration/source before granting trust. In an
existing interactive session, run `/reload` to discover the package, local
skills, and prompt templates. Keep provider/model selection and credentials in your personal Pi setup.

`/autoresearch help` displays help without starting a campaign. Do not use a
free-text `/autoresearch <goal>` command just to inspect setup: it activates the
loop and can start work immediately.

## Standalone benchmark commands

These project-local commands delegate to reusable skills; they do not activate
autoresearch:

| Command | Workflow |
| --- | --- |
| `/benchmark-run-suite` | Run verified workloads on MatchBox, Node.js and JVM BoxLang; save raw samples and report timing boundaries |
| `/benchmark-add-test` | Add a licensed, pinned upstream workload and faithful BoxLang port, verified on all three engines |
| `/measure-instructions` | Count verified hardware instructions for a script, whole-process or scoped after warmup; supports Node `--jitless` |

Append the benchmark name or a natural-language request, for example
`/benchmark-run-suite sieve, all three runtimes, three samples` or
`/benchmark-add-test Bounce from the pinned AWFY revision`.
The skills are also available as `/skill:benchmark-run-suite`,
`/skill:benchmark-add-test`, and `/skill:measure-instructions`; `.pi/prompts/`
supplies the shorter command aliases. For instruction counts, see
[the standalone counter guide](../benchmarks/instructions/README.md).

See [the standalone suite guide](../benchmarks/runtime/README.md). The evaluator
is ordinary Python/JavaScript/BoxLang and remains usable without Pi. Do not confuse
benchmark creation/runs with permission to change implementation code or start an
optimization loop.

## Prepare an isolated campaign

The extension runs `git add -A` and commits on keep. On rejection it restores
tracked files and cleans untracked files outside its preserved session files.
**A branch alone is not isolation: do not run it alongside normal development.**

First commit the reviewed setup changes so a new worktree contains them. Then
create a dedicated worktree from that commit, choosing a unique campaign name:

```bash
git worktree add -b autoresearch/compiler-process ../matchbox-autoresearch HEAD
cd ../matchbox-autoresearch
pi install --local --approve npm:pi-autoresearch@1.9.0
pi
```

`compiler-process` is an example campaign name, not a preselected objective.
In Pi, prepare rather than launch the campaign:

```text
/skill:matchbox-autoresearch Prepare a compiler/process performance campaign. Do not start the loop; present its objective, editable scope, evaluator, checks, and budget for review.
```

The local skill defines MatchBox's authorization, evaluation, and regression
rules. It requires a reviewed `.auto/prompt.md`, `.auto/measure.sh`, and
`.auto/checks.sh` before starting. There are deliberately no placeholder scripts
that emit an invented metric or pass without checking correctness.

Possible starting points:

| Goal | Existing material |
| --- | --- |
| Compiler-dominant process latency | `benchmarks/compiler/README.md`, `benchmarks/compiler/run.sh` |
| Runtime latency | `benchmarks/runtime/README.md` (verified AWFY Sieve on three engines), `benchmarks/prelude_perf.bxs`, QoQ workloads under `benchmarks/` |
| Allocation/memory reduction | `crates/matchbox-vm/tests/canonical_keys.rs`, `tests/wide_struct.py` |
| JVM compatibility | `tests/boxlang_compat_tests.rs`, `tests/scripts/boxlang_compat/README.md` |
| Native JIT work | `docs/building-and-deploying/jit.md`, `tests/jit_runner.py` |

These existing checks are starting points, not an already calibrated unattended
evaluator. Establish actual baseline outcomes and benchmark variability first.

## Start, inspect, and stop

After approving and committing the campaign contract, activate it in the
**dedicated worktree**:

```text
/autoresearch Run the approved campaign in .auto/prompt.md within its configured budget.
```

| Command | Effect |
| --- | --- |
| `/autoresearch help` | Display commands without activating the loop |
| `/autoresearch dashboard` | Inspect the terminal dashboard |
| `/autoresearch export` | Open the live local browser dashboard |
| `/autoresearch off` | Stop autoresearch mode and abort a running experiment |
| `/autoresearch finalize` | Stop and prepare reviewable branches; review grouping before creating them |
| `/autoresearch clear` | Delete the session log; archive evidence first |

`.auto/config.json` defaults to **10 logged runs per segment, including the
baseline**. The extension enforces the limit; agents must not reinitialize a
segment to bypass it. This does not bound API spend, total wall time, or every
setup action: agree timeouts and set provider-side spend limits separately.

The extension can reactivate autoresearch mode when a new session finds an
existing `.auto/log.jsonl` in its working directory. Keep experiment logs in the
dedicated worktree, and use `/autoresearch off` before normal conversation there.
Do not create an empty session log as part of configuration.

## Tracked setup versus local state

| Path | Version control |
| --- | --- |
| `.pi/settings.json`, this guide, local skills and prompt templates | Tracked project setup |
| `.pi/npm/`, `.pi/git/` | Ignored package installations |
| `.auto/config.json` | Tracked bounded default; no machine-specific `workingDir` |
| `.auto/prompt.md`, `measure.sh`, `checks.sh`, optional hooks | Review and commit when preparing a campaign |
| `.auto/log.jsonl`, `ideas.md`, `artifacts/` | Ignored run state; archive before cleanup |
| `benchmarks/runtime/results/`, `benchmarks/instructions/results/` | Ignored standalone measurement reports |

The standalone runner saves timestamped raw reports in `benchmarks/runtime/results/`
by default. Named pilot/campaign reports can also go in `.auto/artifacts/`.
Review executable hooks before adding any. Do not store credentials in campaign
files. Pi extensions and experiment commands run with your user permissions;
worktrees and agent instructions are not a sandbox. Use a restricted environment
for unattended runs when appropriate. Kept patches still require human review
before merging.
