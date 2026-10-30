# Evidence index and reproduction

## Existing raw study (ignored, machine-local)

Repository-relative directory:
`benchmarks/instructions/results/for-loop-20261030T180724Z/`

| File(s) | Contents |
|---|---|
| `README.md` | Full methodology, provenance, results and caveats |
| `study.json`, `derived-summary.json` | Raw timing samples and derived count slopes/ratios |
| `count-{matchbox,node}-{0,100000,1000000}.json` | Hardware count reports, commands, inputs, executable hashes, per-thread evidence, exact output validation |
| `count-*.bxs`, `count-*.js` | Marked loop inputs actually counted |
| `time-*.bxs`, `time-*.js`, `time-*.stdout`, `time-*.stderr` | Separate uninstrumented timing inputs/output |
| `run-study.py` | One-shot study orchestration, NOT an experiment-loop controller |
| `trace-loop.bxs`, `trace-loop.trace`, `trace-loop.stdout` | Unmeasured two-iteration MatchBox execution (result 5) |
| `trace-loop.js`, `node-bytecode.txt`, `node-bytecode.stderr` | Unmeasured Node bytecode diagnostic; script's 48-byte wrapper near lines 129-165 |
| `ForLoop.tla`, `ForLoop.cfg`, `view.mjs`, `statechart/` | Bounded descriptive model and generated chart/report |
| `counter-tests.log` | 18 tests discovered; success, five opt-in live tests skipped; actual counter runs separately succeeded |
| `investigation/tool-preflight.log` | Host/cached-builder tool inventory, retained binary sections/hash |

Raw reports are NOT tracked documentation and may be absent in a fresh checkout. Do not claim a profile exists when only these counter totals exist. Portable workloads and methodology live alongside this packet; the earlier count reports remain the measurement authority.

A local evidence archive was created without clearing originals:
`benchmarks/instructions/results/for-loop-20261030T180724Z-handoff-evidence.tar.gz`
SHA-256: `b9df9f0232e62364f53a26a9a3caff449ce25be04afe87dda358de578948eab6`.
The archive is ignored and not automatically available to another checkout. Transfer it explicitly if needed; do not claim the documentation alone contains raw evidence.

## Shared profiles (subsequent investigation)

`benchmarks/instructions/results/interpreter-loop-profile-20261030T184742Z/` contains the new region-scoped perf profiles, original-binary/matching-symbol proof, native disassembly, sample aggregation and preserved failed diagnostics. Start with its README.md and sample-summary.json; see [profile handoff](02-shared-profile.md). Accepted runs have zero lost/throttled records. Profiling is not uninstrumented timing, and approximate instruction-sample distributions do not replace the exact counter totals below.

The new profile evidence was archived locally as `benchmarks/instructions/results/interpreter-loop-profile-20261030T184742Z-evidence.tar.gz`, SHA-256 `f9e4732f8a9c323e7e5423ba2fa9b736e2ffa9192b3b58a24e7f3bf464685d81`. Originals retained; archive is ignored and must be transferred explicitly for another checkout.

Static follow-up (no new runtime measurements): `handler-map.json`, `map-handler-samples.py`, `phase2-native-excerpts.txt` and `phase2-static-manifest.json` in the same profile directory. Updated evidence archive: `benchmarks/instructions/results/interpreter-loop-profile-20261030T184742Z-phase2-evidence.tar.gz`, SHA-256 `65d013e993528a74122aab052f5fa57270fd02f657b1189f517cfbf7674efc63`. Original archive retained. See [ownership/design review](07-cache-design-review.md).

The operator installed perf7.3.0. A separate detached symbol-build worktree exists at `/home/jacob/dev/ortus-boxlang/matchbox-interpreter-loop-diagnostic`, source139be94. No product source edits, optimization-profile changes or JIT activation. Original reference executable hash remains unchanged. Node profiler-only --perf-basic-prof flag is recorded, not silently equated to baseline flags.

## Subsequent bounded implementation experiment

The explicitly approved global-cache projection candidate and validation are in `benchmarks/instructions/results/global-cache-projection-20261030T193649Z/`. It passed the tested semantic suites and removed the wide cache-entry copy, but regressed the unchanged region's retired instructions by **0.9664%**. Exact patch reversed; no product change retained. See [full result](08-cache-projection-result.md). No tuning variants or campaign restart.

Evidence archive: `benchmarks/instructions/results/global-cache-projection-20261030T193649Z-evidence.tar.gz`, SHA-256 `4f9f3020309062cf523dbca0f6c2f1fb281df8cf2f944c763a65d5a7c4be9bb5`.

## Executed native-path ledger

Six bounded sequential debugger sessions traced selected continuing cycles on the unchanged reference, Node --jitless and the already rejected candidate. All resumed to exact checksum and zero exit. See [current native ledger](09-native-ledger.md) for method, REP accounting, corresponding functional work, model residuals and retrospective +21/cycle candidate reconciliation. No new product change, build, optimization variant, PMU batch or timing study.

