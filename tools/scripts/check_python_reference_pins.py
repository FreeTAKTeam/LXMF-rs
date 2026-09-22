#!/usr/bin/env python3
"""Verify every active Python Reticulum pin matches the canonical manifest."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "tools/interop/independent-implementations.toml"


def manifest_data() -> dict[str, object]:
    with MANIFEST.open("rb") as handle:
        data = tomllib.load(handle)
    if not isinstance(data, dict):
        raise ValueError("canonical manifest must be a TOML object")
    return data


def canonical_pin() -> tuple[str, str]:
    data = manifest_data()
    version = data["rns_reference_version"]
    revision = data["rns_reference_revision"]
    python_reference = data["python_reference"]
    if python_reference["version"] != version or python_reference["revision"] != revision:
        raise ValueError("canonical top-level and python_reference pins differ")
    return version, revision


def parity_target_pin() -> tuple[str, str]:
    target = manifest_data().get("parity_target")
    if not isinstance(target, dict):
        raise ValueError("canonical manifest is missing parity_target")
    version = target.get("version")
    revision = target.get("revision")
    implementation = target.get("implementation")
    repository = target.get("repository")
    if not all(isinstance(value, str) and value for value in (version, revision)):
        raise ValueError("parity_target version and revision must be non-empty strings")
    if implementation != "Reticulum-Python":
        raise ValueError("parity_target implementation must be Reticulum-Python")
    if repository != "https://github.com/markqvist/Reticulum.git":
        raise ValueError("parity_target repository is not the canonical Reticulum repository")
    if target.get("owner_issue") != 605:
        raise ValueError("parity_target owner_issue must be issue 605")
    return version, revision


def active_mirrors() -> dict[str, tuple[str, ...]]:
    return {
        "crates/libs/lxmf-reference/src/lib.rs": (
            'PYTHON_RETICULUM_REFERENCE_VERSION: &str = "{version}"',
            'PYTHON_RETICULUM_REFERENCE_REF: &str = "{revision}"',
        ),
        ".github/workflows/verify.yml": ("PYTHON_RETICULUM_REF: {revision}",),
        ".github/workflows/performance-smoke.yml": ("checkout {revision}",),
        ".github/workflows/performance-release.yml": ("checkout {revision}",),
        ".github/workflows/hil-nightly.yml": ("ref: {revision}",),
        ".github/workflows/hil-release.yml": ("ref: {revision}",),
        "tools/benchmarks/python_impl.toml": ('reticulum = "{revision}"',),
        "xtask/src/hil/runner.rs": ('"python_rns_version".to_string(), "{version}".to_string()', '"{revision}".to_string()'),
        "crates/apps/lxmf-cli/tests/version_cli.rs": (
            'PYTHON_RETICULUM_VERSION: &str = "{version}"',
            'PYTHON_RETICULUM_REF: &str = "{revision}"',
        ),
        "README.md": (
            "targets Python Reticulum {version} at",
            "`{revision}` for the next release candidate",
        ),
        "docs/interop/README.md": ("Python Reticulum {version} at `{revision}`",),
        "docs/contracts/compatibility-contract.md": (
            "Python Reticulum compatibility is assessed against version `{version}` at commit",
            "`{revision}`. The version is diagnostic",
        ),
        "docs/runbooks/release-readiness.md": (
            "conformance `0319444b20e0815f26c6b9ceeba8fa44de037c9b`, Python Reticulum\n`{revision}`, and Python LXMF",
        ),
        "docs/fixtures/sdk-v2/rpc/sdk_negotiate_v2.response.valid.json": (
            '"python_reticulum_version": "{version}"',
            '"python_reticulum_ref": "{revision}"',
            '"version": "{version}"',
            '"revision": "{revision}"',
        ),
        "docs/fixtures/sdk-v2/rpc/sdk_snapshot_v2.response.valid.json": (
            '"python_reticulum_version": "{version}"',
            '"python_reticulum_ref": "{revision}"',
        ),
        "docs/fixtures/sdk-v2/rpc/sdk_status_v2.response.valid.json": (
            '"python_reticulum_version": "{version}"',
            '"python_reticulum_ref": "{revision}"',
        ),
        "tests/hil/lab.toml": ('"reticulum-{version}"',),
        "docs/status/current-roadmap.md": (
            "software-surface parity against Python RNS {version} at",
            "`{revision}`. The 1.5 alignment",
        ),
        "docs/status/reticulum-parity-matrix.md": (
            "The pinned Python baseline is RNS `{version}` at",
            "`{revision}`. Regenerating the strict public",
        ),
        "docs/status/rns-1.5-delta.md": (
            "The authority is tag `{version}`, peeled to",
            "`{revision}`; the previous active reference",
        ),
    }


def parity_target_mirrors() -> dict[str, tuple[str, ...]]:
    return {
        ".github/workflows/verify.yml": ("PYTHON_RETICULUM_PARITY_REF: {revision}",),
        "docs/status/current-roadmap.md": (
            "immutable RNS {version}\ndevelopment revision `{revision}`",
        ),
        "docs/status/reticulum-parity-matrix.md": (
            "`{revision}`, recorded as `{version}`",
        ),
        "docs/status/rns-1.5.4-delta.md": (
            "| Forward parity candidate | Reticulum-Python | `{version}` | `{revision}` |",
            "immutable development commit, not a release tag",
        ),
    }


def verify() -> list[str]:
    try:
        version, revision = canonical_pin()
        target_version, target_revision = parity_target_pin()
    except (KeyError, OSError, ValueError, tomllib.TOMLDecodeError) as error:
        return [f"cannot read canonical pin: {error}"]

    errors: list[str] = []
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        errors.append(f"canonical revision is not a full Git commit: {revision}")
    if not re.fullmatch(r"[0-9a-f]{40}", target_revision):
        errors.append(f"parity target revision is not a full Git commit: {target_revision}")
    if target_revision == revision:
        errors.append("parity target must remain distinct from the active release baseline")

    for relative, needles in active_mirrors().items():
        path = ROOT / relative
        try:
            content = path.read_text(encoding="utf-8")
        except OSError as error:
            errors.append(f"{relative}: cannot read mirror: {error}")
            continue
        for template in needles:
            needle = template.format(version=version, revision=revision)
            if needle not in content:
                errors.append(f"{relative}: missing canonical mirror {needle!r}")
    for relative, needles in parity_target_mirrors().items():
        path = ROOT / relative
        try:
            content = path.read_text(encoding="utf-8")
        except OSError as error:
            errors.append(f"{relative}: cannot read parity-target mirror: {error}")
            continue
        for template in needles:
            needle = template.format(version=target_version, revision=target_revision)
            if needle not in content:
                errors.append(f"{relative}: missing parity-target mirror {needle!r}")
    return errors


def self_test() -> None:
    assert "{version}" in active_mirrors()["crates/libs/lxmf-reference/src/lib.rs"][0]
    assert "{revision}" in parity_target_mirrors()[".github/workflows/verify.yml"][0]
    assert "{revision}" in parity_target_mirrors()["docs/status/rns-1.5.4-delta.md"][0]
    assert ROOT.name == "LXMF-rs-rns-1.5-alignment" or (ROOT / "Cargo.toml").is_file()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
    errors = verify()
    if errors:
        for error in errors:
            print(f"python-reference-pins: {error}", file=sys.stderr)
        return 1
    version, revision = canonical_pin()
    target_version, target_revision = parity_target_pin()
    print(
        "python-reference-pins: ok "
        f"baseline RNS {version} {revision}; "
        f"parity target RNS {target_version} {target_revision}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
