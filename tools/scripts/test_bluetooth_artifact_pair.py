"""Synthetic artifact tests: never load real devices, identities or captures."""
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile

SCRIPT = Path(__file__).with_name("bluetooth-artifact-pair.py")
REVISION = "a" * 40


class PairTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.aar = self.root / "companion.aar"
        self.crate = self.root / "package.crate"
        self.manifest = self.root / "pair.json"
        self.build_aar()
        self.build_crate()
        self.pin()

    def build_aar(self, *, native=False, dirty=False):
        with zipfile.ZipFile(self.aar, "w") as archive:
            archive.writestr("classes.jar", b"synthetic-test-content")
            archive.writestr("assets/bluetooth-build.json", json.dumps({
                "bridge_version": 1, "source_revision": REVISION, "dirty": dirty,
            }))
            if native:
                archive.writestr("jni/arm64-v8a/duplicate.so", b"synthetic")

    def build_crate(self, name="reticulum-rs-bluetooth", library="rns_bluetooth"):
        with tarfile.open(self.crate, "w:gz") as archive:
            files = {
                "Cargo.toml": f'[package]\nname="{name}"\nversion="0.14.0"\nedition="2024"\nrust-version="1.85"\n[lib]\nname="{library}"\n',
                ".cargo_vcs_info.json": json.dumps({"git": {"sha1": REVISION, "dirty": False}}),
            }
            for name, text in files.items():
                data = text.encode()
                info = tarfile.TarInfo("package/" + name)
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))

    def pin(self):
        self.manifest.write_text(json.dumps({"schema": 1, "bridge_version": 1,
            "source_revision": REVISION,
            "aar_sha256": hashlib.sha256(self.aar.read_bytes()).hexdigest(),
            "crate_sha256": hashlib.sha256(self.crate.read_bytes()).hexdigest()}))

    def verify(self):
        return subprocess.run([sys.executable, str(SCRIPT), "verify", "--manifest", str(self.manifest),
            "--aar", str(self.aar), "--crate", str(self.crate), "--revision", REVISION],
            capture_output=True, text=True)

    def test_matching_pair(self):
        self.assertEqual(self.verify().returncode, 0)

    def test_wrong_rust_package_from_same_revision(self):
        self.build_crate("lxmf-sdk", "lxmf_sdk")
        self.pin()
        result = self.verify()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("expected Bluetooth Rust package", result.stderr)

    def test_changed_hash(self):
        with zipfile.ZipFile(self.aar, "a") as archive:
            archive.writestr("unexpected", b"changed")
        self.assertIn("pair mismatch", self.verify().stderr)

    def test_duplicate_native_library(self):
        self.build_aar(native=True)
        self.pin()
        self.assertIn("native libraries", self.verify().stderr)

    def test_dirty_source_identity(self):
        self.build_aar(dirty=True)
        self.pin()
        self.assertIn("dirty build", self.verify().stderr)


if __name__ == "__main__":
    unittest.main()
