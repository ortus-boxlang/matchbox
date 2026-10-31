import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('instruction_runner', Path(__file__).with_name('run.py'))
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class FakeCounter:
    def __init__(self, *args, **kwargs):
        pass

    def enable(self):
        pass

    def disable(self):
        pass

    def read(self):
        return dict(instructions=123, time_enabled_ns=10, time_running_ns=10)

    def close(self):
        pass


class RunnerTests(unittest.TestCase):
    def test_event_flags(self):
        attr = runner.event_attributes(True)
        kind, size, config, _, _, fmt, flags = struct.unpack_from('IIQQQQQ', attr)
        self.assertEqual((kind, size, config, fmt), (0, 128, 1, 3))
        for bit in (0, 1, 5, 6, 12):
            self.assertTrue(flags & (1 << bit))
        self.assertFalse(struct.unpack_from('Q', runner.event_attributes(), 40)[0] & (1 << 12))

    def test_summary(self):
        self.assertEqual(runner.summarize([10, 11, 90]), dict(median=11, mad=1, min=10, max=90))

    def test_output_validation(self):
        runner.validate_output('warm\n__ICOUNT_START__\n__ICOUNT_END__\n42\n', '42', region=True)
        runner.validate_output('42\n', expected_stdout='42\n')
        with self.assertRaises(ValueError):
            runner.validate_output('wrong\n', '42')
        with self.assertRaises(ValueError):
            runner.validate_output('42\n', expected_stdout='42')

    def test_bad_marker_sequences(self):
        for text in ('', '__ICOUNT_END__\n__ICOUNT_START__\n',
                     '__ICOUNT_START__\n__ICOUNT_END__\n__ICOUNT_START__\n'):
            with self.subTest(text=text), self.assertRaises(ValueError):
                runner.validate_output(text, region=True)

    def measure(self, code, **kwargs):
        with mock.patch.object(runner, 'Counter', FakeCounter):
            return runner.measure([sys.executable, '-c', code], Path.cwd(), 'process', None,
                                  kwargs.pop('timeout', 5), **kwargs)

    def test_process_launch_and_validation(self):
        result = self.measure('print(42)', expected_last_line='42')
        self.assertEqual(result['status'], 'complete')
        self.assertEqual(result['instructions'], 123)
        self.assertEqual(result['stdout'], '42\n')
        self.assertNotIn('main_thread_instructions', result)

    def test_wrong_answer_fails_with_evidence(self):
        result = self.measure('print(41)', expected_last_line='42')
        self.assertEqual(result['status'], 'failed')
        self.assertEqual(result['stdout'], '41\n')

    def test_nonzero_exit(self):
        result = self.measure('import sys; print("diagnostic", file=sys.stderr); sys.exit(2)')
        self.assertEqual(result['status'], 'failed')
        self.assertEqual(result['exit_code'], 2)
        self.assertIn('diagnostic', result['stderr'])

    def test_timeout(self):
        result = self.measure('import time; time.sleep(5)', timeout=.05)
        self.assertEqual(result['status'], 'failed')
        self.assertIn('TimeoutError', result['error'])

    def test_missing_counter_fails_not_falls_back(self):
        with mock.patch.object(runner, 'Counter', side_effect=PermissionError('denied')):
            result = runner.measure([sys.executable, '-c', 'print(42)'], Path.cwd(),
                                    'process', None, 5, '42')
        self.assertEqual(result['status'], 'failed')
        self.assertIn('denied', result['error'])

    def test_reject_multiplexed_counter(self):
        counter = object.__new__(runner.Counter)
        counter.fd = 99
        with mock.patch.object(os, 'read', return_value=struct.pack('QQQ', 100, 10, 9)):
            with self.assertRaisesRegex(RuntimeError, 'multiplexed'):
                counter.read()

    def test_cli_report_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(runner, 'Counter', FakeCounter):
            output = Path(tmp) / 'result.json'
            argv = ['--output', str(output), '--samples', '2', '--expect-last-line', '42',
                    '--input', __file__, '--', sys.executable, '-c', 'print(42)']
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(runner.main(argv), 0)
            report = json.loads(output.read_text())
            self.assertEqual(report['status'], 'complete')
            self.assertEqual(len(report['samples']), 2)
            self.assertEqual(report['summary']['instructions']['median'], 123)
            self.assertTrue(report['inputs'][0]['sha256'])
            before = output.read_bytes()
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                runner.main(argv)
            self.assertEqual(output.read_bytes(), before)

    def test_failed_report_preserved(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(runner, 'Counter', FakeCounter):
            output = Path(tmp) / 'result.json'
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                code = runner.main(['--output', str(output), '--expect-last-line', '42',
                                    '--', sys.executable, '-c', 'print(41)'])
            self.assertEqual(code, 1)
            report = json.loads(output.read_text())
            self.assertEqual(report['status'], 'failed')
            self.assertEqual(len(report['samples']), 1)
            self.assertEqual(report['samples'][0]['stdout'], '41\n')

    def test_cli_requires_validation(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            runner.main(['--', sys.executable, '-c', 'pass'])


@unittest.skipUnless(os.environ.get('INSTRUCTION_TEST_LIVE') == '1', 'opt-in hardware tests')
class HardwareTests(unittest.TestCase):
    def test_process_hardware(self):
        result = runner.measure([sys.executable, '-c', 'print(42)'], Path.cwd(),
                                'process', None, 10, '42')
        self.assertEqual(result['status'], 'complete', result)
        self.assertGreater(result['instructions'], 0)

    def test_region_hardware(self):
        lib = os.environ.get('INSTRUCTION_MARKER_LIBRARY')
        if not lib:
            self.skipTest('set INSTRUCTION_MARKER_LIBRARY to built markers.so')
        code = ('import os; os.write(1,b"__ICOUNT_START__\\n"); '
                'sum(range(1000)); os.write(1,b"__ICOUNT_END__\\n"); print(42)')
        result = runner.measure([sys.executable, '-c', code], Path.cwd(),
                                'region', Path(lib).resolve(), 10, '42')
        self.assertEqual(result['status'], 'complete', result)
        self.assertGreater(result['main_thread_instructions'], 0)

    def test_region_writev_hardware(self):
        lib = os.environ.get('INSTRUCTION_MARKER_LIBRARY')
        if not lib:
            self.skipTest('set INSTRUCTION_MARKER_LIBRARY to built markers.so')
        code = ('import os; os.writev(1,[b"__ICOUNT_START__",b"\\n"]); '
                'sum(range(1000)); os.writev(1,[b"__ICOUNT_END__",b"\\n"]); print(42)')
        result = runner.measure([sys.executable, '-c', code], Path.cwd(),
                                'region', Path(lib).resolve(), 10, '42')
        self.assertEqual(result['status'], 'complete', result)

    def test_region_timeout_after_start(self):
        lib = os.environ.get('INSTRUCTION_MARKER_LIBRARY')
        if not lib:
            self.skipTest('set INSTRUCTION_MARKER_LIBRARY to built markers.so')
        code = 'import os,time; os.write(1,b"__ICOUNT_START__\\n"); time.sleep(5)'
        result = runner.measure([sys.executable, '-c', code], Path.cwd(),
                                'region', Path(lib).resolve(), .1, '42')
        self.assertEqual(result['status'], 'failed')
        self.assertIn('TimeoutError', result['error'])

    def test_missing_region_marker_fails(self):
        lib = os.environ.get('INSTRUCTION_MARKER_LIBRARY')
        if not lib:
            self.skipTest('set INSTRUCTION_MARKER_LIBRARY to built markers.so')
        result = runner.measure([sys.executable, '-c', 'print(42)'], Path.cwd(),
                                'region', Path(lib).resolve(), 10, '42')
        self.assertEqual(result['status'], 'failed')
        self.assertIn('before region marker', result['error'])


if __name__ == '__main__':
    unittest.main()
