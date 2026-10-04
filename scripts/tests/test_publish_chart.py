"""Exercise publication retries and registry failures without accessing GHCR."""

import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import urllib.error


SCRIPT = Path(__file__).resolve().parents[1] / "publish_chart.py"
SPEC = importlib.util.spec_from_file_location("publish_chart", SCRIPT)
publisher = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(publisher)


class RuntimeVersionTests(unittest.TestCase):
    def test_runtime_release_rejects_an_old_app_version_before_registry_access(self):
        metadata = {"version": "0.3.4", "appVersion": "0.2.0"}
        with patch("sys.argv", ["publish_chart.py", "--runtime-tag", "v0.2.1"]), \
                patch.object(publisher, "chart_metadata", return_value=metadata), \
                patch.object(publisher.Registry, "manifest_exists") as exists, \
                patch.object(publisher, "publish") as publish:
            with self.assertRaisesRegex(ValueError, "appVersion must match"):
                publisher.main()
            exists.assert_not_called()
            publish.assert_not_called()

    def test_runtime_and_standalone_releases_accept_the_selected_image(self):
        metadata = {"version": "0.3.4", "appVersion": "0.2.0"}
        for arguments in [[], ["--runtime-tag", "v0.2.0"]]:
            with self.subTest(arguments=arguments), \
                    patch("sys.argv", ["publish_chart.py"] + arguments), \
                    patch.object(publisher, "chart_metadata", return_value=metadata), \
                    patch.object(publisher.Registry, "manifest_exists", return_value=True), \
                    patch.object(publisher, "publish") as publish:
                publisher.main()
                publish.assert_called_once_with(Path("charts/submilli"), "0.3.4")


class PublishTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.archive = self.root / "expected.tgz"
        write_archive(self.archive, [("submilli/Chart.yaml", b"version: 0.3.4\n")])
        self.remote = self.archive.read_bytes()
        self.commands = []
        self.public = True
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, {"GITHUB_ACTOR": "test-actor", "GH_TOKEN": "test-credential"},
                   clear=True).start()
        patch.object(publisher.subprocess, "run", side_effect=self.run_helm).start()
        self.exists = patch.object(publisher.Registry, "manifest_exists", return_value=False).start()

    def run_helm(self, command, **options):
        self.commands.append(command)
        operation = command[1]
        if operation == "package":
            destination = Path(command[command.index("--destination") + 1])
            (destination / "submilli-0.3.4.tgz").write_bytes(self.archive.read_bytes())
        elif operation == "pull":
            anonymous = "anonymous" in options["env"]["HELM_REGISTRY_CONFIG"]
            if anonymous and not self.public:
                raise subprocess.CalledProcessError(1, command)
            destination = Path(command[command.index("--destination") + 1])
            (destination / "submilli-0.3.4.tgz").write_bytes(self.remote)
        return subprocess.CompletedProcess(command, 0)

    def test_new_version_is_pushed_then_verified_with_and_without_credentials(self):
        output = self.root / "output"
        with patch.dict(os.environ, GITHUB_OUTPUT=str(output)):
            publisher.publish(Path("charts/submilli"), "0.3.4")
        self.assertEqual([command[1] for command in self.commands],
                         ["registry", "package", "push", "pull", "pull"])
        self.assertEqual(self.commands[2][-1], "oci://ghcr.io/submilli/charts")
        self.assertEqual(output.read_text(), "version=0.3.4\n")
        self.assertNotIn("test-credential", repr(self.commands))

    def test_identical_retry_never_pushes(self):
        self.exists.return_value = True
        publisher.publish(Path("charts/submilli"), "0.3.4")
        self.assertNotIn("push", [command[1] for command in self.commands])

    def test_conflicting_version_is_rejected_without_push(self):
        self.exists.return_value = True
        changed = self.root / "changed.tgz"
        write_archive(changed, [("submilli/Chart.yaml", b"changed")])
        self.remote = changed.read_bytes()
        with self.assertRaisesRegex(RuntimeError, "bump the chart version"):
            publisher.publish(Path("charts/submilli"), "0.3.4")
        self.assertNotIn("push", [command[1] for command in self.commands])

    def test_private_first_upload_can_resume_after_visibility_changes(self):
        self.public = False
        with self.assertRaisesRegex(RuntimeError, "Make the charts/submilli"):
            publisher.publish(Path("charts/submilli"), "0.3.4")
        self.assertEqual([c[1] for c in self.commands].count("push"), 1)
        self.commands.clear()
        self.public = True
        self.exists.return_value = True
        publisher.publish(Path("charts/submilli"), "0.3.4")
        self.assertNotIn("push", [command[1] for command in self.commands])

    def test_registry_failure_does_not_push(self):
        self.exists.side_effect = RuntimeError("HTTP 403")
        with self.assertRaisesRegex(RuntimeError, "403"):
            publisher.publish(Path("charts/submilli"), "0.3.4")
        self.assertNotIn("push", [command[1] for command in self.commands])


