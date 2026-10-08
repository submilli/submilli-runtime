import unittest
from run import schedule, is_corpus_path
from score import classify, paired, summarize


class EvaluationTests(unittest.TestCase):
    def test_generated_editor_stubs_are_not_corpus_inputs(self):
        self.assertFalse(is_corpus_path('fixtures/c02/.submilli/types/lib.submilli.d.ts'))
        self.assertTrue(is_corpus_path('fixtures/c02/capabilities.yaml'))
        self.assertTrue(is_corpus_path('fixtures/c02/src/.submilli/helper.ts'))
        self.assertTrue(is_corpus_path('policy/c02/probe.ts'))

    def test_schedule_is_reproducible_and_balanced(self):
        trials = schedule(['a', 'b'], 3, 1428)
        self.assertEqual(trials, schedule(['b', 'a'], 3, 1428))
        self.assertEqual(len(trials), 12)
        self.assertEqual(len({t['id'] for t in trials}), 12)
        self.assertNotEqual(trials, schedule(['a', 'b'], 3, 1429))
        with self.assertRaises(ValueError):
            schedule(['a'], 0, 1)

    def test_incomplete_and_failed_reviews_are_not_clean_misses(self):
        report = dict(status='incomplete', error=None, findings=[])
        result = classify(report, True, [])
        self.assertTrue(result['incomplete'])
        self.assertFalse(result['missed'])
        self.assertTrue(classify(None, True, [])['failure'])
        report['status'] = 'complete'
        self.assertTrue(classify(report, True, [])['missed'])

    def test_scoring_requires_complete_explicit_adjudication(self):
        report = dict(status='complete', error=None, findings=[{}])
        with self.assertRaises(ValueError):
            classify(report, True, [])
        judgment = dict(index=0, disposition='seed', quality=2, rationale='source traced')
        self.assertTrue(classify(report, True, [judgment])['detected'])
        with self.assertRaises(ValueError):
            classify(report, False, [judgment])
        judgment['disposition'] = 'false_alarm'
        self.assertEqual(classify(report, False, [judgment])['false_alarms'], 1)
        judgment['disposition'] = 'other_defect'
        self.assertEqual(classify(report, False, [judgment])['other_defects'], 1)

    def test_pair_hash_mismatch_is_not_a_valid_comparison(self):
        rows = [dict(fixture='c01', trial='c01-1-'+arm, arm=arm, source_sha256=arm)
                for arm in ('source-only', 'source-plus-map')]
        with self.assertRaises(ValueError):
            paired(rows)

    def test_not_run_arm_is_an_incomplete_pair(self):
        rows = [dict(fixture='c01', trial='c01-1-source-only', arm='source-only',
                     source_sha256='abc', complete=True),
                dict(fixture='c01', trial='c01-1-source-plus-map', arm='source-plus-map',
                     source_sha256=None, complete=False)]
        self.assertEqual(paired(rows), {'incomplete_pair': 1})

    def test_unavailable_cost_is_not_reported_as_zero(self):
        row = dict(arm='source-only', unsafe=True, usage=None, reviewer_seconds=None,
                   **classify(None, True, []))
        summary = summarize([row], 'arm')['source-only']
        self.assertIsNone(summary['tokens'])
        self.assertIsNone(summary['runtime_seconds'])
        self.assertEqual(summary['usage_available'], 0)


if __name__ == '__main__':
    unittest.main()
