"""Exercise CI selection against real Git trees, including deletion and renames."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "ci_changes.sh"


class ChangeSelectionTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.git("init", "--quiet")
        self.git("config", "user.name", "CI test")
        self.git("config", "user.email", "ci@example.test")
        self.git("commit", "--quiet", "--allow-empty", "-m", "base")
        self.base = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.run(
            ["git", *args], cwd=self.root, check=True,
            capture_output=True, text=True,
        ).stdout

    def commit_file(self, path):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text("fixture\n")
        self.git("add", "--", path)
        self.git("commit", "--quiet", "-m", "change")

    def selected(self, target, base=None):
        output = self.root / "step-output"
        output.unlink(missing_ok=True)
        subprocess.run(
            ["bash", str(SCRIPT), target], cwd=self.root, check=True,
            env={**os.environ, "BASE_SHA": self.base if base is None else base,
                 "GITHUB_OUTPUT": str(output)},
            capture_output=True, text=True,
        )
        return output.read_text().strip() == "run=true"

    def test_package_only_change_skips_rust_but_runs_packages(self):
        self.commit_file("packages/x/src/lib.ts")
        self.assertFalse(self.selected("rust"))
        self.assertTrue(self.selected("packages"))

    def test_package_manifest_selects_packages_without_rust(self):
        self.commit_file("submilli.toml")
        self.assertFalse(self.selected("rust"))
        self.assertTrue(self.selected("packages"))

    def test_package_lockfile_selects_packages_without_rust(self):
        self.commit_file("submilli.lock")
        self.assertFalse(self.selected("rust"))
        self.assertTrue(self.selected("packages"))

    def test_unrelated_documentation_skips_both(self):
        self.commit_file("docs/part-2-blueprints/guide.md")
        self.assertFalse(self.selected("rust"))
        self.assertFalse(self.selected("packages"))

    def test_rust_and_its_build_test_inputs_select_both(self):
        for path in (
            "crates/interpreter/src/lib.rs", "helper.rs", "Cargo.lock",
            "crates/interpreter/tests/fixtures/example.ts", ".cargo/config.toml",
            "rust-toolchain.toml", "clippy.toml", ".gitattributes", "skills/submilli/SKILL.md",
            "examples/quickstart/verify.sh", "scripts/ci_changes.sh",
            "docs/part-1-start-here/03-quickstart.md", ".github/workflows/ci.yml",
        ):
            with self.subTest(path=path):
                self.base = self.git("rev-parse", "HEAD").strip()
                self.commit_file(path)
                self.assertTrue(self.selected("rust"))
                self.assertTrue(self.selected("packages"))

    def test_package_workflow_change_selects_itself(self):
        self.commit_file(".github/workflows/packages.yml")
        self.assertFalse(self.selected("rust"))
        self.assertTrue(self.selected("packages"))

    def test_deleting_a_rust_input_still_selects_rust(self):
        path = "crates/interpreter/tests/fixture.ts"
        self.commit_file(path)
        self.base = self.git("rev-parse", "HEAD").strip()
        self.git("rm", "--", path)
        self.git("commit", "--quiet", "-m", "delete")
        self.assertTrue(self.selected("rust"))

    def test_rename_out_of_rust_inputs_still_selects_rust(self):
        self.commit_file("crates/interpreter/tests/fixture.ts")
        self.base = self.git("rev-parse", "HEAD").strip()
        (self.root / "packages").mkdir()
        self.git("mv", "crates/interpreter/tests/fixture.ts", "packages/fixture.ts")
        self.git("commit", "--quiet", "-m", "rename")
        self.assertTrue(self.selected("rust"))
        self.assertTrue(self.selected("packages"))

    def test_missing_and_unavailable_bases_run_instead_of_skipping(self):
        for base in ("", "0" * 40, "1" * 40):
            with self.subTest(base=base):
                self.assertTrue(self.selected("rust", base))
                self.assertTrue(self.selected("packages", base))

    def test_shallow_checkout_fetches_the_comparison_commit(self):
        self.commit_file("packages/x/src/lib.ts")
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        clone = Path(directory.name) / "checkout"
        subprocess.run(
            ["git", "clone", "--quiet", "--depth=1", self.root.as_uri(), str(clone)],
            check=True, capture_output=True, text=True,
        )
        self.root = clone
        missing = subprocess.run(
            ["git", "cat-file", "-e", self.base], cwd=clone, capture_output=True,
        )
        self.assertNotEqual(missing.returncode, 0)
        self.assertFalse(self.selected("rust"))
        self.assertTrue(self.selected("packages"))

    def test_unchanged_tree_skips_both(self):
        self.assertFalse(self.selected("rust"))
        self.assertFalse(self.selected("packages"))


if __name__ == "__main__":
    unittest.main()
