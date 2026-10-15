#!/usr/bin/env python3
"""Exercise real Cargo metadata, package lists and an archive without a registry."""

from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib
import unittest

from check_license_metadata import check


LICENSE = "Existing primary license\nExisting conditional secondary-license notice\n"


class LicenseMetadataTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.write("LICENSE", LICENSE)
        self.write("Cargo.toml", '''[workspace]
members = ["library", "internal"]
exclude = ["vendor/peer"]
resolver = "2"
[workspace.package]
license = "EPL-2.0 OR GPL-2.0-or-later"
license-file = "LICENSE"
''')
        self.write("library/Cargo.toml", '''[package]
name = "license-check-fixture"
version = "0.1.0"
edition = "2021"
license.workspace = true
license-file.workspace = true
''')
        self.write("library/src/lib.rs", "pub fn example() {}\n")
        self.write("internal/Cargo.toml", '''[package]
name = "private-fixture"
version = "0.1.0"
publish = false
license = "MIT"
''')
        self.write("internal/src/lib.rs", "")

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    def replace(self, name, before, after):
        path = self.root / name
        path.write_text(path.read_text().replace(before, after))

    def add_peer(self, *, reason="Retains the upstream MIT notice", exclude=False):
        with (self.root / "library/Cargo.toml").open("a") as manifest:
            manifest.write('\n[dependencies]\npeer = { package = "upstream-fixture", path = "../vendor/peer" }\n')
        self.write("vendor/peer/Cargo.toml", '''[package]
name = "upstream-fixture"
version = "0.1.0"
license = "MIT"
''' + ('exclude = ["NOTICE.upstream"]\n' if exclude else ''))
        self.write("vendor/peer/src/lib.rs", "")
        self.write("vendor/peer/NOTICE.upstream", "Upstream copyright and MIT grant\n")
        with (self.root / "Cargo.toml").open("a") as manifest:
            manifest.write(f'''
[workspace.metadata.license-check.exceptions."vendor/peer"]
license = "MIT"
notices = ["NOTICE.upstream"]
reason = "{reason}"
''')

    def test_inherited_metadata_and_private_package(self):
        self.assertEqual(check(self.root), (1, []))

    def test_inconsistent_spdx_identifies_package_and_correction(self):
        self.replace("library/Cargo.toml", 'license.workspace = true', 'license = "MIT"')
        _, errors = check(self.root)
        self.assertTrue(any("license-check-fixture" in error and "SPDX" in error and "Inherit" in error for error in errors), errors)

    def test_checkout_license_does_not_prove_packaged_notice(self):
        self.replace("library/Cargo.toml", 'license-file.workspace = true', '')
        _, errors = check(self.root)
        self.assertTrue(any("distributable is missing LICENSE" in error for error in errors), errors)

    def test_missing_source_notice_fails(self):
        (self.root / "LICENSE").unlink()
        with self.assertRaisesRegex(ValueError, "missing or empty"):
            check(self.root)

    def test_documented_third_party_license_is_preserved(self):
        self.add_peer()
        self.assertEqual(check(self.root), (2, []))

    def test_undocumented_first_party_exception_fails(self):
        self.replace("library/Cargo.toml", 'license.workspace = true', 'license = "Apache-2.0"')
        _, errors = check(self.root)
        self.assertTrue(any("document an intentional exception" in error for error in errors), errors)

    def test_exception_without_reason_fails(self):
        self.add_peer(reason="")
        _, errors = check(self.root)
        self.assertTrue(any("document exception" in error for error in errors), errors)

    def test_documented_first_party_exception(self):
        self.replace("library/Cargo.toml", 'license.workspace = true', 'license = "MIT"')
        self.replace("library/Cargo.toml", 'license-file.workspace = true', 'license-file = "LICENSE-MIT"')
        self.write("library/LICENSE-MIT", "Retained MIT grant for this package\n")
        with (self.root / "Cargo.toml").open("a") as manifest:
            manifest.write('''
[workspace.metadata.license-check.exceptions.library]
license = "MIT"
notices = ["LICENSE-MIT"]
reason = "Existing package-specific grant"
''')
        self.assertEqual(check(self.root), (1, []))

    def test_stale_exception_is_reported(self):
        with (self.root / "Cargo.toml").open("a") as manifest:
            manifest.write('''
[workspace.metadata.license-check.exceptions.missing]
license = "MIT"
notices = ["LICENSE"]
reason = "No such package"
''')
        _, errors = check(self.root)
        self.assertTrue(any("does not name a discovered publishable package" in error for error in errors), errors)

    def test_notice_excluded_from_actual_package_fails(self):
        self.add_peer(exclude=True)
        self.assertTrue((self.root / "vendor/peer/NOTICE.upstream").is_file())
        _, errors = check(self.root)
        self.assertTrue(any("distributable is missing NOTICE.upstream" in error for error in errors), errors)

    def test_packaged_archive_retains_complete_license_and_notice(self):
        self.assertEqual(check(self.root), (1, []))
        subprocess.run(
            ["cargo", "package", "--no-verify", "--offline", "--allow-dirty", "-p", "license-check-fixture"],
            cwd=self.root, check=True, capture_output=True, text=True,
        )
        archive = self.root / "target/package/license-check-fixture-0.1.0.crate"
        with tarfile.open(archive) as package:
            with package.extractfile("license-check-fixture-0.1.0/LICENSE") as notice:
                self.assertEqual(notice.read(), LICENSE.encode())
            with package.extractfile("license-check-fixture-0.1.0/Cargo.toml") as manifest:
                metadata = tomllib.loads(manifest.read().decode())["package"]
                self.assertEqual(metadata["license"], "EPL-2.0 OR GPL-2.0-or-later")
                self.assertEqual(metadata["license-file"], "LICENSE")


if __name__ == "__main__":
    unittest.main()
