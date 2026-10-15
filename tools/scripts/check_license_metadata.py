#!/usr/bin/env python3
"""Check package metadata and Cargo's distributable file lists, not legal rights."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[2]


def cargo(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["cargo", *args], cwd=root, capture_output=True, text=True, check=False
    )
    if result.returncode:
        raise ValueError(f"cargo {' '.join(args)} failed:\n{result.stderr.strip()}")
    return result.stdout


def discover_packages(root: Path) -> list[dict]:
    """Include local path packages outside workspace membership, such as zeromq."""
    pending = [root / "Cargo.toml"]
    packages = {}
    while pending:
        manifest = pending.pop()
        if manifest in packages:
            continue
        metadata = json.loads(cargo(
            root, "metadata", "--no-deps", "--locked", "--format-version", "1",
            "--manifest-path", str(manifest),
        ))
        for package in metadata["packages"]:
            packages[Path(package["manifest_path"]).resolve()] = package
        for package in metadata["packages"]:
            for dependency in package["dependencies"]:
                if dependency.get("path"):
                    local = Path(dependency["path"]).resolve() / "Cargo.toml"
                    if local.is_relative_to(root) and local not in packages:
                        pending.append(local)
    return sorted(
        (package for package in packages.values() if package["publish"] != []),
        key=lambda package: package["name"],
    )


def check(root: Path) -> tuple[int, list[str]]:
    root = root.resolve()
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    declared = workspace["package"]
    if not declared.get("license") or not declared.get("license-file"):
        raise ValueError("Cargo.toml: set workspace.package.license and license-file")
    license_path = (root / declared["license-file"]).resolve()
    if not license_path.is_file() or not license_path.read_bytes().strip():
        raise ValueError(f"{license_path}: required workspace license/notice file is missing or empty")
    exceptions = workspace.get("metadata", {}).get("license-check", {}).get("exceptions", {})
    if not isinstance(exceptions, dict):
        raise ValueError("Cargo.toml: workspace.metadata.license-check.exceptions must be a table")
    packages = discover_packages(root)
    errors = []
    seen = set()
    for package in packages:
        folder = Path(package["manifest_path"]).parent
        relative = folder.relative_to(root).as_posix()
        label = f"{package['name']} ({relative}/Cargo.toml)"
        exception = exceptions.get(relative)
        seen.add(relative)
        if exception is not None:
            if (
                not isinstance(exception, dict)
                or not isinstance(exception.get("reason"), str)
                or not exception["reason"].strip()
                or not exception.get("license")
                or not isinstance(exception.get("notices"), list)
                or not exception["notices"]
                or not all(isinstance(name, str) and name.strip() for name in exception["notices"])
            ):
                errors.append(f"{label}: document exception license, notices and reason in workspace.metadata.license-check.exceptions")
                continue
            expected = exception["license"]
            notices = [(name, folder / name) for name in exception["notices"]]
        else:
            expected = declared["license"]
            notices = [(license_path.name, license_path)]
            package_license = package.get("license_file")
            if not package_license or (folder / package_license).resolve() != license_path:
                errors.append(f"{label}: inherit the existing license/secondary notice with license-file.workspace = true")
        if package["license"] != expected:
            errors.append(f"{label}: SPDX expression {package['license']!r}; expected {expected!r}. Inherit license.workspace or document an intentional exception")
        try:
            listing = set(cargo(
                root, "package", "--list", "--locked", "--allow-dirty",
                "--manifest-path", package["manifest_path"],
            ).splitlines())
        except ValueError as error:
            errors.append(f"{label}: {error}")
            continue
        for name, source in notices:
            if not source.is_file() or not source.read_bytes().strip():
                errors.append(f"{label}: required notice {source} is missing or empty; restore the retained notice")
            if name not in listing:
                errors.append(f"{label}: distributable is missing {name}; correct license-file or package include/exclude rules")
    for relative in sorted(set(exceptions) - seen):
        errors.append(f"Cargo.toml: license exception {relative!r} does not name a discovered publishable package")
    return len(packages), errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()
    try:
        count, errors = check(args.root)
    except (OSError, ValueError, KeyError) as error:
        print(f"license metadata check: {error}", file=sys.stderr)
        return 1
    if errors:
        for error in errors:
            print(f"license metadata check: {error}", file=sys.stderr)
        return 1
    print(f"License metadata and packaged notices checked for {count} publishable packages; no ownership or legal-compatibility determination.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
