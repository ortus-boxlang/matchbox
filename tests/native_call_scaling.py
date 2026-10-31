#!/usr/bin/env python3
"""Public positional-BIF scaling gate: unrelated globals must not slow native calls."""
import json, os, statistics, subprocess, sys, tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MATCHBOX = str(Path(sys.argv[1] if len(sys.argv) > 1 else os.environ.get('MATCHBOX', ROOT / 'target/lint-dev/debug/matchbox')).resolve())
ITERATIONS = 40000
with tempfile.TemporaryDirectory(prefix='matchbox-native-calls-') as directory:
    work = Path(directory); binaries = {}; timings = {0: [], 8192: []}
    # Preserve named/reference/callback behavior on both source-free runner formats.
    script = work / 'modes.bxs'; script.write_bytes((ROOT / 'tests/scripts/vm_native_calls.bxs').read_bytes())
    bytecode = work / 'modes.bxb'; native = work / 'modes'
    for flags, output in [(['--build'], bytecode), (['--target', 'native'], native)]:
        subprocess.run([MATCHBOX, '--no-shaking', *flags, '--output', str(output), str(script)], cwd=work, check=True)
    script.unlink()
    for index, command in enumerate([[MATCHBOX, str(bytecode)], [str(native)]]):
        env = dict(os.environ, BOXLANG_HOME=str(work / f'mode-run-{index}' / 'home'), MATCHBOX_NATIVE_CACHE=str(work / f'mode-run-{index}' / 'cache'))
        result = subprocess.run(command, cwd=work, env=env, capture_output=True, text=True, timeout=60, check=True)
        assert result.stdout.strip() == 'Native positional / named / reference / callback calls: OK', (result.stdout, result.stderr)
    for count in timings:
        source = ''.join(f'var padding{i}={i};\n' for i in range(count))
        if count: source += f'assert padding{count-1} == {count-1};\n'
        source += f'''var total=0; var i=0; var started=getTickCount();
for(i=0;i<{ITERATIONS};i++){{ total += len("x") + ascii("a") + len(chr(98)); }}
var elapsed=getTickCount()-started;
assert total == {ITERATIONS * 99};
println(jsonSerialize({{ms: elapsed}}));
'''
        script = work / f'calls-{count}.bxs'; binary = work / f'calls-{count}'
        script.write_text(source)
        subprocess.run([MATCHBOX, '--no-shaking', '--target', 'native', '--output', str(binary), str(script)], cwd=work, check=True)
        script.unlink(); binaries[count] = binary
    for trial in range(3):
        for count in (list(timings) if trial != 1 else list(reversed(timings))):
            env = dict(os.environ, BOXLANG_HOME=str(work / f'run-{count}-{trial}' / 'home'), MATCHBOX_NATIVE_CACHE=str(work / f'run-{count}-{trial}' / 'cache'))
            result = subprocess.run([str(binaries[count])], cwd=work, env=env, capture_output=True, text=True, timeout=60, check=True)
            timings[count].append(json.loads(result.stdout)['ms'])
    medians = {count: statistics.median(samples) for count, samples in timings.items()}
    print(json.dumps({'iterations': ITERATIONS, 'samples_ms': timings, 'median_ms': medians}), flush=True)
    # Broad ratio gate, not a hardware-specific latency target; excludes parsing/startup.
    assert medians[8192] <= max(25, medians[0] * 3), f'Positional calls scale with unrelated globals: {medians}'
print('Source-free positional native-call scaling: OK')
