#!/usr/bin/env python3
"""Create/verify a checksummed AAR + packaged Rust pair from an immutable source."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tomllib
import zipfile


def digest(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def validate_aar(path, revision):
    with zipfile.ZipFile(path) as archive:
        if any(name.endswith(".so") for name in archive.namelist()):
            raise ValueError("companion must not package native libraries")
        identity = json.loads(archive.read("assets/bluetooth-build.json"))
        if identity != {"bridge_version": 1, "source_revision": revision, "dirty": False}:
            raise ValueError("AAR source identity mismatch or dirty build")
        if "classes.jar" not in archive.namelist():
            raise ValueError("companion classes missing")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["create", "verify"])
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--aar", required=True, type=Path)
    parser.add_argument("--crate", required=True, type=Path)
    parser.add_argument("--revision", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9a-f]{40}", args.revision):
        raise ValueError("full immutable source revision required")
    validate_aar(args.aar, args.revision)
    with tarfile.open(args.crate) as package:
        names = [m for m in package.getmembers() if m.name.endswith('/.cargo_vcs_info.json')]
        if len(names) != 1:
            raise ValueError("crate source identity missing")
        prefix = names[0].name.rsplit('/', 1)[0]
        manifest = tomllib.loads(package.extractfile(prefix + '/Cargo.toml').read().decode())
        if (manifest['package']['name'] != 'reticulum-rs-bluetooth'
                or manifest['lib']['name'] != 'rns_bluetooth'
                or manifest['package']['edition'] != '2024'
                or manifest['package']['rust-version'] != '1.85'):
            raise ValueError("not the expected Bluetooth Rust package")
        info = json.load(package.extractfile(names[0]))
        if info["git"]["sha1"] != args.revision or info["git"].get("dirty", False):
            raise ValueError("crate source identity mismatch or dirty package")
    expected = {"schema": 1, "bridge_version": 1, "source_revision": args.revision,
                "aar_sha256": digest(args.aar), "crate_sha256": digest(args.crate)}
    if args.mode == "verify":
        if json.loads(args.manifest.read_text()) != expected:
            raise ValueError("Bluetooth artifact pair mismatch")
    else:
        root = Path(__file__).resolve().parents[2]
        def git(*command):
            return subprocess.check_output(["git", *command], cwd=root, text=True).strip()
        if git("rev-parse", "HEAD") != args.revision:
            raise ValueError("artifact revision is not checked-out HEAD")
        if git("status", "--porcelain", "--untracked-files=normal"):
            raise ValueError("create requires a clean source checkout")
        # Build artifacts must be provided by the caller's build of this checkout.
        # Checksums prove pair integrity; signed provenance remains the release job's responsibility.
        args.manifest.write_text(json.dumps(expected, indent=2) + "\n")
    print("Bluetooth artifact pair verified")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, tarfile.TarError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
