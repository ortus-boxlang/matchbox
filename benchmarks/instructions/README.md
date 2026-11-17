# Machine-instruction measurements

A standalone Python-standard-library tool for counting **executed user-space CPU
instructions**, not VM bytecodes, generated code size, CPU cycles, or elapsed time.
It works without Pi. The project skill is `/skill:measure-instructions`, with the
short alias `/measure-instructions` after Pi `/reload`.

## Requirements and boundaries

- Native **Linux x86-64**, Python 3.10+, and access to hardware performance counters
  through `perf_event_open`. `perf` itself, root, and a host Rust toolchain are not
  required. Permission/hardware failures fail the run; no simulated fallback.
- Build or explicitly agree on the MatchBox executable before measuring it.
  Record its `--version`, build profile/features, and SHA-256. A checkout revision
  does not establish the build revision of an existing binary.
- Compare engines natively on the same host, sequentially, with equivalent source,
  input sizes, repetition policies, and correctness checks.
- No global sysctl changes, toolchain installation, runtime optimization, automatic
  commits, or autoresearch loop are authorized by this tool or skill.

| Mode | Counted interval |
| --- | --- |
| `process` (default) | From executable launch through exit: dynamic loader, startup, source processing/compilation, workload, output, and shutdown |
| `region` | Between two explicit stdout markers, after any initialization/warmup placed before the first marker |

Both exclude kernel/hypervisor instructions and include inherited threads. Region
mode attaches to every pre-existing thread and inherits new threads. Process mode
uses one inherited counter whose total includes target threads; it does **not**
report that aggregate as a main-thread-only count. Child processes can inherit
counters too: use synchronous foreground scripts that await their children. This
is not a server/daemon profiler or a process-tree accounting tool. On completion,
failure, or timeout the target process group is cleaned up.

Counters must have `time_running == time_enabled > 0`; multiplexed counters are
rejected rather than scaled. Output validation and a zero exit status are both
required. Never interpret a failed report as a measurement.

## Count an unchanged script (whole process)

The example assumes `script.bxs` and `script.js` print `10` as the last nonempty
stdout line. Add a checkable result or use `--expect-stdout-file` for exact output
(an empty expected file is allowed for a script intentionally producing nothing,
but independently verify its result first).

```bash
python3 benchmarks/instructions/run.py \
  --samples 3 --timeout 30 --input script.bxs --expect-last-line 10 \
  --runtime-version 'Record matchbox --version output here' \
  --note 'Record build profile/features and whether prebuilt or current-source' \
  -- target/release/matchbox script.bxs

python3 benchmarks/instructions/run.py \
  --samples 3 --timeout 30 --input script.js --expect-last-line 10 \
  --runtime-version 'Record node --version output here' \
  -- node script.js

python3 benchmarks/instructions/run.py \
  --samples 3 --timeout 30 --input script.js --expect-last-line 10 \
  --runtime-version 'Record node --version output here' \
  -- node --jitless script.js
```

Anything after `--` is the target command and arguments, not a shell string.
Flags are preserved in each report. `--jitless` disables V8's runtime executable
memory allocation, including baseline/optimizing JIT tiers; `--no-opt` is **not**
an interpreter-only substitute. Native builtins and interpreter machinery remain
native. Node may warn that `--expose_wasm` is disabled; these scripts use no Wasm.
Call this an interpreter-only baseline, not Node's universal worst case. MatchBox
settings remain unchanged unless a separate configuration is explicitly requested.

Each invocation saves a unique timestamped JSON report under `results/` (ignored).
Use `--output .auto/artifacts/name.json` for a named report. Existing reports are
never overwritten. `--cwd` sets the target's working directory; target script
arguments resolve there. `--input` paths resolve from the tool's invocation
working directory. Repeat `--input` for every relevant script/module/harness.
Runtime versions are supplied explicitly rather than inferred from Git or guessed
by executing an arbitrary command with `--version`.

## Count execution after warmup (region mode)

### 1. Build the marker helper outside measurement

On a host with a C compiler:

```bash
mkdir -p .auto/artifacts/instruction-helper
cc -shared -fPIC -O2 -Wall -Wextra -Werror \
  -o .auto/artifacts/instruction-helper/markers.so benchmarks/instructions/markers.c
```

If no host compiler is available, an **already available** native Linux builder
can compile the helper. For example, using the project's local Rust image:

```bash
podman run --rm --network none \
  -v "$PWD/benchmarks/instructions:/source:ro,Z" \
  -v "$PWD/.auto/artifacts/instruction-helper:/out:Z" \
  docker.io/library/rust:1.98.1-trixie \
  cc -shared -fPIC -O2 -Wall -Wextra -Werror \
  -o /out/markers.so /source/markers.c
```

Do not silently install/pull a toolchain. Only helper compilation happens in the
container; **both measured engines still run natively on the host**. The helper
must match the host architecture/libc; do not load untrusted shared libraries.

