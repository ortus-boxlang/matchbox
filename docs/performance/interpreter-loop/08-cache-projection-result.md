# Bounded global-cache projection experiment: rejected

Status: **implemented, tested, measured once as a candidate, then reverted.** The user explicitly approved this bounded implementation/validation after the read-only design review. No autoresearch campaign was resumed, no tuning variants were tried, and no product change is retained.

## Candidate

The patch added `cached_global_index(site, namespace) -> Option<usize>` to Chunk/ChunkRuntime and moved all five global-cache consumers to it. The cache entry was inspected by reference under the existing mutex, or in the existing route-owned vector. Only the namespace-validated index escaped. General cache-entry retrieval remained for shape/JIT consumers. Synchronization, namespace guards, source-program isolation, function sharing and miss/refill behavior were preserved.

The candidate factored the existing dense/sparse lookup into a borrowed-entry helper, so full-entry callers still cloned explicitly. No callbacks, borrowed references escaping the lock, bytecode changes, benchmark recognition, build-profile tuning or JIT activation.

## Validation

New tests covered both cache stores, absent/out-of-range/empty slots, all entry variants, matching/mismatched namespaces, zero/max IDs, cache-content preservation, function/recursive cache sharing, program identity changes and fresh runtime isolation.

| Suite | Passed | Ignored |
|---|---:|---:|
| VM unit (includes 3 new tests) | 62 | 0 |
| VM integration/canonical keys | 17 | 0 |
| Root library unit | 27 | 0 |
| BoxLang compatibility | 543 | 29 |
| Host lifecycle | 5 | 0 |
| Integration | 195 | 0 |
| Route-cache ownership | 4 | 0 |

Early broad runs failed because the offline builder lacked Java. Their logs are retained. Exposing the host's existing OpenJDK 25 read-only at `/opt/jdk` and setting JAVA_HOME allowed full reruns to pass without changing tests. No packages were installed. The ESP32 sparse lookup was updated/reviewed, but no ESP32 cross-build or device validation was available.

## Native-code check

Candidate global accessor: `0x42374e..0x42380b`; projection helper: `0x443cac..0x443ccf`. Native code retains mutex acquisition/release and checks the Global variant/namespace in place. The 80-byte full-entry copy is absent from this accessor path. All five global consumers call the new accessor.

However, the projection helper remains outlined, and the accessor saves/restores its small result across an ordinary mutex-guard-drop call. The original retrieval could tail-jump to guard drop after copying its result. These are observed code-generation differences, not a complete causal allocation of the measured regression. An 80-byte copy payload was never evidence of 80 separately retired machine instructions or 9% removable work.

## Instruction comparison

One candidate, three fresh processes per executable, measured sequentially on the same native AMD Ryzen 7 PRO 6850U host. No warmup. Unchanged million-iteration checksum workload and START/END boundary. Startup/compiler excluded; START write/resumption and END dispatch included. User-space retired instructions across inherited threads, no multiplexing/scaling/fallback. All six runs returned exact checksum **500001500000**, exit zero, and equal positive counter running/enabled times.

| Executable | Median retired instructions | MAD | Batch-normalized / iteration |
|---|---:|---:|---:|
| Retained reference | 2,172,993,764 | 47 | 2172.993764 |
| Candidate | 2,193,993,909 | 176 | 2193.993909 |

Delta: **+21,000,145 instructions per batch, +0.9664%**. This is a regression against the instruction-count objective, much larger than the observed within-batch variation. It does not prove a wall-time slowdown. No separate timing study or new Node comparison was conducted; controller whole-process durations are not loop timings.

Both use source base `139be94`, the real compiler/tree-shaken prelude, default features and the same pinned Rust 1.98.1 builder. Release opt-level=z, LTO, one codegen unit, panic abort unchanged. The separately built candidate keeps symbols (`-C strip=none`) for native inspection, as in the earlier diagnostic build. No JIT is enabled; the harness asserts that before/after execution.

Reference SHA-256: `5787b8a92026d52d97ce97d18cf552d06fbfc8d324493bcd661b94aa8e220557`.

Candidate SHA-256: `889736fd905281f27effc0f947a2bd0c29a0ea1dd6c6798d88add28b70781837`.

Patch SHA-256: `04eb63b3e185096e37b2aa442826e381467dbb75e4b42e6055561a3438688281`.

## Decision and implications

**Reject this implementation for the stated objective.** The exact candidate patch was reversed in both the main checkout and isolated diagnostic worktree; source/tests return to the reference revision. The original reference executable is unchanged. The candidate executable, patch, logs and results are retained separately for review/reproduction.

Subsequent [executed-path reconciliation](09-native-ledger.md) now accounts for the regression: +26 accessor/projection-helper instructions and −5 caller instructions = +21/cycle. Extrapolation differs from the measured million-batch delta by only 145 instructions. This is a retrospective explanation, not a claim that a valid numeric savings prediction existed before this candidate.

This falsifies the expectation that this particular narrower accessor reduces total retired instructions under the retained build. It does not prove that all projected cache interfaces are bad, that copying is universally free, or that synchronization is required inside every future execution representation. No inline-attribute, return-type or helper-layout variants were tried: that would be a new experiment decision, not an automatic continuation.

The broader evidence still points to distributed dispatch/state/container/scope/cache overhead. Before further implementation, review that architectural picture rather than turning this failed candidate into a local tuning loop.

## Evidence and reproduction

Raw directory: `benchmarks/instructions/results/global-cache-projection-20261030T193649Z/` (ignored; transfer explicitly).

Evidence archive: `benchmarks/instructions/results/global-cache-projection-20261030T193649Z-evidence.tar.gz`, SHA-256 `4f9f3020309062cf523dbca0f6c2f1fb281df8cf2f944c763a65d5a7c4be9bb5`. Originals retained. `files-sha256.json` records per-file hashes. The isolated worktree's executable was restored from the saved baseline after the candidate was copied to its explicit artifact path.

- `candidate.patch`: complete implementation and three new tests.
- `builder.sh`, `build.log`: isolated offline builder; dedicated diagnostic worktree and target cache, original reference never overwritten.
- `instruction_interpreter-candidate`, `baseline-unstripped`: separately retained executables.
- `unit-tests.log`, `vm-tests.log`, `regression-tests-with-jdk.log`, earlier failure logs: validation history.
- `counter-tests.log`: instruction evaluator tests.
- `count-reference.json`, `count-candidate.json`: exact native commands, executable/workload/helper hashes, host, raw samples and validated counters.
- `candidate-native.txt`, `native-symbols.json`: static native inspection.
- `result.json`: machine-readable decision and summary.

To reproduce the candidate, apply the patch only in an isolated worktree at the recorded base revision, use the recorded builder/profile and preserve the reference. Reports identify the existing marker helper and workload. Reproduction/another implementation variant requires its own explicit bounded authorization; nothing here restarts autoresearch.
