import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from release import CLI_FILES, bump, finalize_changelog, metadata, package, prepare_changelog


class ReleaseTest(unittest.TestCase):
    def test_versions_and_next_minor_bump(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "Cargo.toml").write_text('[package]\nname = "matchbox"\nversion = "0.11.0"\n')
            (root / "Cargo.lock").write_text('[[package]]\nname = "matchbox"\nversion = "0.11.0"\n\n[[package]]\nname = "other"\nversion = "0.11.0"\n')
            snapshot = metadata(root, True, "123", "a" * 40)
            self.assertEqual(snapshot["build_version"], "0.11.0-snapshot+123")
            self.assertEqual(snapshot["tag"], "snapshot")
            stable = metadata(root, False, "124", "b" * 40)
            self.assertEqual(stable["tag"], "v0.11.0")
            self.assertEqual(stable["channel"], "latest")
            with self.assertRaises(ValueError):
                bump(root, "0.10.2")
            self.assertEqual(bump(root, "0.11.0"), "0.12.0")
            self.assertIn('name = "other"\nversion = "0.11.0"', (root / "Cargo.lock").read_text())
            self.assertEqual(metadata(root, True, "125", "a" * 40)["version"], "0.12.0-snapshot")

    def test_changelog_preserves_history_and_opens_next_unreleased_section(self):
        text = "# Changelog\n\n## [Unreleased]\n\n### Changed\n- Release automation.\n\n## [0.10.2] - 2026-01-01\n\n- Previous release.\n"
        finalized, notes = finalize_changelog(text, "0.11.0", "2026-02-01")
        self.assertEqual(notes, "### Changed\n- Release automation.")
        self.assertIn("## [Unreleased]\n\n## [0.11.0] - 2026-02-01", finalized)
        self.assertTrue(finalized.endswith(text[text.index("## [0.10.2]"):]))
        with self.assertRaises(ValueError):
            finalize_changelog(finalized, "0.11.0", "2026-02-01")
        with self.assertRaises(ValueError):
            finalize_changelog("# No unreleased section", "0.11.0", "2026-02-01")

    def test_preparation_keeps_new_develop_notes_unreleased(self):
        original = "# Changelog\n\n## [Unreleased]\n\n### Changed\n- Existing change.\n\n## [0.10.2] - 2026-01-01\n\n- Previous release.\n"
        released, _ = finalize_changelog(original, "0.11.0", "2026-02-01")
        development = original.replace("- Existing change.", "- Existing change.\n- New development work.\n\n### Added\n- Another feature.")
        prepared = prepare_changelog(development, original, released, "0.11.0")
        unreleased, history = prepared.split("## [0.11.0]")
        self.assertIn("### Changed\n- New development work.", unreleased)
        self.assertIn("### Added\n- Another feature.", unreleased)
        self.assertNotIn("Existing change.", unreleased)
        self.assertIn("Existing change.", history)
        self.assertNotIn("New development work.", history)
        self.assertEqual(prepare_changelog(original, original, released, "0.11.0"), released)
        edited = original.replace("- Existing change.", "- Updated description.")
        self.assertIn("Updated description.", prepare_changelog(edited, original, released, "0.11.0").split("## [0.11.0]")[0])
        with self.assertRaises(ValueError):
            prepare_changelog(prepared, original, released, "0.11.0")

    def test_stable_and_snapshot_version_discovery(self):
        for snapshot, channel, version in (
            (False, "latest", "0.11.0+123"),
            (True, "snapshot", "0.11.0-snapshot+123"),
        ):
            with self.subTest(channel=channel), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                (root / "Cargo.toml").write_text('[package]\nversion = "0.11.0"\n')
                directory = root / "assets"
                directory.mkdir()
                for name in CLI_FILES:
                    (directory / name).write_bytes(name.encode())
                package(directory, metadata(root, snapshot, "123", "a" * 40))
                properties = (directory / f"version-{channel}.properties").read_text()
                self.assertEqual(properties.splitlines()[0], f"version={version}")
                self.assertRegex(properties.splitlines()[1], r"^buildDate=\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$")
                other_channel = "latest" if snapshot else "snapshot"
                self.assertFalse((directory / f"version-{other_channel}.properties").exists())
                self.assertIn(f"  version-{channel}.properties\n", (directory / "SHA256SUMS").read_text())

    def test_download_metadata_and_checksums_require_all_eight_clis(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "Cargo.toml").write_text('[package]\nversion = "0.11.0"\n')
            directory = root / "assets"
            directory.mkdir()
            info = metadata(root, True, "123", "a" * 40)
            with self.assertRaises(ValueError):
                package(directory, info)
            for name in CLI_FILES:
                (directory / name).write_bytes(name.encode())
            package(directory, info)
            document = json.loads((directory / "version.json").read_text())
            self.assertEqual(document["build_version"], "0.11.0-snapshot+123")
            self.assertEqual(len(document["artifacts"]), 8)
            for line in (directory / "SHA256SUMS").read_text().splitlines():
                checksum, name = line.split("  ")
                self.assertEqual(checksum, hashlib.sha256((directory / name).read_bytes()).hexdigest())
            package(directory, info)  # Packaging is repeatable.
            (directory / "matchbox-slim-linux-x64").write_bytes(b"obsolete")
            with self.assertRaises(ValueError):
                package(directory, info)


if __name__ == "__main__":
    unittest.main()
