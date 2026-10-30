#!/usr/bin/env python3
"""Run verified AWFY workloads on MatchBox, Node.js, and JVM BoxLang.

Python standard library only. This runs benchmarks, not an optimization loop.
"""
import argparse
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
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent
RUNTIMES = ("matchbox", "node", "boxlang")
PREFIX = "BENCHMARK_RESULT "


class SuiteError(Exception):
    pass


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_manifest(root=ROOT):
    manifest = json.loads((root / "manifest.json").read_text())
    names = set()
    for case in manifest["benchmarks"]:
        name = case["name"]
        if name in names:
            raise SuiteError(f"Duplicate benchmark: {name}")
        names.add(name)
        for language in ("javascript", "boxlang"):
            path = (root / case[language]).resolve()
            if not path.is_relative_to(root.resolve()) or not path.is_file():
                raise SuiteError(f"Missing or invalid {language} source for {name}: {path}")
    if not names:
        raise SuiteError("The manifest must contain at least one benchmark")
    for relative, expected in manifest["upstream"]["files"].items():
        path = (root / relative).resolve()
        if not path.is_relative_to(root.resolve()) or not path.is_file() or sha256(path) != expected:
            raise SuiteError(f"Upstream source missing or modified: {relative}")
    return manifest


def suite_hashes():
    # Include local ports, helpers, licenses, manifest and runner, not generated reports.
    paths = [ROOT / "manifest.json", ROOT / "run.py", *ROOT.glob("harness.*")]
    paths.extend(path for path in (ROOT / "awfy").rglob("*") if path.is_file())
    return {str(path.relative_to(ROOT)): sha256(path) for path in sorted(paths)}


def executable(value):
    found = shutil.which(value)
    if found is None:
        raise SuiteError(f"Executable not found: {value}. Set the corresponding --*-bin option.")
    # Preserve launcher symlinks: some runtimes use their invocation path for discovery.
    return str(Path(found).absolute())


def run_command(command, timeout, cwd=ROOT):
    started = time.perf_counter()
    with subprocess.Popen(command, cwd=cwd, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          text=True, encoding="utf-8", errors="replace",
                          start_new_session=(os.name == "posix")) as process:
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
            stdout, stderr = process.communicate()
            raise SuiteError(f"Timeout after {timeout}s: {command!r}\n{stdout[-2000:]}\n{stderr[-2000:]}")
        elapsed = (time.perf_counter() - started) * 1000
        if process.returncode:
            raise SuiteError(f"Exit {process.returncode}: {command!r}\n{stdout[-4000:]}\n{stderr[-4000:]}")
    return {"stdout": stdout, "stderr": stderr, "process_ms": elapsed}


def parse_result(stdout, case, warmup, iterations):
    lines = [line[len(PREFIX):] for line in stdout.splitlines() if line.startswith(PREFIX)]
    if len(lines) != 1:
        raise SuiteError(f"Expected exactly one {PREFIX.strip()} record, got {len(lines)}")
    try:
        result = json.loads(lines[0])
    except json.JSONDecodeError as error:
        raise SuiteError(f"Invalid benchmark JSON: {error}") from error
    expected = {"benchmark": case["name"], "warmup": warmup,
                "iterations": iterations, "result": case["expected_result"]}
    if not isinstance(result, dict):
        raise SuiteError("Benchmark record must be an object")
    for key, value in expected.items():
        actual = result.get(key)
        if actual != value or (isinstance(actual, bool) and not isinstance(value, bool)):
            raise SuiteError(f"Invalid {key}: expected {value!r}, got {actual!r}")
    elapsed = result.get("elapsed_ms")
    if isinstance(elapsed, bool) or not isinstance(elapsed, (int, float)) or not math.isfinite(elapsed) or elapsed <= 0:
        raise SuiteError("elapsed_ms must be finite and positive; increase --iterations if the timer returned zero")
    # Workload payloads must not overwrite externally measured process_ms,
    # commands or raw output when merged into a sample.
    return {key: result[key] for key in (*expected, "elapsed_ms")}


def benchmark_command(runtime, binary, case, warmup, iterations):
    if runtime == "node":
        return [binary, str(ROOT / "harness.js"), str(ROOT / case["javascript"]),
                case["name"], str(warmup), str(iterations)]
    return [binary, str(ROOT / case["boxlang"]),
            f"--warmup={warmup}", f"--iterations={iterations}"]


def summarize(samples, iterations):
    batches = [sample["elapsed_ms"] for sample in samples]
    median = statistics.median(batches)
    return {
        "batch_median_ms": median,
        "batch_mad_ms": statistics.median(abs(value - median) for value in batches),
        "ms_per_iteration": median / iterations,
        "process_median_ms": statistics.median(sample["process_ms"] for sample in samples),
        "short_batch": min(batches) < 100,
    }


