"""Keep release preparation deferrable and published Compose checks mandatory."""

from contextlib import redirect_stdout
import importlib.util
import io
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.error


SCRIPT = Path(__file__).resolve().parents[1] / "check_compose_release.py"
sys.path.insert(0, str(SCRIPT.parent))
SPEC = importlib.util.spec_from_file_location("check_compose_release", SCRIPT)
checker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checker)

MANIFEST = '[workspace.package]\nversion = "0.6.2"\n\n[workspace.dependencies]\n'


def guide(version="0.6.2", image_version=None):
    image_version = image_version or version
    return (
        "## Start it\n\n```sh\n"
        "curl -fsSLO https://raw.githubusercontent.com/submilli/submilli-runtime/"
        f"v{version}/compose.yaml\n"
        f"printf 'SUBMILLI_IMAGE=ghcr.io/submilli/submilli-runtime:{image_version}\\n' > .env\n"
        "```\n\n## Upgrade\n"
        "curl -fsSLO https://raw.githubusercontent.com/submilli/submilli-runtime/v0.5.1/compose.yaml\n"
    )


class TargetTests(unittest.TestCase):
    def test_only_start_commands_supply_the_pins(self):
        self.assertEqual(checker.compose_target(guide()),
                         ("v0.6.2", "ghcr.io/submilli/submilli-runtime:0.6.2"))

    def test_missing_or_inconsistent_pins_fail(self):
        for document in ["", "## Start it\n", guide().replace("printf", "echo"),
                         guide(image_version="0.6.1"), guide("not-a-version")]:
            with self.subTest(document=document), self.assertRaises(ValueError):
                checker.compose_target(document)

    def test_workspace_version_is_scoped_to_workspace_package(self):
        manifest = '[package]\nversion = "9.9.9"\n' + MANIFEST
        self.assertEqual(checker.workspace_version(manifest), "0.6.2")
        with self.assertRaises(ValueError):
            checker.workspace_version('[workspace.dependencies]\nversion = "9.9.9"\n')


class ReadinessTests(unittest.TestCase):
    def setUp(self):
        self.registry = patch.object(checker, "Registry").start()
        self.addCleanup(patch.stopall)
        self.exists = self.registry.return_value.manifest_exists

    def test_published_images_run_in_documentation_and_release_modes(self):
        self.exists.return_value = True
        for version, required in [("0.6.1", False), ("0.6.2", False), ("0.6.2", True)]:
            with self.subTest(version=version, required=required):
                self.assertTrue(checker.image_ready(version, "0.6.2", required))
                self.exists.assert_called_with("submilli/submilli-runtime", version)
        self.assertTrue(all(call.args == () for call in self.registry.call_args_list))

    def test_only_the_unpublished_workspace_release_can_defer(self):
        self.exists.return_value = False
        self.assertFalse(checker.image_ready("0.6.2", "0.6.2"))
        for version in ["0.6.1", "0.6.3", "0.6.22"]:
            with self.subTest(version=version), self.assertRaisesRegex(RuntimeError, "not been published"):
                checker.image_ready(version, "0.6.2")

    def test_release_mode_never_defers(self):
        self.exists.return_value = False
        with self.assertRaisesRegex(RuntimeError, "not been published"):
            checker.image_ready("0.6.2", "0.6.2", require_published=True)

    def test_registry_and_transport_failures_are_not_absent_images(self):
        for error in [RuntimeError("HTTP 401"), RuntimeError("HTTP 403"),
                      RuntimeError("HTTP 429"), RuntimeError("HTTP 503"),
                      urllib.error.URLError("connection lost")]:
            with self.subTest(error=error):
                self.exists.side_effect = error
                with self.assertRaises(type(error)):
                    checker.image_ready("0.6.2", "0.6.2")


class WorkflowOutputTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.output = Path(self.directory.name) / "output"
        self.summary = Path(self.directory.name) / "summary"
        self.root = Path(self.directory.name) / "source"
        self.write_source(self.root, "0.6.2")
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, {"GITHUB_OUTPUT": str(self.output),
                                "GITHUB_STEP_SUMMARY": str(self.summary)}, clear=True).start()
        patch.object(checker, "ROOT", self.root).start()
        self.registry = patch.object(checker, "Registry").start()

    def write_source(self, root, version):
        document = root / "docs/part-4-server/06-deploy-with-compose.md"
        document.parent.mkdir(parents=True)
        document.write_text(guide(version))
        (root / "Cargo.toml").write_text(MANIFEST.replace("0.6.2", version))

    def test_deferred_journey_is_explicit_and_does_not_run(self):
        self.registry.return_value.manifest_exists.return_value = False
        with redirect_stdout(io.StringIO()) as output:
            checker.main([])
        self.assertEqual(self.output.read_text(), "available=false\n")
        self.assertIn("deferred", self.summary.read_text())
        self.assertIn("::notice::", output.getvalue())

    def test_published_image_enables_the_journey(self):
        self.registry.return_value.manifest_exists.return_value = True
        with redirect_stdout(io.StringIO()):
            checker.main([])
        self.assertEqual(self.output.read_text(), "available=true\n")
        self.assertFalse(self.summary.exists())

    def test_release_tag_mismatch_fails_before_registry_access(self):
        with self.assertRaisesRegex(ValueError, "match the runtime release tag"):
            checker.main(["--require-published", "--runtime-tag", "v0.6.3"])
        self.registry.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_required_release_image_cannot_produce_a_deferred_success(self):
        self.registry.return_value.manifest_exists.return_value = False
        with self.assertRaisesRegex(RuntimeError, "not been published"):
            checker.main(["--require-published", "--runtime-tag", "v0.6.2"])
        self.assertFalse(self.output.exists())
        self.assertFalse(self.summary.exists())

    def test_target_extraction_needs_no_registry_and_sets_no_step_output(self):
        with redirect_stdout(io.StringIO()) as output:
            checker.main(["--print-target"])
        self.assertEqual(output.getvalue(),
                         "v0.6.2 ghcr.io/submilli/submilli-runtime:0.6.2\n")
        self.registry.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_historical_source_needs_no_verification_tools_of_its_own(self):
        source = Path(self.directory.name) / "old-release"
        self.write_source(source, "0.6.1")
        self.registry.return_value.manifest_exists.return_value = True
        with redirect_stdout(io.StringIO()):
            checker.main(["--source-root", str(source), "--require-published",
                          "--runtime-tag", "v0.6.1"])
        self.registry.return_value.manifest_exists.assert_called_once_with(
            "submilli/submilli-runtime", "0.6.1")
        self.assertEqual(self.output.read_text(), "available=true\n")

    def test_deferral_uses_the_selected_sources_workspace_version(self):
        source = Path(self.directory.name) / "next-release"
        self.write_source(source, "0.6.3")
        self.registry.return_value.manifest_exists.return_value = False
        with redirect_stdout(io.StringIO()):
            checker.main(["--source-root", str(source)])
        self.assertEqual(self.output.read_text(), "available=false\n")


if __name__ == "__main__":
    unittest.main()
