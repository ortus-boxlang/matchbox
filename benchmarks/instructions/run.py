#!/usr/bin/env python3
"""Linux x86-64 retired-instruction counter. Python standard library only."""
import argparse
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
import signal
import statistics
import struct
import subprocess
import sys
import tempfile
import time

START = '__ICOUNT_START__'
END = '__ICOUNT_END__'


def event_attributes(enable_on_exec=False):
    attr = bytearray(128)
    # disabled, inherit, exclude_kernel, exclude_hv, optionally enable_on_exec.
    flags = (1 << 0) | (1 << 1) | (1 << 5) | (1 << 6)
    if enable_on_exec:
        flags |= 1 << 12
    # PERF_TYPE_HARDWARE=0, PERF_COUNT_HW_INSTRUCTIONS=1,
    # PERF_FORMAT_TOTAL_TIME_ENABLED | PERF_FORMAT_TOTAL_TIME_RUNNING = 3.
    struct.pack_into('IIQQQQQ', attr, 0, 0, 128, 1, 0, 0, 3, flags)
    return bytes(attr)


class Counter:
    def __init__(self, tid, enable_on_exec=False):
        libc = ctypes.CDLL(None, use_errno=True)
        buf = ctypes.create_string_buffer(event_attributes(enable_on_exec))
        # __NR_perf_event_open=298 on Linux x86-64, PERF_FLAG_FD_CLOEXEC=8.
        self.fd = libc.syscall(298, ctypes.byref(buf), tid, -1, -1, 8)
        if self.fd < 0:
            err = ctypes.get_errno()
            raise OSError(err, f'perf_event_open: {os.strerror(err)}; '
                          'hardware counters/permissions unavailable (no fallback)')

    def enable(self):
        import fcntl
        fcntl.ioctl(self.fd, 0x2400, 0)

    def disable(self):
        import fcntl
        fcntl.ioctl(self.fd, 0x2401, 0)

    def read(self):
        value, enabled, running = struct.unpack('QQQ', os.read(self.fd, 24))
        if running <= 0 or running != enabled:
            raise RuntimeError(f'counter unavailable or multiplexed: '
                               f'enabled={enabled}, running={running}; refusing scaled estimate')
        return dict(instructions=value, time_enabled_ns=enabled, time_running_ns=running)

    def close(self):
        os.close(self.fd)


def wait_for(pid, deadline, stopped=False):
    while time.monotonic() < deadline:
        found, status = os.waitpid(pid, os.WNOHANG | os.WUNTRACED)
        if found:
            if os.WIFSTOPPED(status):
                if stopped and os.WSTOPSIG(status) == signal.SIGSTOP:
                    return status
                raise RuntimeError(f'unexpected target stop: signal {os.WSTOPSIG(status)}')
            if stopped:
                raise RuntimeError(f'target exited before region marker: '
                                   f'exit={os.waitstatus_to_exitcode(status)}')
            return status
        time.sleep(.001)
    raise TimeoutError('sample timeout (including startup, warmup, execution and shutdown)')


def validate_output(stdout, expected_last_line=None, expected_stdout=None, region=False):
    lines = stdout.splitlines()
    if region and [line for line in lines if line in (START, END)] != [START, END]:
        raise ValueError('expected exactly one START then one END marker on separate stdout lines')
    if expected_stdout is not None and stdout != expected_stdout:
        raise ValueError('stdout differs from expected output file')
    if expected_last_line is not None:
        nonempty = [line for line in lines if line.strip()]
        if not nonempty or nonempty[-1] != expected_last_line:
            raise ValueError(f'last nonempty stdout line does not equal {expected_last_line!r}')


