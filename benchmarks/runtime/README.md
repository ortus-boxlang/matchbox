# Cross-runtime performance benchmarks

A standalone, Python-standard-library runner for equivalent small programs on
**MatchBox, Node.js, and JVM BoxLang**. This suite does not require Pi and never
starts an autoresearch loop or edits runtime implementation code.

## First workload: AWFY Sieve

The initial case is Sieve from [Are We Fast Yet?](https://github.com/smarr/are-we-fast-yet),
pinned to revision `74306fec151070fd07157cefeacf19e7e0bcdc89`.

Each invocation allocates a fresh 5,000-element Boolean array, initializes it to
true, marks multiples starting at `i + i`, and verifies **669 primes**. It retains
upstream's algorithm, class/method structure, and result check. No closed-form
shortcut, optimized replacement sieve, native prime library, or changed problem
size is used. `arraySet(flags, 1, 5000, true)` expresses JavaScript's fixed-bound
array allocation/fill using BoxLang's one-based arrays.

- `awfy/javascript/sieve.js` and `benchmark.js` are unmodified upstream files.
  Their SHA-256 hashes are checked against `manifest.json` before execution.
  `awfy/AUTORS.md` preserves upstream's SOM attribution (including its filename).
- `awfy/boxlang/` contains the attributed BoxLang port. **Exactly the same files**
  execute on MatchBox and JVM BoxLang; no engine-detection branches.
- The selected files use the MIT notice preserved in their headers and
  `awfy/LICENSES/SOM-MIT.txt`. Other AWFY benchmarks have different provenance;
  review their individual licenses before adding them.
- Local `harness.js` and `harness.bxs` perform equivalent warmup and measured
  batches, checking each actual result. The report also validates the result,
  benchmark name, and iteration counts independently.

Sieve is a useful first array/loop workload, not a complete runtime score. Add
object-, string-, recursion-, and allocation-heavy cases before generalizing.

## Run

Requirements: Python 3.10+, a native MatchBox executable, Node.js, and JVM BoxLang.
Build MatchBox outside the measured run, preserving the release profile/features:

```bash
cargo build --release --locked
python3 benchmarks/runtime/run.py --list
python3 benchmarks/runtime/run.py --benchmarks sieve
```

Defaults: all three runtimes, three fresh-process samples per engine, 100 warmup
invocations and 1,000 measured invocations per sample, 120 seconds per process.
Sample order rotates across engines. Default reports go to ignored
`benchmarks/runtime/results/<timestamp>.json`.

Override executable locations and retain a named report:

```bash
python3 benchmarks/runtime/run.py --benchmarks sieve \
  --matchbox-bin /path/to/matchbox \
  --node-bin /path/to/node \
  --boxlang-bin /path/to/boxlang \
  --samples 3 --warmup 100 --iterations 1000 --timeout 120 \
  --output .auto/artifacts/sieve-baseline.json \
  --note 'Record build revision, toolchain, features, flags, and other run context'
```

Executable defaults are `target/release/matchbox`, `node`, and `boxlang`;
`MATCHBOX_BIN`, `NODE_BIN`, and `BOXLANG_BIN` override them. Values are executable
paths, not shell command strings. A wrapper executable can supply runtime flags;
record those flags in `--note`.

Use `--runtimes matchbox` or `--runtimes node boxlang` for an explicitly partial
run. Missing requested runtimes, process errors/timeouts, missing/duplicate
records, invalid timings, changed inputs, and incorrect answers **fail the run**,
not skip or report success. Failed reports retain diagnostics and available raw
samples; only `status: complete` reports qualify as successful measurements.

The runner controls the working directory and passes absolute entry paths.
To inspect a workload manually, use that same working directory:

```bash
cd benchmarks/runtime
node harness.js "$PWD/awfy/javascript/sieve.js" sieve 100 1000
boxlang "$PWD/awfy/boxlang/sieve.bxs" --warmup=100 --iterations=1000
../../target/release/matchbox "$PWD/awfy/boxlang/sieve.bxs" --warmup=100 --iterations=1000
```

## Read the measurements correctly

Two separate measurements are reported:

- **Batch elapsed time**: inside the runtime, after warmup. Includes workload
  allocation, execution, normal GC, and per-invocation result validation. Median,
  median absolute deviation (MAD), and median milliseconds per invocation are
  reported. Warmup is a fixed policy, not proof that every JIT has stabilized.
- **Whole-process elapsed time**: Python's external monotonic timer, covering
  process launch, source processing/loading, runtime initialization, warmup,
  measured work, validation, output and shutdown. This is fresh-process latency,
  **not** isolated compiler timing or guaranteed cold-cache latency. JVM BoxLang
  may reuse compiled template caches. Do not subtract the two to claim compile time.

Both workload harnesses currently use millisecond-resolution wall-clock timers.
The runner rejects zero/negative/nonfinite measurements and warns when any
measured batch is below 100ms. Increase `--iterations` (and, if necessary,
`--timeout`) before judging small differences. Never silently use different
problem sizes or repetition counts for the engines in a comparison. Larger
batches can be expensive on the slower engine; agree the budget first.

JSON reports include versions, executable paths/hashes, host information, Git
revision/dirty state, workload settings, upstream revision, SHA-256 hashes of
suite inputs, commands, stdout/stderr, and raw measurements. Build features,
compiler flags, Java/Node options, toolchains, and machine load must be recorded
in the note/experiment record; they cannot be inferred fully from an executable.
Peak RSS and GC pause statistics are **not** measured by this initial runner.

Run on an otherwise idle machine when possible. These local pilot measurements
are not a claim about all applications or other machines. Keep this runtime
suite separate from `../compiler/` when evaluating frontend compilation speed.

## Validate and extend

Runner tests require no installed runtimes:

```bash
python3 -m unittest discover -s benchmarks/runtime -p 'test_*.py' -v
```

Then execute the real three-engine suite; mocked unit tests do not establish
language compatibility or performance.

Project-local Pi commands are `/benchmark-run-suite` and `/benchmark-add-test`.
Their instruction sources are `.pi/skills/benchmark-run-suite/SKILL.md` and
`.pi/skills/benchmark-add-test/SKILL.md`, also available as `/skill:<name>`.
Run `/reload` after adding them. Prompt templates provide the shorter aliases;
the substantive instructions live in the skills.

### Portability lessons from the first test

- Use BoxLang's canonical `jsonSerialize()`, not CFML's `serializeJSON()` alias,
  which is absent in a plain JVM BoxLang installation without compatibility modules.
- Use `cliGetArgs().options` with `--name=value`. JVM versions differ on whether
  positional arguments include the script path; MatchBox excludes it.
- Use explicit package imports and a stable suite working directory for `.bx`
  classes and parent classes; do not rely on implicit sibling-class discovery.
- Translate JavaScript `flags[i - 1]` to BoxLang `flags[i]`; audit every index.
- Do not request nanoseconds from `getTickCount()` on MatchBox: the current BIF
  ignores the requested unit and still returns milliseconds. The portable harness
  uses the default unit and fails/warns on insufficient timing resolution.
- Build once before timing. No runtime implementation changes were needed to run
  Sieve; fix port/protocol mistakes rather than making the VM benchmark-specific.

Before autoresearch, review/freeze these inputs, define implementation scope and
regression gates, and prepare an approved campaign in a clean dedicated worktree
following `.pi/skills/matchbox-autoresearch/SKILL.md`. A benchmark run by itself
is not permission to optimize, mutate the evaluator, or start a loop.