class RegistryTests(unittest.TestCase):
    def test_only_explicit_missing_manifest_or_repository_is_absent(self):
        for code in ["MANIFEST_UNKNOWN", "NAME_UNKNOWN"]:
            with self.subTest(code=code), patch.object(publisher.urllib.request, "urlopen",
                                                     side_effect=http_error(404, code)):
                self.assertIsNone(publisher.request_json("https://ghcr.io/test", {}, True))

    def test_permission_service_and_unrecognized_not_found_errors_stop_publication(self):
        for status, code in [(401, "UNAUTHORIZED"), (403, "DENIED"),
                             (429, "TOOMANYREQUESTS"), (500, "UNKNOWN"), (404, "UNKNOWN")]:
            with self.subTest(status=status), patch.object(publisher.urllib.request, "urlopen",
                                                          side_effect=http_error(status, code)):
                with self.assertRaisesRegex(RuntimeError, "publication stopped"):
                    publisher.request_json("https://ghcr.io/test", {}, True)

    def test_token_endpoint_not_found_does_not_allow_a_push(self):
        with patch.object(publisher.urllib.request, "urlopen",
                          side_effect=http_error(404, "NAME_UNKNOWN")):
            with self.assertRaisesRegex(RuntimeError, "publication stopped"):
                publisher.Registry("actor", "secret").manifest_exists("submilli/chart", "0.3.4")

    def test_anonymous_probe_and_helm_build_metadata_tag(self):
        with patch.object(publisher, "request_json", side_effect=[{"token": "bearer"}, {}]) as request:
            self.assertTrue(publisher.Registry().manifest_exists("submilli/chart", "1.2.3+build"))
        token_call, manifest_call = request.call_args_list
        self.assertEqual(token_call.args[1], {})
        self.assertIn("%3Apull", token_call.args[0])
        self.assertTrue(manifest_call.args[0].endswith("/1.2.3_build"))


class ArchiveTests(unittest.TestCase):
    def test_order_and_timestamps_do_not_change_content_but_modes_and_files_do(self):
        with tempfile.TemporaryDirectory() as directory:
            first, second = Path(directory) / "a.tgz", Path(directory) / "b.tgz"
            entries = [("submilli/a", b"a"), ("submilli/b", b"b")]
            write_archive(first, entries, time=1)
            write_archive(second, list(reversed(entries)), time=2)
            self.assertEqual(publisher.archive_contents(first), publisher.archive_contents(second))
            write_archive(second, entries, mode=0o755)
            self.assertNotEqual(publisher.archive_contents(first), publisher.archive_contents(second))
            write_archive(second, entries[:1])
            self.assertNotEqual(publisher.archive_contents(first), publisher.archive_contents(second))

    def test_duplicate_members_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "a.tgz"
            write_archive(archive, [("submilli/a", b"a"), ("submilli/a", b"b")])
            with self.assertRaisesRegex(ValueError, "duplicate"):
                publisher.archive_contents(archive)

    def test_metadata_is_validated_before_registry_use(self):
        for metadata in ["name: other\nversion: 1.2.3\nappVersion: 0.2.0\n",
                         "name: submilli\nversion: ../other\nappVersion: 0.2.0\n",
                         "name: submilli\nversion: 1.2.3\n"]:
            with self.subTest(metadata=metadata), patch.object(publisher.subprocess, "check_output",
                                                             return_value=metadata):
                with self.assertRaises(ValueError):
                    publisher.chart_metadata(Path("chart"))


def write_archive(path, entries, time=0, mode=0o644):
    with tarfile.open(path, "w:gz") as archive:
        for name, content in entries:
            entry = tarfile.TarInfo(name)
            entry.size = len(content)
            entry.mtime = time
            entry.mode = mode
            archive.addfile(entry, io.BytesIO(content))


def http_error(status, code):
    body = json.dumps({"errors": [{"code": code}]}).encode()
    return urllib.error.HTTPError("https://ghcr.io/test", status, "test response", {}, io.BytesIO(body))


if __name__ == "__main__":
    unittest.main()