def measure(command, cwd, mode, library, timeout, expected_last_line=None, expected_stdout=None):
    """Always return evidence, even on failure. SIGKILL/reap target on every error."""
    sample = dict(command=command, status='failed')
    counters = {}
    pid = None
    gate_r = gate_w = None
    begin = time.monotonic()
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        try:
            env = dict(os.environ)
            if mode == 'region':
                env['LD_PRELOAD'] = str(library) + (
                    ':' + env['LD_PRELOAD'] if env.get('LD_PRELOAD') else '')
            gate_r, gate_w = os.pipe()
            pid = os.fork()
            if pid == 0:
                try:
                    os.close(gate_w)
                    os.setsid()
                    os.dup2(stdout.fileno(), 1)
                    os.dup2(stderr.fileno(), 2)
                    os.chdir(cwd)
                    os.read(gate_r, 1)
                    os.close(gate_r)
                    os.execve(command[0], command, env)
                except BaseException as exc:
                    os.write(2, f'exec failed: {exc}\n'.encode())
                    os._exit(127)
            os.close(gate_r)
            gate_r = None
            deadline = begin + timeout
            if mode == 'process':
                # Child is blocked; enable_on_exec excludes fork/Python setup.
                counters[pid] = Counter(pid, enable_on_exec=True)
            os.write(gate_w, b'x')
            os.close(gate_w)
            gate_w = None
            if mode == 'region':
                wait_for(pid, deadline, stopped=True)
                # SIGSTOP is group-wide. Attach individually to pre-existing
                # threads and inherit any future threads from each of them.
                for task in sorted((Path('/proc') / str(pid) / 'task').iterdir()):
                    tid = int(task.name)
                    counters[tid] = Counter(tid)
                for counter in counters.values():
                    counter.enable()
                os.kill(pid, signal.SIGCONT)
                wait_for(pid, deadline, stopped=True)
                for counter in counters.values():
                    counter.disable()
                sample['threads'] = {str(tid): c.read() for tid, c in counters.items()}
                os.kill(pid, signal.SIGCONT)
            status = wait_for(pid, deadline)
            sample['exit_code'] = os.waitstatus_to_exitcode(status)
            if mode == 'process':
                sample['threads'] = {str(tid): c.read() for tid, c in counters.items()}
            if sample['exit_code'] != 0:
                raise RuntimeError(f'target exited with code {sample["exit_code"]}')
            sample['instructions'] = sum(t['instructions'] for t in sample['threads'].values())
            if mode == 'region':
                sample['main_thread_instructions'] = sample['threads'][str(pid)]['instructions']
            if sample['instructions'] <= 0:
                raise RuntimeError('zero instruction count')
            stdout.seek(0)
            text = stdout.read().decode('utf-8', errors='replace')
            validate_output(text, expected_last_line, expected_stdout, mode == 'region')
            sample['status'] = 'complete'
        except (Exception, KeyboardInterrupt) as exc:
            sample['error'] = f'{type(exc).__name__}: {exc}'
        finally:
            if pid:
                # Also clean up foreground descendants on timeout/failure.
                try:
                    os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    # Do not signal a PID already reaped by wait_for().
                    found, _ = os.waitpid(pid, os.WNOHANG)
                    if not found:
                        os.kill(pid, signal.SIGKILL)
                        os.waitpid(pid, 0)
                except (ChildProcessError, ProcessLookupError):
                    pass
            for fd in (gate_r, gate_w):
                if fd is not None:
                    os.close(fd)
            for counter in counters.values():
                counter.close()
            sample['whole_process_seconds'] = time.monotonic() - begin
            for name, file in [('stdout', stdout), ('stderr', stderr)]:
                file.seek(0)
                sample[name] = file.read().decode('utf-8', errors='replace')
    return sample


def summarize(values):
    median = statistics.median(values)
    return dict(median=median, mad=statistics.median(abs(x-median) for x in values),
                min=min(values), max=max(values))


def file_info(path):
    path = Path(path).resolve()
    return dict(path=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest())


