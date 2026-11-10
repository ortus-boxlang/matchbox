# Shared native profile: coordinator handoff

## Status: collected and released for analysis

**Current attribution:** the later [executed native-path ledger](09-native-ledger.md) provides much stronger per-path instruction accounting. Some sampled helper shares differ materially from traced logical counts (fiber indexing 6.4% sampled versus 13.5% traced; cache_get 9.0% versus 3.2%). Preserve these profiles as observations, not exact component cost bills. No specific sampling-bias mechanism is asserted without further evidence.

The operator installed **perf 7.3.0**. User-space instructions/cycles tests passed without privilege/security changes. Region-scoped profiles now exist for the ORIGINAL retained MatchBox binary and Node --jitless, with exact checksum validation.

Raw/profile report directory:
`benchmarks/instructions/results/interpreter-loop-profile-20261030T184742Z/`

Start with its `README.md` and `sample-summary.json`. Full commands, manifests, raw perf.data, mappings, reports, failures and native disassembly are preserved there. No optimization implementation edits or new autoresearch runs.

## Artifact identity and matching symbols

- Reference source: 139be94; original executable SHA-256 still 5787b8a92026d52d97ce97d18cf552d06fbfc8d324493bcd661b94aa8e220557.
- Detached diagnostic worktree: `/home/jacob/dev/ortus-boxlang/matchbox-interpreter-loop-diagnostic` at 139be94, clean. Separate copied build/dependency caches, no hardlinks to reference artifacts.
- Offline pinned builder, same features/optimization/LTO/codegen settings; final `cargo rustc ... -- -C strip=none` preserves symbols. No DWARF/frame-pointer changes, Cargo.toml or product source edits.
- ELF comparison confirms every allocated section's address/size/bytes matches reference except the build-ID note. .text SHA-256 on both: 4deedb779fc2c4e3d3f73d7f2e7909fe28ce06c13519b4b0977bb53c4e9cbf98.
- A separate symbol companion copy has only its build-ID note patched to the reference ID for explicit --symfs resolution; full provenance and before/after comparisons recorded. It is NOT the executed binary. After that note patch all allocated sections match reference. No original/diagnostic executable or global build-ID cache was replaced.

## Boundary and accepted runs

Existing million-iteration marked sources, no warmup. Controller waits before START, attaches perf disabled, enables with acknowledgment, resumes, waits before END, disables with acknowledgment, then resumes/validates. Includes START write and END dispatch; excludes startup/source compilation/checksum output. All runs sequential, native host, per-runtime timeout30s, address-space limit8GiB.

| Accepted runs | Event/period | Samples | Lost/throttled |
|---|---|---:|---|
| mb-instructions-4/5/6 | instructions:u / 1,000,000 | 6,513 combined | zero |
| mb-cycles-2 | cycles:u / 250,000, flat | 2,905 | zero |
| node-instructions-3/4/5 | instructions:u / 500,000 | 1,520 combined | zero |

Node uses diagnostic `--perf-basic-prof` in addition to --jitless; original count/timing baseline did not. Captured maps/logs retained. Do not claim unchanged Node flags. About 0.9% Node samples are unresolved/skid artifacts and remain unassigned.

No precise_ip requested: ordinary sampling can skid and alias. Percentages are approximate self instruction-sample distributions, not exact handler counts; cycle samples are separate. Do not derive speedups from profiles or add inclusive and self shares.

## Preserved failures/limits

- Large perf mmap budget initially rejected; ring size reduced, no sysctl changes.
- Initial control ack parser did not accept perf's NUL terminator; fixed transport before successful region runs.
- Too-small sampling periods caused throttling; those runs excluded. Lower sampling rate yielded unthrottled accepted runs.
- DWARF stack-dump attempt mb-cycles-1 produced unreliable unwinds with nonsensical frames. Do not use its caller graph for attribution. Flat self-PC evidence is valid, but primary cycle cross-check is subsequent flat mb-cycles-2.
- Inline/source attribution unavailable without DWARF. Function symbols and code addresses are reliable via matching companion; source-to-native range mapping remains inspected inference.
- Rust/linker identical-code folding can give shared helpers misleading generic type names (e.g. regex cache guard). Do not interpret that as regex execution; follow actual native call sites.

## First findings for reviewers

Combined MB instruction-sample SELF shares:
- run_fiber_inner: 51.53%; of that, a native shared dispatch/header interval is 19.65% of ALL samples (subset, not additive).
- Chunk::cache_get 9.03%; Vec<BxFiber>::index 6.40%; read_constant 5.05%.
- operand-stack Vec pop/push 4.38%/2.53%; numeric_value 3.61%.
- slot_binding 2.99%; set_local_value Option::or_else 2.87%; SlotBindings::get 1.04%.
- RefCell borrow shared helper 2.79%; Mutex lock 2.61%; MutexGuard drop shared helper 2.47%, plus poisoning helpers.

Native cache_get assembly retains mutex acquisition and an **80-byte entry copy on hit**. Actual machine evidence now supports these operations; their isolated costs are not exactly attributed. See area-specific handoffs and budget.

## Next coordination step

Three analysis handoffs can now use the SAME shared data; no separate profiling environments needed. Initial source+native findings were processed sequentially in this session (no subagent tool). Parallel read-only reviewers may inspect assigned areas, but coordinator alone schedules any additional probes.

Follow-up complete: conservative direct-branch reachability partitions the previous ~31.88% residual into about 30.79% candidate-specific handler bodies and 1.09% explicitly shared tails. See `handler-map.json` / `map-handler-samples.py` in the raw evidence directory and the [cache ownership/design review](07-cache-design-review.md). This reuses existing samples, not a new measurement. Remaining uncertainties include detailed shared-helper ownership and the complete semantic instruction bill. The next decision is review of a bounded projected-cache-read candidate, not an automatic redesign or implementation run.

No concurrent profiling/counting/timing, benchmark edits, feature removal, profile tuning or implementation experiments. The previous campaign remains exhausted/frozen.
