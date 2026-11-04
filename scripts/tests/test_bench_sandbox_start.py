import importlib.util
import os
import json
import sys
import time
from pathlib import Path
import tempfile
import unittest


MODULE_PATH = Path(__file__).parents[1] / "bench-sandbox-start.py"
SPEC = importlib.util.spec_from_file_location("bench_sandbox_start", MODULE_PATH)
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)


class NearestRankTests(unittest.TestCase):
    def test_uses_nearest_rank_without_interpolation(self):
        samples = [1, 2, 3, 4]
        self.assertEqual(BENCH.nearest_rank(samples, 50), 2)
        self.assertEqual(BENCH.nearest_rank(samples, 95), 4)
        self.assertEqual(BENCH.nearest_rank([10, 20, 30], 50), 20)

    def test_rejects_empty_samples_and_bad_percentiles(self):
        with self.assertRaises(ValueError):
            BENCH.nearest_rank([], 50)
        with self.assertRaises(ValueError):
            BENCH.nearest_rank([1], 0)
        with self.assertRaises(ValueError):
            BENCH.nearest_rank([1], 101)

    def test_summary_requires_positive_integer_nanoseconds(self):
        with self.assertRaises(BENCH.BenchmarkFailure):
            BENCH.summary_ns([1, 0])
        with self.assertRaises(BENCH.BenchmarkFailure):
            BENCH.summary_ns([1, 2.0])
        with self.assertRaises(BENCH.BenchmarkFailure):
            BENCH.summary_ns([True])

    def test_truncated_cli_report_allows_zero(self):
        self.assertEqual(BENCH.summary_ns([0, 0, 1000000], allow_zero=True)["p50_ns"], 0)

    def test_median_is_taken_per_quantile_across_repetitions(self):
        summaries = [
            {"p50_ns": 1, "p95_ns": 20, "p99_ns": 30, "max_ns": 40},
            {"p50_ns": 5, "p95_ns": 6, "p99_ns": 7, "max_ns": 8},
            {"p50_ns": 9, "p95_ns": 10, "p99_ns": 11, "max_ns": 12},
        ]
        self.assertEqual(BENCH.median_summary(summaries), {
            "p50_ns": 5, "p95_ns": 10, "p99_ns": 11, "max_ns": 12,
        })


class WorkerProtocolTests(unittest.TestCase):
    def run_source(self, source, timeout=3):
        with tempfile.TemporaryDirectory() as directory:
            worker = Path(directory) / "worker.py"
            worker.write_text(f"#!{sys.executable}\n" + source, encoding="utf-8")
            worker.chmod(0o755)
            return BENCH.run_worker(worker, "cold", "none", 0, 1, timeout, os.environ.copy())

    def result_source(self, **changes):
        payload = {"mode": "cold", "vfs": "none", "warmups": 0, "samples_ns": [123],
                   "measurement": "setup to marker", "module_prepared_before_measurement": True,
                   "program_marker_first_instruction": True}
        payload.update(changes)
        return ('print(\'{"event":"ready"}\', flush=True)\n'
                + f"print({json.dumps(json.dumps(payload))}, flush=True)\n")

    def test_worker_requires_ready_then_matching_result(self):
        run = self.run_source(self.result_source())
        self.assertGreater(run["boot_ns"], 0)
        self.assertEqual(run["result"]["samples_ns"], [123])

    def test_rejects_missing_ready_mismatched_counts_and_false_contract(self):
        for source in (
            "print('{}')", self.result_source(samples_ns=[]), self.result_source(warmups=1),
            self.result_source(vfs="ephemeral"), self.result_source(program_marker_first_instruction=False),
            self.result_source().replace("flush=True)\n", "flush=True)\nprint('null')\n", 1),
            self.result_source() + "print('{}')\n",
        ):
            with self.subTest(source=source), self.assertRaises(BENCH.BenchmarkFailure):
                self.run_source(source)

    def test_timeout_kills_child(self):
        started = time.monotonic()
        with self.assertRaisesRegex(BENCH.BenchmarkFailure, "timed out"):
            self.run_source("import time\ntime.sleep(30)\n", timeout=0.1)
        self.assertLess(time.monotonic() - started, 5)

    def test_large_stderr_does_not_deadlock(self):
        with self.assertRaisesRegex(BENCH.BenchmarkFailure, "worker failed"):
            self.run_source("import sys\nsys.stderr.write('error' * 20000)\nsys.exit(1)\n")


if __name__ == "__main__":
    unittest.main()