Raw packet: `benchmarks/instructions/results/native-ledger-20261030T201844Z/`. Archive: `benchmarks/instructions/results/native-ledger-20261030T201844Z-evidence.tar.gz`, SHA-256 `99663489051e57b7e81c60e1d00702fa347150eb253b2d3b70c190a400795efe`. Includes plans, tracer snapshots, configs, command/provenance manifest, raw PC/operand traces, checksums and recomputable ledgers. Originals retained; ignored archive requires explicit transfer.

## Subsequent read-only candidate selection

[10-candidate-selection.md](10-candidate-selection.md) compares four implementation scopes plus no change, recommends an explicit unbound-local frame state, and records conditional pre-implementation estimate P1. [compare-scopes.py](compare-scopes.py) recomputes three disjoint local-access sequences (553 instructions total) and replacement-model inputs from the existing four reference traces. This is static reanalysis, not another debugger/counter batch. No original raw evidence/archive was modified and no candidate build or measured improvement exists.

## Approved Unbound-local implementation and comparison

The selected candidate was subsequently approved, implemented and retained after tests/native inspection and one fresh reference/candidate counter comparison. [11-unbound-local-result.md](11-unbound-local-result.md) records **16.29% fewer instructions**, pre-measurement P1/P2 reconciliation, and separate exploratory timing (refreshed median142 ms versus candidate121 ms, with initial baseline153 ms and drift disclosed).

Raw packet: `benchmarks/instructions/results/unbound-locals-20261030T210916Z/`. It includes authorization/budget, preserved predictions, exact patch/executables/hashes, initial compile failure, passing test/build logs, static native-model inputs and helper checks, scoped count reports, fresh timing samples and `result.json`. No new debugger/profile sessions or variants; original reference unchanged.

Archive: `benchmarks/instructions/results/unbound-locals-20261030T210916Z-evidence.tar.gz`, SHA-256 `85e4966230270fc621c5a5327d391b1e9f14e1bd61c827b8aa5e6b51949ab7a1`. Originals retained; ignored archive must be transferred explicitly.

## Measured values

| Runtime | n | Median retired instructions | MAD | Median loop ms | MAD ms |
|---|---:|---:|---:|---:|---:|
| MatchBox | 0 | 5025 | 0 | below resolution | - |
| MatchBox | 100000 | 217305958 | 51 | 16 | 0 |
| MatchBox | 1000000 | 2172993829 | 125 | 142 | 0 |
| Node --jitless | 0 | 67062 | 182 | below resolution | - |
| Node --jitless | 100000 | 23668692 | 181 | 2 | 0 |
| Node --jitless | 1000000 | 254269661 | 2358 | 14 | 1 |

Large-minus-small marginal instructions/iteration: MatchBox 2172.986523; Node 256.223299. Do not confuse those slopes with million-batch normalization (2172.993829 and 254.269661). Node has a systematic size-dependent change larger than its sample MAD. Cause unknown.

Counts: scoped all-thread retired user instructions, kernel/hypervisor excluded, includes marker boundary work. Every sample complete and non-multiplexed with matching enabled/running time. Timings: different fresh uninstrumented processes, same body, millisecond clocks around loop; exclude initialization/output. No warmup for either. Three samples are not confidence intervals. Counter controller's whole-process elapsed times are not isolated workload timing.

## Key scope mismatch to keep visible

Node executes these .js files under its CommonJS function wrapper: sum and i use local registers. MatchBox's top-level sum is a VM-global slot; the for-loop index is a compiler stack local. This is faithful normal script execution for each harness, not identical storage contracts. Do not silently wrap the measured MatchBox loop in a function to erase this difference. A storage-matched follow-up would be a separately labeled experiment.

## Read-only reproduction

From repo root, after checking binary provenance:

```sh
sha256sum target/build/release/examples/instruction_interpreter
node --version
timeout 30s target/build/release/examples/instruction_interpreter docs/performance/interpreter-loop/workloads/loop.bxs
timeout 30s node --jitless docs/performance/interpreter-loop/workloads/loop.js
```

Each final line must equal 500001500000. These commands verify output, not speed or hardware instructions.

To reproduce hardware counts, read `.pi/skills/measure-instructions/SKILL.md` and `benchmarks/instructions/README.md`. Use unchanged `benchmarks/instructions/run.py`, original marked sources and a NEW output filename; samples=3, timeout=30, exact stdout validation. Hash all inputs, identify executable provenance, and keep runs sequential. Do not overwrite previous JSON, replace hardware counters with estimates or use BX_TRACE in a measured process.

`run-study.py` writes named files; it must be copied into a new sibling result directory before rerunning. Never run `.auto/measure.sh` or `.auto/checks.sh` to obtain new study data: their campaign is complete and budget exhausted.

The existing marker helper is `.auto/artifacts/markers.so`, SHA-256 `659262daf29f597ce11fe22121888b2e56f21344c260b858c9dee2ec864dcc8a`. If unavailable, rebuild the documented helper separately, record its provenance and do not pretend the artifact is unchanged.
