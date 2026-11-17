"""Standard-library tests for the suite contract; no installed runtimes needed."""
import contextlib
import io
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest import mock

import run


class ProtocolTests(unittest.TestCase):
    def setUp(self):
        self.case = run.load_manifest()["benchmarks"][0]
        self.result = {"benchmark": "sieve", "warmup": 2, "iterations": 1000,
                       "result": 669, "elapsed_ms": 125}

    def parse(self, result):
        return run.parse_result(run.PREFIX + json.dumps(result), self.case, 2, 1000)

    def test_valid_record_with_unrelated_output(self):
        output = "runtime banner\n" + run.PREFIX + json.dumps(self.result) + "\n"
        self.assertEqual(run.parse_result(output, self.case, 2, 1000), self.result)

    def test_wrong_result_or_workload_is_rejected(self):
        for field, value in [("result", 668), ("benchmark", "other"),
                             ("iterations", 999), ("warmup", 0), ("iterations", True)]:
            with self.subTest(field=field), self.assertRaises(run.SuiteError):
                self.parse({**self.result, field: value})

    def test_invalid_timings_are_rejected(self):
        for value in [0, -1, float("nan"), float("inf"), True, "10", None]:
            with self.subTest(value=value), self.assertRaises(run.SuiteError):
                self.parse({**self.result, "elapsed_ms": value})

    def test_missing_duplicate_and_malformed_records_are_rejected(self):
        valid = run.PREFIX + json.dumps(self.result)
        for stdout in ["", valid + "\n" + valid, run.PREFIX + "{", run.PREFIX + "[]"]:
            with self.subTest(stdout=stdout), self.assertRaises(run.SuiteError):
                run.parse_result(stdout, self.case, 2, 1000)

    def test_payload_cannot_override_external_measurements(self):
        parsed = self.parse({**self.result, "process_ms": -1, "command": ["forged"]})
        self.assertNotIn("process_ms", parsed)
        self.assertNotIn("command", parsed)

    def test_boxlang_engines_share_exact_source_and_named_options(self):
        matchbox = run.benchmark_command("matchbox", "mb", self.case, 2, 1000)
        boxlang = run.benchmark_command("boxlang", "bx", self.case, 2, 1000)
        self.assertEqual(matchbox[1:], boxlang[1:])
        self.assertEqual(matchbox[-2:], ["--warmup=2", "--iterations=1000"])

    def test_known_prime_count_independently(self):
        count = sum(all(n % divisor for divisor in range(2, int(n ** 0.5) + 1))
                    for n in range(2, self.case["problem_size"] + 1))
        self.assertEqual(count, self.case["expected_result"])

    def test_medians_and_mad(self):
        samples = [{"elapsed_ms": n, "process_ms": n + 100} for n in [100, 120, 110]]
        summary = run.summarize(samples, 10)
        self.assertEqual(summary["batch_median_ms"], 110)
        self.assertEqual(summary["batch_mad_ms"], 10)
        self.assertEqual(summary["ms_per_iteration"], 11)
        self.assertEqual(summary["process_median_ms"], 210)
        self.assertFalse(summary["short_batch"])


