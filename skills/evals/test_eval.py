import argparse
import contextlib
import io
import json
import pathlib
import sys
import tempfile
import unittest

import run


class EvalRunnerTest(unittest.TestCase):
    def test_paired_trials_keep_rubric_out_of_workspace_and_preserve_fixture(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "eval"
            with contextlib.redirect_stdout(io.StringIO()):
                run.prepare(argparse.Namespace(output=output, agent="codex", repeats=2,
                                               case=["analyze-existing"]))
            suite = json.loads((output / "suite.json").read_text())
            self.assertEqual(len(suite["trials"]), 4)
            for trial in suite["trials"]:
                workspace = output / trial["id"] / "workspace"
                self.assertTrue((workspace / "agent.ts").exists())
                self.assertFalse((workspace / "cases.json").exists())
                self.assertEqual((workspace / ".agents/skills/submilli/SKILL.md").exists(),
                                 trial["variant"] == "skill")

    def test_failures_cannot_be_graded_into_passes(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "eval"
            with contextlib.redirect_stdout(io.StringIO()):
                run.prepare(argparse.Namespace(output=output, agent="claude", repeats=1,
                                               case=["unrelated-css"]))
                run.run(argparse.Namespace(output=output, trial=None, timeout=10,
                                           command=[sys.executable, "-c", "raise SystemExit(3)"]))
            suite = json.loads((output / "suite.json").read_text())
            checks = suite["cases"][0]["criteria"]
            grades = {t["id"]: {c: {"pass": True, "evidence": "test evidence"} for c in checks}
                      for t in suite["trials"]}
            grade_path = output / "grades.json"
            grade_path.write_text(json.dumps(grades))
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(run.score(argparse.Namespace(output=output, grades=grade_path)), 1)
            grades.pop(next(iter(grades)))
            grade_path.write_text(json.dumps(grades))
            with self.assertRaises(ValueError):
                run.score(argparse.Namespace(output=output, grades=grade_path))

    def test_cases_have_unique_ids_and_observable_rubrics(self):
        cases = run.cases()
        self.assertEqual(len(cases), len({c["id"] for c in cases}))
        for case in cases:
            self.assertTrue(case["prompt"].strip())
            self.assertTrue(case["criteria"])
            checks = case["criteria"] + case["critical"]
            self.assertEqual(len(checks), len(set(checks)))
            if "fixture" in case:
                self.assertTrue((run.HERE / "fixtures" / case["fixture"]).is_dir())


if __name__ == "__main__":
    unittest.main()