def git_info(cwd):
    try:
        return {name: subprocess.check_output(['git', *args], cwd=cwd, text=True,
                                             stderr=subprocess.DEVNULL, timeout=5).strip()
                for name, args in [('revision', ['rev-parse', 'HEAD']),
                                   ('dirty_status', ['status', '--short'])]}
    except (OSError, subprocess.SubprocessError):
        return None


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=['process', 'region'], default='process')
    parser.add_argument('--marker-library', type=Path, help='required for region mode')
    parser.add_argument('--samples', type=int, default=3)
    parser.add_argument('--timeout', type=float, default=30, help='seconds per fresh process')
    parser.add_argument('--cwd', type=Path, default=Path.cwd())
    parser.add_argument('--output', type=Path, help='new JSON file; never overwrite an existing report')
    parser.add_argument('--input', type=Path, action='append', default=[], help='hash script/module/harness; repeatable')
    parser.add_argument('--expect-last-line', help='required correctness check, or use --expect-stdout-file')
    parser.add_argument('--expect-stdout-file', type=Path, help='exact UTF-8 stdout including markers')
    parser.add_argument('--runtime-version', default='', help='record independently checked version/build info')
    parser.add_argument('--note', default='', help='build profile/features, warmup/iterations, flags, provenance')
    parser.add_argument('command', nargs=argparse.REMAINDER, help='-- executable [flags] script [args]')
    args = parser.parse_args(argv)
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('supply -- executable [flags] script [args]')
    if args.samples < 1 or not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error('samples and timeout must be positive (timeout finite)')
    if args.expect_last_line is None and args.expect_stdout_file is None:
        parser.error('supply --expect-last-line or --expect-stdout-file; exit success alone is not verification')
    if sys.platform != 'linux' or platform.machine() not in ('x86_64', 'AMD64'):
        parser.error('only native Linux x86-64 is supported; no emulation/fallback')
    cwd = args.cwd.resolve()
    if not cwd.is_dir():
        parser.error('--cwd must be a directory')
    # Resolve executable as the child would, relative to the requested cwd.
    if '/' in command[0]:
        exe = (cwd / command[0]).resolve()
    else:
        found = shutil.which(command[0])
        if not found:
            parser.error(f'executable not found: {command[0]}')
        exe = Path(found).absolute()
    if not exe.is_file() or not os.access(exe, os.X_OK):
        parser.error(f'executable unavailable: {exe}')
    command[0] = str(exe)  # Preserve symlink spelling for argv[0]-sensitive launchers.
    if args.mode == 'region':
        if not args.marker_library or not args.marker_library.is_file():
            parser.error('region mode requires --marker-library pointing to a built markers.so')
        args.marker_library = args.marker_library.resolve()
    try:
        inputs = [file_info(p) for p in args.input]
        expected = args.expect_stdout_file.read_bytes().decode('utf-8') if args.expect_stdout_file else None
    except (OSError, UnicodeError) as exc:
        parser.error(str(exc))
    timestamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ')
    output = args.output or Path(__file__).resolve().parent / 'results' / f'{timestamp}.json'
    output.parent.mkdir(parents=True, exist_ok=True)
    try:
        with output.open('x'):
            pass
    except FileExistsError:
        parser.error(f'refusing to overwrite report: {output}')
    report = dict(schema_version=1, status='running', timestamp_utc=timestamp,
                  mode=args.mode, command=command, cwd=str(cwd),
                  scope='user-space retired instructions; inherited threads; kernel/hypervisor excluded',
                  host=dict(system=platform.platform(), machine=platform.machine(),
                            cpu_info=Path('/proc/cpuinfo').read_text().split('\n\n')[0],
                            perf_event_paranoid=Path('/proc/sys/kernel/perf_event_paranoid').read_text().strip()),
                  git=git_info(cwd), executable=file_info(exe), inputs=inputs,
                  runner=file_info(__file__), runtime_version=args.runtime_version, note=args.note,
                  environment={k: v for k, v in os.environ.items()
                               if k in ('NODE_OPTIONS', 'LD_PRELOAD', 'RUSTFLAGS', 'CARGO_BUILD_TARGET',
                                        'MATCHBOX_JIT_TRACE', 'MATCHBOX_NATIVE_CACHE', 'MATCHBOX_TARGET')},
                  requested_samples=args.samples, timeout_seconds=args.timeout,
                  validation=dict(expected_last_line=args.expect_last_line, expected_stdout=expected),
                  samples=[])
    if args.mode == 'region':
        report['marker_library'] = file_info(args.marker_library)
        report['markers'] = dict(start=START, end=END, stop='before marker write')
    try:
        output.write_text(json.dumps(report, indent=2) + '\n')
        for _ in range(args.samples):
            sample = measure(command, cwd, args.mode, args.marker_library, args.timeout,
                             args.expect_last_line, expected)
            report['samples'].append(sample)
            output.write_text(json.dumps(report, indent=2) + '\n')
            if sample['status'] != 'complete':
                raise RuntimeError(sample['error'])
        report['summary'] = {'instructions': summarize([s['instructions'] for s in report['samples']]),
                             'whole_process_seconds': summarize([s['whole_process_seconds'] for s in report['samples']])}
        if args.mode == 'region':
            report['summary']['main_thread_instructions'] = summarize(
                [s['main_thread_instructions'] for s in report['samples']])
        report['status'] = 'complete'
    except (Exception, KeyboardInterrupt) as exc:
        report['status'] = 'failed'
        report['error'] = f'{type(exc).__name__}: {exc}'
    finally:
        output.write_text(json.dumps(report, indent=2) + '\n')
    print(f'{report["status"]}: {output}')
    if report['status'] == 'complete':
        print(json.dumps(report['summary'], indent=2))
        return 0
    print(report['error'], file=sys.stderr)
    return 1


if __name__ == '__main__':
    sys.exit(main())