class ExecutionTests(unittest.TestCase):
    def test_upstream_edits_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "suite"
            shutil.copytree(run.ROOT, root, ignore=shutil.ignore_patterns("results", "__pycache__"))
            (root / "awfy/javascript/sieve.js").write_text("changed")
            with self.assertRaisesRegex(run.SuiteError, "Upstream source missing or modified"):
                run.load_manifest(root)

    def test_nonzero_exit_and_timeout_are_failures(self):
        with self.assertRaisesRegex(run.SuiteError, "Exit 7"):
            run.run_command([sys.executable, "-c", "import sys; sys.exit(7)"], 10)
        with self.assertRaisesRegex(run.SuiteError, "Timeout"):
            run.run_command([sys.executable, "-c", "import time; time.sleep(10)"], 0.05)

    def test_missing_engine_writes_failed_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "report.json"
            with contextlib.redirect_stderr(io.StringIO()):
                status = run.main(["--runtimes", "node", "--node-bin", str(output / "missing"),
                                   "--output", str(output)])
            self.assertEqual(status, 1)
            report = json.loads(output.read_text())
            self.assertEqual(report["status"], "failed")
            self.assertIn("Executable not found", report["error"])

    def test_mocked_two_engine_run_and_report(self):
        calls = []
        def capture(command, timeout):
            calls.append(command)
            stdout = "test version"
            if command[-1] != "--version":
                stdout = run.PREFIX + json.dumps({"benchmark": "sieve", "warmup": 0,
                         "iterations": 10, "result": 669, "elapsed_ms": 500})
            return {"stdout": stdout, "stderr": "", "process_ms": 1000}
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "report.json"
            with mock.patch.object(run, "run_command", side_effect=capture), \
                 contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
                status = run.main(["--runtimes", "node", "boxlang", "--node-bin", sys.executable,
                                   "--boxlang-bin", sys.executable, "--warmup", "0", "--iterations", "10",
                                   "--samples", "2", "--output", str(output)])
            self.assertEqual(status, 0)
            report = json.loads(output.read_text())
            self.assertEqual(report["status"], "complete")
            for runtime in ["node", "boxlang"]:
                result = report["benchmarks"]["sieve"]["runtimes"][runtime]
                self.assertEqual(len(result["samples"]), 2)
                self.assertEqual(result["summary"]["ms_per_iteration"], 50)
            # Version calls first, then Node/BoxLang followed by BoxLang/Node.
            self.assertTrue(calls[2][1].endswith("harness.js"))
            self.assertTrue(calls[3][1].endswith("sieve.bxs"))
            self.assertTrue(calls[4][1].endswith("sieve.bxs"))
            self.assertTrue(calls[5][1].endswith("harness.js"))

    def test_report_cannot_overwrite_manifest(self):
        original = run.sha256(run.ROOT / "manifest.json")
        with contextlib.redirect_stderr(io.StringIO()):
            status = run.main(["--output", str(run.ROOT / "manifest.json")])
        self.assertEqual(status, 1)
        self.assertEqual(run.sha256(run.ROOT / "manifest.json"), original)

    def test_bad_result_preserves_raw_output_in_failed_report(self):
        def capture(command, timeout):
            stdout = "version" if command[-1] == "--version" else run.PREFIX + json.dumps({
                "benchmark": "sieve", "warmup": 0, "iterations": 10, "result": 668, "elapsed_ms": 100})
            return {"stdout": stdout, "stderr": "", "process_ms": 200}
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "report.json"
            with mock.patch.object(run, "run_command", side_effect=capture), contextlib.redirect_stderr(io.StringIO()):
                status = run.main(["--runtimes", "node", "--node-bin", sys.executable,
                                   "--warmup", "0", "--iterations", "10", "--output", str(output)])
            report = json.loads(output.read_text())
            self.assertEqual(status, 1)
            self.assertEqual(report["status"], "failed")
            self.assertIn("668", report["benchmarks"]["sieve"]["runtimes"]["node"]["samples"][0]["stdout"])

    def test_changed_inputs_invalidate_run(self):
        def capture(command, timeout):
            stdout = "version" if command[-1] == "--version" else run.PREFIX + json.dumps({
                "benchmark": "sieve", "warmup": 0, "iterations": 10, "result": 669, "elapsed_ms": 100})
            return {"stdout": stdout, "stderr": "", "process_ms": 200}
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "report.json"
            with mock.patch.object(run, "run_command", side_effect=capture), \
                 mock.patch.object(run, "suite_hashes", side_effect=[{}, {"changed": "hash"}]), \
                 contextlib.redirect_stderr(io.StringIO()):
                status = run.main(["--runtimes", "node", "--node-bin", sys.executable,
                                   "--warmup", "0", "--iterations", "10", "--samples", "1",
                                   "--output", str(output)])
            self.assertEqual(status, 1)
            report = json.loads(output.read_text())
            self.assertEqual(report["status"], "failed")
            self.assertIn("changed during the run", report["error"])

    def test_invalid_cli_counts_and_duplicates(self):
        manifest = run.load_manifest()
        for args in [["--iterations", "0"], ["--warmup", "-1"], ["--samples", "0"],
                     ["--timeout", "nan"], ["--runtimes", "node", "node"]]:
            with self.subTest(args=args), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                run.arguments(manifest, args)


if __name__ == "__main__":
    unittest.main()
