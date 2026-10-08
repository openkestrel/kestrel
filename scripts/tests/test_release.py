import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
CLI = ROOT / "scripts/release.py"


class ReleaseTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for path in ["Cargo.toml", "Cargo.lock", "package.json", "packages/client/package.json"]:
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / path, target)
        for manifest in (ROOT / "crates").glob("*/Cargo.toml"):
            target = self.root / manifest.relative_to(ROOT)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(manifest, target)
        self.notes = self.root / "notes.md"
        self.notes.write_text("### Behavior changes\n\n- Guided first start.\n\n### Fixes\n\n- Clear startup errors.\n\n### Known limitations\n\n- No Windows distribution.\n")
        subprocess.run(["git", "init", "-q", self.root], check=True)

    def run_cli(self, *args):
        return subprocess.run(["python3", CLI, "--root", self.root, *args], capture_output=True, text=True, env={**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null"})

    def prepare(self):
        result = self.run_cli("prepare", "0.4.1", "--notes", str(self.notes))
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_prepare_updates_reviewable_files_without_tagging_or_publishing(self):
        self.prepare()
        self.assertIn('version = "0.4.1"', (self.root / "Cargo.toml").read_text())
        self.assertEqual(json.loads((self.root / "packages/client/public/version.json").read_text()), {"version": "0.4.1"})
        self.assertIn("## [0.4.1]", (self.root / "CHANGELOG.md").read_text())
        self.assertIn("No Windows distribution.", (self.root / "CHANGELOG.md").read_text())
        self.assertEqual(subprocess.check_output(["git", "-C", self.root, "tag", "--list"], text=True), "")
        self.assertFalse((self.root / "dist").exists())
        self.assertEqual(self.run_cli("validate", "v0.4.1").returncode, 0)

    def test_existing_tag_cannot_be_prepared_again(self):
        subprocess.run(["git", "-C", self.root, "-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", "baseline"], check=True)
        subprocess.run(["git", "-C", self.root, "-c", "tag.gpgsign=false", "tag", "v0.4.1"], check=True)
        before = (self.root / "Cargo.toml").read_bytes()
        result = self.run_cli("prepare", "0.4.1", "--notes", str(self.notes))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.root / "Cargo.toml").read_bytes(), before)

    def test_validator_rejects_each_disagreement(self):
        self.prepare()
        cases = [
            ("Cargo.toml", 'version = "0.4.1"', 'version = "0.4.2"'),
            ("Cargo.lock", 'version = "0.4.1"', 'version = "0.4.2"'),
            ("crates/kestrel/Cargo.toml", "version.workspace = true", 'version = "0.4.1"'),
            ("packages/client/public/version.json", "0.4.1", "0.4.2"),
            ("packages/client/package.json", '"private": true,', '"private": true, "version": "0.4.1",'),
            ("CHANGELOG.md", "[0.4.1]", "[0.4.2]"),
            ("CHANGELOG.md", "### Known limitations", "### Other"),
        ]
        for path, old, new in cases:
            with self.subTest(path=path, old=old):
                target = self.root / path
                original = target.read_text()
                self.assertIn(old, original)
                target.write_text(original.replace(old, new))
                result = self.run_cli("validate", "v0.4.1")
                self.assertNotEqual(result.returncode, 0, result.stdout)
                target.write_text(original)
        for tag in ["v0.4.2", "0.4.1", "v0.0.0", "v01.4.1"]:
            with self.subTest(tag=tag):
                self.assertNotEqual(self.run_cli("validate", tag).returncode, 0)

    def test_validator_checks_built_browser_metadata(self):
        self.prepare()
        build = self.root / "built-client"
        build.mkdir()
        self.assertNotEqual(self.run_cli("validate", "v0.4.1", "--browser-build", str(build)).returncode, 0)
        (build / "version.json").write_text('{"version":"0.4.2"}')
        self.assertNotEqual(self.run_cli("validate", "v0.4.1", "--browser-build", str(build)).returncode, 0)
        (build / "version.json").write_text('{"version":"0.4.1"}')
        self.assertEqual(self.run_cli("validate", "v0.4.1", "--browser-build", str(build)).returncode, 0)

    def test_invalid_preparation_changes_nothing(self):
        before = (self.root / "Cargo.toml").read_bytes()
        for version in ["0.0.0", "v0.4.1", "0.4", "0.4.1-pre"]:
            self.assertNotEqual(self.run_cli("prepare", version, "--notes", str(self.notes)).returncode, 0)
        self.notes.write_text("### Behavior changes\n\n- Something.\n")
        self.assertNotEqual(self.run_cli("prepare", "0.4.1", "--notes", str(self.notes)).returncode, 0)
        self.assertEqual((self.root / "Cargo.toml").read_bytes(), before)
        self.assertFalse((self.root / "CHANGELOG.md").exists())

    def test_version_tag_reuse_for_changed_content_is_rejected(self):
        self.prepare()
        git = ["git", "-C", self.root, "-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"]
        subprocess.run([*git, "add", "."], check=True)
        subprocess.run([*git, "commit", "-qm", "candidate"], check=True)
        subprocess.run([*git, "tag", "-a", "v0.4.1", "-m", "candidate"], check=True)
        self.assertEqual(self.run_cli("validate", "v0.4.1").returncode, 0)
        (self.root / "CHANGELOG.md").write_text((self.root / "CHANGELOG.md").read_text() + "\nChanged content.\n")
        self.assertNotEqual(self.run_cli("validate", "v0.4.1").returncode, 0)
        subprocess.run([*git, "commit", "-qam", "changed"], check=True)
        self.assertNotEqual(self.run_cli("validate", "v0.4.1").returncode, 0)

    def test_broken_lockfile_is_refused_before_preparation_writes(self):
        target = self.root / "Cargo.lock"
        target.write_text(target.read_text().replace('name = "kestrel"', 'name = "missing-kestrel"'))
        before = (self.root / "Cargo.toml").read_bytes()
        self.assertNotEqual(self.run_cli("prepare", "0.4.1", "--notes", str(self.notes)).returncode, 0)
        self.assertEqual((self.root / "Cargo.toml").read_bytes(), before)
