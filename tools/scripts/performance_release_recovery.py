"""Apply a pinned benchmark-only overlay while retaining the candidate commit."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

from release_config import classify_tag, release_metadata


OVERLAY_PATHS = (
    "tools/scripts/python-lxmd-rust-lxmd-smoke.sh",
    "tools/scripts/performance_fixture.py",
    "tools/scripts/e2e_performance.py",
)


def git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def prepare(candidate: Path, harness: Path, harness_ref: str, tag: str, publish: bool) -> dict:
    if not re.fullmatch(r"[0-9a-f]{40}", harness_ref):
        raise ValueError("harness_ref must be a full immutable 40-character commit SHA")
    candidate_sha = git(candidate, "rev-parse", "HEAD")
    harness_sha = git(harness, "rev-parse", "HEAD")
    if harness_sha != harness_ref:
        raise ValueError("harness checkout does not match the pinned harness SHA")
    if git(candidate, "status", "--porcelain", "--untracked-files=no"):
        raise ValueError("candidate tracked files must be clean before the benchmark overlay")
    classify_tag(tag)
    tag_sha = None
    if publish:
        release_metadata(tag, (candidate / "VERSION").read_text().strip())
        tag_sha = git(candidate, "rev-parse", f"refs/tags/{tag}^{{commit}}")
        if candidate_sha != tag_sha:
            raise ValueError("publication candidate must be the exact release tag commit")
    files = []
    for name in OVERLAY_PATHS:
        source = harness / name
        committed = subprocess.check_output(["git", "-C", str(harness), "show", f"{harness_sha}:{name}"])
        if source.read_bytes() != committed:
            raise ValueError(f"harness file differs from pinned commit: {name}")
        destination = candidate / name
        original = hashlib.sha256(destination.read_bytes()).hexdigest() if destination.is_file() else None
        destination.write_bytes(committed)
        files.append({"path": name, "candidate_sha256": original,
                      "harness_sha256": hashlib.sha256(committed).hexdigest()})
    changed = set(git(candidate, "diff", "--name-only", "HEAD").splitlines())
    if not changed.issubset(OVERLAY_PATHS):
        raise ValueError(f"benchmark overlay changed non-allowlisted files: {sorted(changed)}")
    provenance = {"schema_version": 1, "candidate_sha": candidate_sha,
                  "harness_sha": harness_sha, "release_tag": tag,
                  "release_tag_sha": tag_sha, "overlay_files": files}
    output = candidate / "target/performance/lxmf-rs-performance-provenance.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(provenance, indent=2, sort_keys=True) + "\n")
    return provenance


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--harness", required=True, type=Path)
    parser.add_argument("--harness-ref", required=True)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--publish", action="store_true")
    args = parser.parse_args()
    try:
        result = prepare(args.candidate, args.harness, args.harness_ref, args.release_tag, args.publish)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"performance_release_recovery: {error}") from error
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