def positive_int(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def nonnegative_int(value):
    number = int(value)
    if number < 0:
        raise argparse.ArgumentTypeError("must be nonnegative")
    return number


def positive_seconds(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("must be finite and positive")
    return number


def arguments(manifest, argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true", help="list benchmarks without needing runtimes")
    parser.add_argument("--benchmarks", nargs="+", choices=[case["name"] for case in manifest["benchmarks"]])
    parser.add_argument("--runtimes", nargs="+", choices=RUNTIMES, default=list(RUNTIMES))
    parser.add_argument("--matchbox-bin", default=os.environ.get("MATCHBOX_BIN", str(REPO / "target/release/matchbox")))
    parser.add_argument("--node-bin", default=os.environ.get("NODE_BIN", "node"))
    parser.add_argument("--boxlang-bin", default=os.environ.get("BOXLANG_BIN", "boxlang"))
    parser.add_argument("--warmup", type=nonnegative_int, default=100)
    parser.add_argument("--iterations", type=positive_int, default=1000)
    parser.add_argument("--samples", type=positive_int, default=3)
    parser.add_argument("--timeout", type=positive_seconds, default=120, help="seconds per process")
    parser.add_argument("--output", type=Path, help="JSON report path; default: results/<timestamp>.json")
    parser.add_argument("--note", default="", help="record build/toolchain/features or other run context")
    args = parser.parse_args(argv)
    if len(set(args.runtimes)) != len(args.runtimes):
        parser.error("duplicate runtimes are not allowed")
    if args.benchmarks and len(set(args.benchmarks)) != len(args.benchmarks):
        parser.error("duplicate benchmarks are not allowed")
    return args


def main(argv=None):
    report = None
    output = None
    try:
        manifest = load_manifest()
        args = arguments(manifest, argv)
        cases = [case for case in manifest["benchmarks"] if not args.benchmarks or case["name"] in args.benchmarks]
        if args.list:
            for case in cases:
                print(f"{case['name']}: {case['description']}")
            return 0
        timestamp = datetime.now(timezone.utc)
        output = args.output or ROOT / "results" / f"{timestamp.strftime('%Y%m%dT%H%M%S.%fZ')}.json"
        hashes = suite_hashes()
        if output.resolve() in {(ROOT / relative).resolve() for relative in hashes}:
            raise SuiteError("The report path must not overwrite a benchmark input")
        git = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True)
        dirty = subprocess.run(["git", "status", "--porcelain"], cwd=REPO, capture_output=True, text=True)
        report = {
            "schema_version": 1, "status": "running", "timestamp_utc": timestamp.isoformat(),
            "host": platform.uname()._asdict(), "cpu_count": os.cpu_count(),
            "python": platform.python_version(), "repository_commit": git.stdout.strip(),
            "repository_dirty": bool(dirty.stdout.strip()), "suite_sha256": hashes,
            "upstream": manifest["upstream"], "note": args.note,
            "settings": {"warmup": args.warmup, "iterations": args.iterations,
                         "samples": args.samples, "timeout_seconds": args.timeout},
            "runtimes": {}, "benchmarks": {},
        }
        # Preflight every requested engine. Missing engines never become silent skips.
        for runtime in args.runtimes:
            binary = executable(getattr(args, runtime + "_bin"))
            version = run_command([binary, "--version"], min(args.timeout, 30))
            report["runtimes"][runtime] = {
                "executable": binary, "executable_sha256": sha256(Path(binary)),
                "version": (version["stdout"] + version["stderr"]).strip(),
            }
        for case in cases:
            runs = {runtime: {"samples": []} for runtime in args.runtimes}
            report["benchmarks"][case["name"]] = {"case": case, "runtimes": runs}
            for trial in range(args.samples):
                # Rotate order to reduce systematic first/last-engine bias.
                offset = trial % len(args.runtimes)
                order = args.runtimes[offset:] + args.runtimes[:offset]
                for runtime in order:
                    command = benchmark_command(runtime, report["runtimes"][runtime]["executable"],
                                                case, args.warmup, args.iterations)
                    print(f"{case['name']} / {runtime}: sample {trial + 1}/{args.samples}", file=sys.stderr, flush=True)
                    captured = run_command(command, args.timeout)
                    sample = {"trial": trial + 1, "command": command, **captured}
                    runs[runtime]["samples"].append(sample)
                    sample.update(parse_result(captured["stdout"], case, args.warmup, args.iterations))
            for runtime, run in runs.items():
                run["summary"] = summarize(run["samples"], args.iterations)
            # Check that no workload or harness changed while the suite was running.
            if suite_hashes() != hashes:
                raise SuiteError("Benchmark inputs changed during the run; report is not valid")
        report["status"] = "complete"
        print("\n| benchmark | runtime | batch median ms | MAD ms | ms/iteration | process median ms |")
        print("|---|---|---:|---:|---:|---:|")
        for name, case_report in report["benchmarks"].items():
            for runtime, run in case_report["runtimes"].items():
                summary = run["summary"]
                print(f"| {name} | {runtime} | {summary['batch_median_ms']:.3f} | "
                      f"{summary['batch_mad_ms']:.3f} | {summary['ms_per_iteration']:.6f} | "
                      f"{summary['process_median_ms']:.3f} |")
                if summary["short_batch"]:
                    print(f"WARNING: {name}/{runtime} has a batch below 100ms; increase --iterations "
                          "before drawing conclusions from millisecond-resolution timings.", file=sys.stderr)
        print("\nWorkload includes allocation and result checks; process time also includes startup, source processing and warmup.")
        return 0
    except (SuiteError, OSError, ValueError, KeyError) as error:
        if report is not None:
            report["status"] = "failed"
            report["error"] = str(error)
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    finally:
        if report is not None and output is not None:
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
            print(f"Report: {output}", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main())
