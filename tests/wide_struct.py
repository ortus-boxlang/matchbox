#!/usr/bin/env python3
"""Linux public wide-struct memory gate, source-free bytecode/native and fresh processes."""
import os, shutil, subprocess, sys, tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MATCHBOX = str(Path(sys.argv[1] if len(sys.argv) > 1 else os.environ.get('MATCHBOX', ROOT / 'target/lint-dev/debug/matchbox')).resolve())
with tempfile.TemporaryDirectory(prefix='matchbox-wide-struct-') as directory:
    work = Path(directory); script = work / 'wide.bxs'; bytecode = work / 'wide.bxb'; native = work / 'wide'
    shutil.copyfile(ROOT / 'tests/scripts/vm_wide_struct.bxs', script)
    for flags, output in [(['--build'], bytecode), (['--target', 'native'], native)]:
        subprocess.run([MATCHBOX, '--no-shaking', *flags, '--output', str(output), str(script)], cwd=work, check=True)
    script.unlink()
    for label, command in [('bytecode', [MATCHBOX, str(bytecode)]), ('native', [str(native)])]:
        env = dict(os.environ, BOXLANG_HOME=str(work / 'home'), MATCHBOX_NATIVE_CACHE=str(work / f'cache-{label}'))
        with subprocess.Popen(command, cwd=work, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as process:
            try:
                line = process.stdout.readline()
                assert line.strip() == 'Wide struct checks passed', (line, process.stderr.read())
                fields = dict(line.split(':', 1) for line in Path(f'/proc/{process.pid}/status').read_text().splitlines() if ':' in line)
                rss = int(fields['VmRSS'].split()[0]) / 1024; peak = int(fields['VmHWM'].split()[0]) / 1024
                assert peak < 256, f'{label} wide dictionary exceeded 256 MiB: RSS={rss:.1f}, peak={peak:.1f}'
                stdout, stderr = process.communicate(timeout=15)
                assert process.returncode == 0, (stdout, stderr)
                print(f'{label}: wide dictionary RSS={rss:.1f} MiB, peak={peak:.1f} MiB: OK')
            finally:
                if process.poll() is None: process.terminate(); process.wait(timeout=5)
