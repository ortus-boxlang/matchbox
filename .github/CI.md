# CI build caching

Snapshots call `release.yml`, so both channels use the same build and cache
configuration. `snapshot.yml` only controls invocation and cancellation.

## Cache ownership

- **Cargo downloads and non-pilot dependency artifacts:** pinned
  `Swatinem/rust-cache`, scoped by job and matrix target/tag. Its keys incorporate
  the Rust toolchain and Cargo manifests/lockfiles. Default cleanup removes
  workspace and incremental artifacts rather than saving whole `target/` trees.
  Compiler/toolchain changes do not reuse an unrelated workload's cache.
- **Runner stubs:** the cache maps the standalone `crates/matchbox-runner`
  workspace to the root `target/` directory used by CI.
- **Kache pilot:** only Ubuntu tests and the Linux x64 CLI build. Those jobs
  cache Cargo downloads but not `target/`; Kache owns their compiler artifacts.
  Executable caching is enabled, with a pinned Kache action and binary version.
- **ESP32 and Docker:** unchanged. ESP32 remains disabled; Docker packages the
  already-built release binaries rather than recompiling Rust.

Kache's built-in GitHub persistence is disabled because its lockfile-based key
cannot save changes after an exact cache hit. Instead, `actions/cache` persists
its store with keys containing the workload, OS/target and architecture, Rust
toolchain, lockfiles, and source revision. Restore prefixes stay within the same
workload and toolchain, preferring matching lockfiles. New revisions can save
updated stores; reruns of the same revision reuse the existing entry.

Each pilot store is limited to 2 GiB. Revision-specific snapshots still consume
repository cache storage and may evict older entries, so monitor cache size and
transfer time before expanding the pilot. The store is under `runner.temp` to
keep it on the hosted runner's workspace volume. The action warns if hardlinks
are unavailable and artifacts must be copied instead.

## Build correctness

`build.rs` reads Git revision, build date, and `MATCHBOX_BUILD_VERSION`, generates
`src/stubs.rs`, and may launch nested runner builds. The pilot explicitly sets
`KACHE_BUILD_SCRIPT_CACHE=0` so build scripts execute rather than restoring their
side effects. Compiler outputs, including build-script executables, can still
be cached.

The Linux x64 CLI job checks `--version` against the requested release/snapshot
build version and checked-out Git revision, then runs the existing native-runner
BIF smoke test. These checks run on cold and warm builds alike. Changing build
metadata can correctly cause executable-cache misses even when dependencies hit.

Workspace artifacts are not retained by the Cargo dependency cache, so removing
the old whole-target/stub-hash cache does not preserve a stale MatchBox executable
or its root build-script outputs. Full LTO and one release codegen unit remain
unchanged; linking and test execution can still dominate warm builds.

## Baseline and evaluation

The successful snapshot run
[36948978957](https://github.com/ortus-boxlang/matchbox/actions/runs/36948978957)
showed significant Windows cache overhead:

| Job | Main build/test step | Cache restore | Cache save |
| --- | ---: | ---: | ---: |
| Windows tests | 420 s | 87 s | 394 s |
| Windows stubs | 184 s | 84 s | 336 s |
| Linux tests | 322 s | 1 s (miss) | 17 s |
| Linux x64 CLI | 138 s | Cargo/target misses | 6 s combined |

Windows tests restored a roughly 2 GB compressed stub cache through a broad
fallback prefix. All eight CLI targets missed their Cargo and target caches in
that run. These are observations from one run, not a prediction of savings.

Compare GitHub Actions job/step durations and Kache job summaries for:

1. A cold run without a compatible store.
2. A rerun of the same revision after a successful run saved its store.
3. A new revision with a small Rust source change and unchanged lockfiles.

Record setup/restore time, build/test time, cache-save time, total job duration,
and Kache hits/misses. Prioritize reduced Windows cache overhead and total time
on the gated test/stub/CLI path, not just compiler-cache hit rates. Different
target architectures and flags require different compiler outputs.

Do not expand Kache to other platforms until the pilot demonstrates a total
job-time improvement and metadata/native-runner checks pass. To disable the
pilot, remove its Kache setup/store steps and enable `cache-targets` for those
jobs. When updating the Kache binary or caching policy, change the `kache-v1`
cache prefix too so incompatible stores are not restored.
