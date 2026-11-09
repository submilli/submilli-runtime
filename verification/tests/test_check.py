"""Protect the checker gate against false passes, especially expected failures."""

from contextlib import ExitStack
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import urllib.error


path = Path(__file__).resolve().parents[1] / "check.py"
spec = importlib.util.spec_from_file_location("model_checks", path)
checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checks)


class ResultTests(unittest.TestCase):
    def test_negative_requires_the_named_invariant_and_a_trace(self):
        case = {"kind": "negative", "invariant": "NoStaleWrites"}
        trace = "Invariant NoStaleWrites is violated.\nState 1: <Initial predicate>"
        self.assertTrue(checks.assess_result(case, 12, trace)[0])
        for code, output in [
            (150, "Parsing or semantic analysis failed."),
            (12, trace.replace("NoStaleWrites", "TypeOK")),
            (0, trace),
            (12, "Invariant NoStaleWrites is violated."),
        ]:
            with self.subTest(code=code, output=output):
                self.assertFalse(checks.assess_result(case, code, output)[0])

    def test_positive_requires_exhausted_state_space(self):
        case = {"kind": "safety"}
        complete = checks.COMPLETE + "\n100 states generated, 20 distinct states found, 0 states left on queue."
        self.assertTrue(checks.assess_result(case, 0, complete)[0])
        self.assertFalse(checks.assess_result(case, 0, checks.COMPLETE)[0])
        self.assertFalse(checks.assess_result(case, 0, complete, timed_out=True)[0])
        self.assertFalse(checks.assess_result(case, 255, complete)[0])
        self.assertFalse(checks.assess_result(
            case, 0, complete.replace("0 states left", "10 states left")
        )[0])
        self.assertFalse(checks.assess_result(
            case, 0, complete + "\n101 states generated, 21 distinct states found, 1 states left on queue."
        )[0])

    def test_liveness_cannot_pass_with_only_safety_evidence(self):
        case = {"kind": "liveness"}
        output = checks.COMPLETE + "\n100 states generated, 20 distinct states found, 0 states left on queue."
        self.assertFalse(checks.assess_result(case, 0, output)[0])
        self.assertTrue(checks.assess_result(
            case, 0, output + "\nFinished checking temporal properties"
        )[0])

    def test_witness_is_a_counterexample_to_unreachability(self):
        case = {"kind": "witness", "invariant": "NeverWitness"}
        self.assertFalse(checks.assess_result(case, 0, checks.COMPLETE)[0])
        self.assertTrue(checks.assess_result(
            case, 12, "Invariant NeverWitness is violated.\nState 1:"
        )[0])


class DownloadTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.destination = self.directory / "tool.jar"
        contexts = ExitStack()
        self.addCleanup(contexts.close)
        self.urlopen = contexts.enter_context(mock.patch.object(checks.urllib.request, "urlopen"))
        self.sleep = contexts.enter_context(mock.patch.object(checks.time, "sleep"))
        self.stderr = contexts.enter_context(mock.patch.object(checks.sys, "stderr", io.StringIO()))

    def http_error(self, code):
        return urllib.error.HTTPError("https://example.test/tool.jar", code, "failure", {}, io.BytesIO())

    def test_transient_http_errors_retry_with_longer_timeout(self):
        for code in checks.RETRYABLE_HTTP_STATUS:
            with self.subTest(code=code):
                self.urlopen.reset_mock()
                self.sleep.reset_mock()
                self.urlopen.side_effect = [self.http_error(code), io.BytesIO(b"complete")]
                checks.download_tool("https://example.test/tool.jar", self.destination)
                self.assertEqual(self.destination.read_bytes(), b"complete")
                self.assertEqual(self.urlopen.call_count, 2)
                self.assertEqual(self.urlopen.call_args.kwargs["timeout"], 180)
                self.sleep.assert_called_once_with(2)
        self.assertIn("retrying in 2s", self.stderr.getvalue())

    def test_retry_limit_and_backoff(self):
        for error in [self.http_error(504), urllib.error.URLError("connection reset"),
                      TimeoutError("timed out"), checks.socket.timeout("socket timed out"), ConnectionResetError("reset"),
                      checks.http.client.IncompleteRead(b"partial")]:
            with self.subTest(error=type(error).__name__):
                self.urlopen.reset_mock()
                self.sleep.reset_mock()
                self.urlopen.side_effect = error
                with self.assertRaises(type(error)):
                    checks.download_tool("https://example.test/tool.jar", self.destination)
                self.assertEqual(self.urlopen.call_count, 3)
                self.assertEqual(self.sleep.call_args_list, [mock.call(2), mock.call(4)])

    def test_permanent_http_error_is_not_retried(self):
        self.urlopen.side_effect = self.http_error(404)
        with self.assertRaises(urllib.error.HTTPError):
            checks.download_tool("https://example.test/tool.jar", self.destination)
        self.assertEqual(self.urlopen.call_count, 1)
        self.sleep.assert_not_called()

    def test_partial_body_is_replaced_on_retry(self):
        response = mock.MagicMock()
        response.__enter__.return_value = response
        response.read.side_effect = [b"partial body longer than success", checks.socket.timeout("read timed out")]
        self.urlopen.side_effect = [response, io.BytesIO(b"ok")]
        checks.download_tool("https://example.test/tool.jar", self.destination)
        self.assertEqual(self.destination.read_bytes(), b"ok")
        response.__exit__.assert_called_once()

    def test_local_write_error_is_not_retried(self):
        self.urlopen.return_value = io.BytesIO(b"ok")
        with mock.patch.object(Path, "open", side_effect=PermissionError("read only")):
            with self.assertRaises(PermissionError):
                checks.download_tool("https://example.test/tool.jar", self.destination)
        self.assertEqual(self.urlopen.call_count, 1)
        self.sleep.assert_not_called()

    def test_only_verified_download_is_cached(self):
        pin = {"release": "test", "url": "https://example.test/tool.jar",
               "sha256": hashlib.sha256(b"verified").hexdigest()}
        (self.directory / "toolchain.json").write_text(json.dumps(pin))
        with mock.patch.object(checks, "ROOT", self.directory), \
                mock.patch.object(checks.tempfile, "gettempdir", return_value=str(self.directory)):
            self.urlopen.return_value = io.BytesIO(b"wrong")
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                checks.tool_jar(None)
            cache = self.directory / "submilli-tla-tools"
            self.assertEqual(list(cache.iterdir()), [])
            self.sleep.assert_not_called()
            self.assertEqual(self.urlopen.call_count, 1)
            self.urlopen.side_effect = [self.http_error(504), io.BytesIO(b"verified")]
            jar = checks.tool_jar(None)
            self.assertEqual(jar.read_bytes(), b"verified")
            self.urlopen.reset_mock()
            self.assertEqual(checks.tool_jar(None), jar)
            self.urlopen.assert_not_called()


class ConfigurationTests(unittest.TestCase):
    def test_editor_configuration_matches_the_checked_baseline(self):
        directory = checks.ROOT / "durability"
        manifest = json.loads((directory / "checks.json").read_text())
        case = next(case for case in manifest["cases"] if case["name"] == "safety")
        self.assertEqual(
            (directory / "safety.cfg").read_text(), checks.render_config(manifest, case)
        )

    def test_unknown_case_or_constant_cannot_silently_skip_coverage(self):
        manifest = {"cases": [{"name": "safety", "kind": "safety"}]}
        with self.assertRaises(ValueError):
            checks.select_cases(manifest, ["saftey"], "ci")
        with self.assertRaises(ValueError):
            checks.render_config(
                {"constants": {"MaxRuns": 1}, "invariants": []},
                {"kind": "safety", "constants": {"MaxRun": 2}},
            )

    def test_config_preserves_fault_strings_and_model_values(self):
        manifest = {
            "constants": {"Fault": "none", "Lifecycle": False,
                          "NoWrite": {"model_value": "NoWrite"}},
            "invariants": ["TypeOK"], "properties": ["AppendOnly"],
        }
        config = checks.render_config(manifest, {
            "kind": "negative", "invariant": "NoStaleWrites",
            "constants": {"Fault": "stale-write", "Lifecycle": True},
        })
        self.assertIn('Fault = "stale-write"', config)
        self.assertIn("Lifecycle = TRUE", config)
        self.assertIn("NoWrite = NoWrite", config)
        self.assertIn("INVARIANT NoStaleWrites", config)
        self.assertNotIn("PROPERTY", config)

    def test_wrong_jar_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            jar = Path(temporary) / "tool.jar"
            jar.write_bytes(b"wrong or incomplete download")
            with self.assertRaises(ValueError):
                checks.check_digest(jar, "0" * 64)

    def test_evidence_cannot_overwrite_an_existing_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "existing").write_text("preserve")
            with self.assertRaises(ValueError):
                checks.output_directory(directory)
            self.assertEqual((directory / "existing").read_text(), "preserve")


if __name__ == "__main__":
    unittest.main()
