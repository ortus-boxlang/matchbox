import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC=importlib.util.spec_from_file_location('campaign',Path(__file__).with_name('campaign.py'))
c=importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(c)

class CampaignTests(unittest.TestCase):
    def test_marginal_metric_and_sample_policy(self):
        calls=[]
        def count(engine,case,n,samples,tag):
            calls.append((case,n,samples))
            return dict(median=700+n*42,mad=9)
        with mock.patch.object(c,'count',side_effect=count):
            values=c.measured('matchbox','test')
        self.assertEqual(values['full']['instructions_per_iteration'],42)
        self.assertEqual(values['full']['noise_instructions_per_iteration'],18/9000)
        self.assertEqual([x[2] for x in calls],[3,3,3,3,5,5])

    def test_negative_slope_fails(self):
        with mock.patch.object(c,'count',return_value=dict(median=100,mad=0)):
            with self.assertRaisesRegex(RuntimeError,'nonpositive slope'):
                c.measured('matchbox','test')

    def test_test_identities_and_known_failure(self):
        text='test a ... ok\ntest b ... FAILED\ntest c ... ignored, environment\ntest result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out;\n'
        with mock.patch.object(c,'command',return_value=text):
            suites=c.tests('test')
        self.assertEqual(suites['vm_lib'],{'a':'ok','b':'FAILED','c':'ignored'})

    def test_partial_test_output_rejected(self):
        for text in ['test a ... ok\n',
                     'test a ... ok\ntest result: ok. 2 passed; 0 failed; 0 ignored;']:
            with self.subTest(text=text),mock.patch.object(c,'command',return_value=text):
                with self.assertRaises(RuntimeError):
                    c.tests('test')

    def test_nested_runner_cannot_hide_outer_failure(self):
        text=('test outer ... \n'
              'running 1 test\ntest outer ... ok\n'
              'test result: ok. 1 passed; 0 failed; 0 ignored;\n'
              'FAILED\ntest another ... ok\n'
              'test result: FAILED. 1 passed; 1 failed; 0 ignored;\n')
        self.assertEqual(c.parse_test_outcomes(text,'integration'),{'outer':'FAILED','another':'ok'})

    def test_budget_cannot_silently_restart(self):
        with tempfile.TemporaryDirectory() as tmp,mock.patch.object(c,'ART',Path(tmp)),mock.patch.object(c,'AUTO',Path(tmp)):
            with mock.patch.object(c.time,'time',return_value=100):
                c.budget()
            with mock.patch.object(c.time,'time',return_value=3701):
                with self.assertRaisesRegex(RuntimeError,'budget exhausted'):
                    c.budget()
            self.assertEqual((Path(tmp)/'session-start.json').read_text(),'{"start": 100}')

if __name__=='__main__': unittest.main()