### 2. Add explicit boundaries to a copy of the workload

Use exactly these markers, each on its own line in a separate, synchronous stdout
write: `__ICOUNT_START__` and `__ICOUNT_END__`. Exactly one measured region per
process is supported. For example, BoxLang:

```java
function add(a, b) { return a + b; }
function batch(n) {
    var sum = 0;
    for (var i = 0; i < n; i++) {
        var x = i;
        x += 2;
        var y = add(x, 3);
        sum += y;
    }
    return sum;
}
println("PRIMED");
for (var w = 0; w < 100; w++) {
    batch(1000);
    sleep(1);
}
println("__ICOUNT_START__");
var result = batch(10000);
println("__ICOUNT_END__");
println(result); // 50045000
```

The JavaScript workload is identical. Replace `println` with `console.log`, and
replace `sleep(1)` with
`Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 1)` in its harness.
Those engine-specific idle calls are outside measurement; they allow background
compilers to finish. Keep workload bodies equivalent. The console is primed to
avoid charging its first-use initialization to the region.

Place imports/setup before START and verification/output after END. Normal
allocation, GC and any JIT work that occurs inside the region **are counted**.
Warmup is a policy, not proof of steady state. Check optimization/deoptimization
traces in separate diagnostic processes if steady-state claims matter. Do not
silently force optimization or include tracing in measured runs.

### 3. Measure and validate

```bash
python3 benchmarks/instructions/run.py \
  --mode region --marker-library .auto/artifacts/instruction-helper/markers.so \
  --samples 3 --timeout 30 --input region.bxs --expect-last-line 50045000 \
  --note '100x1000 warmup, 1ms idle/batch; 10000 measured iterations; build provenance' \
  -- target/release/matchbox region.bxs
```

Run equivalent commands for `node region.js` and, when requested,
`node --jitless region.js`. Preserve all reports, including failed ones.

The helper intercepts libc `write`/`writev` using `LD_PRELOAD`, stopping the entire
target process **before** each marker write. The controller enables counters at
the first stop, resumes, then disables counters at the second stop. This excludes
startup and warmup, but includes START's write/resumption and END's dispatch.
Signals perturb execution; this is not a zero-overhead instruction probe.

**Limitations:** dynamically linked libc programs only; no static binaries, direct
syscall-only output, split-token writes, buffered/coalesced markers, async stdout,
concurrent marker emitters, or unrelated SIGSTOP signals. Redirected Node
`console.log` and MatchBox `println` work in the tested environment, but do not
assume all runtimes' output paths do. Missing/extra/out-of-order markers and
nonzero exits fail. Any wait, including startup/warmup, is bounded by `--timeout`.

## Interpret the evidence

Reports contain exact commands, executable/input/helper/runner hashes, supplied
runtime version and build notes, host CPU/OS, checkout revision/dirty state,
relevant inherited runtime settings, raw stdout/stderr, per-thread counters,
raw samples, median/MAD/min/max, and whole-process elapsed time. Reports can
contain workload output and local paths: review them before sharing.

- Report `instructions` as the total across counted threads. Region reports also
  expose `main_thread_instructions`; state which metric you compare.
- For tiny scripts, collect an empty-script or empty-region control with the same
  policy. Startup or marker overhead can exceed the actual workload cost.
- For repeated work, use at least two batch sizes with identical warmup and source
  structure. Approximate marginal instructions/iteration as
  `(median_large - median_small) / (iterations_large - iterations_small)`.
  State both raw batch counts and the normalization. Include loop control,
  checksums, variable access, dispatch and allocation in the boundary description.
- Subtraction is approximate. If noise exceeds the increment or slopes are
  negative/nonlinear, report inconclusive, not a negative operation cost. Check
  a third size or longer warmup only within an agreed budget.
- Preserve observable results so Node cannot trivially eliminate unused work.
  For constant-input tests, label possible constant folding; add a separate
  varying-input case rather than silently replacing the user's script.
- A per-iteration wrapper function can dominate VM call cost while Node inlines
  it away. Prefer a direct loop body for the primary comparison; label wrapper
  experiments separately. Do not call its entire cost an addition's cost.
- Instruction ratios are not elapsed-time speedups or universal runtime scores.
  Whole-process elapsed time includes controller/signal waiting; it is diagnostic,
  not compiler timing or isolated workload timing. No RSS/GC-pause claims.

## Tests

```bash
python3 -m unittest discover -s benchmarks/instructions -p 'test_*.py' -v
INSTRUCTION_TEST_LIVE=1 \
INSTRUCTION_MARKER_LIBRARY="$PWD/.auto/artifacts/instruction-helper/markers.so" \
python3 -m unittest discover -s benchmarks/instructions -p 'test_*.py' -v
```

The normal tests use fake counters to test orchestration without perf permissions.
The opt-in tests exercise real process/region hardware counting and missing-marker
failure. They fail, rather than skip, on hardware permission errors. Region live
tests require the explicitly supplied helper path.
