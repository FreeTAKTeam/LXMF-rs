#!/usr/bin/env python3
"""Shared tag, platform and publication policy for hosted release workflows."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

# Keep build and Linux packaging selection together; workflows consume these
# matrices rather than maintaining independent platform inventories.
TARGETS = [
    dict(id="linux-x86_64", label="Linux x86_64 (musl)", os="ubuntu-latest",
         target="x86_64-unknown-linux-musl", artifact_arch="linux-x86_64",
         use_cross=False, suffix="", package_arch="amd64"),
    dict(id="linux-aarch64", label="Linux aarch64 (musl)", os="ubuntu-latest",
         target="aarch64-unknown-linux-musl", artifact_arch="linux-aarch64",
         use_cross=True, suffix="", package_arch="arm64"),
    dict(id="linux-armv7", label="Linux armv7 (musleabihf)", os="ubuntu-latest",
         target="armv7-unknown-linux-musleabihf", artifact_arch="linux-armv7",
         use_cross=True, suffix="", package_arch="armhf"),
    dict(id="windows-x86_64", label="Windows x86_64", os="windows-latest",
         target="x86_64-pc-windows-msvc", artifact_arch="windows-x86_64",
         use_cross=False, suffix=".exe"),
    dict(id="macos-x86_64", label="macOS x86_64", os="macos-15-intel",
         target="x86_64-apple-darwin", artifact_arch="macos-x86_64",
         use_cross=False, suffix=""),
    dict(id="macos-aarch64", label="macOS aarch64", os="macos-14",
         target="aarch64-apple-darwin", artifact_arch="macos-aarch64",
         use_cross=False, suffix=""),
]
REQUIRED_JOBS = ("prepare", "sdk-wrapper-parity", "build", "package-linux", "sbom", "container")
PLATFORM_JOBS = ("package-windows", "package-macos")
# Build metadata is excluded because '+' cannot be used in the OCI version tag.
TAG = re.compile(r"v?(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?")


def classify_tag(tag):
    match = TAG.fullmatch(tag)
    if not match or len(tag.removeprefix("v")) > 128:
        raise ValueError(f"invalid release tag: {tag!r}")
    prerelease = match.group(4)
    if prerelease and any(part.isdigit() and len(part) > 1 and part[0] == "0"
                          for part in prerelease.split(".")):
        raise ValueError(f"invalid numeric prerelease identifier in {tag!r}")
    return {"version": tag.removeprefix("v"), "prerelease": str(prerelease is not None).lower()}


def release_metadata(tag, project_version):
    metadata = classify_tag(tag)
    if metadata["version"] != project_version.strip():
        raise ValueError(f"release tag {tag!r} does not match tagged VERSION {project_version.strip()!r}")
    linux_only = metadata["version"].endswith("-linux")
    selected = [target for target in TARGETS if not linux_only or "package_arch" in target]
    metadata.update(
        linux_only=str(linux_only).lower(),
        build_matrix=json.dumps({"include": [
            {key: value for key, value in target.items() if key != "package_arch"}
            for target in selected
        ]}, separators=(",", ":")),
        linux_package_matrix=json.dumps({"include": [
            {"build_id": target["id"], "arch": target["package_arch"]}
            for target in selected if "package_arch" in target
        ]}, separators=(",", ":")),
    )
    return metadata


def validate_needs(needs):
    if not isinstance(needs, dict) or set(needs) != set(REQUIRED_JOBS + PLATFORM_JOBS):
        raise ValueError("release dependency inventory is missing or unexpected")
    prepare = needs.get("prepare")
    outputs = prepare.get("outputs") if isinstance(prepare, dict) else None
    linux_only = outputs.get("linux_only") if isinstance(outputs, dict) else None
    if linux_only not in ("true", "false"):
        raise ValueError("prepare did not declare a valid Linux-only policy")
    for name in REQUIRED_JOBS + PLATFORM_JOBS:
        job = needs[name]
        result = job.get("result") if isinstance(job, dict) else None
        expected = "skipped" if linux_only == "true" and name in PLATFORM_JOBS else "success"
        if result != expected:
            raise ValueError(f"release dependency {name!r} is {result!r}; expected {expected!r}")


def git(*args):
    return subprocess.check_output(["git", *args], text=True).strip()


def emit(metadata, output):
    text = "".join(f"{name}={value}\n" for name, value in metadata.items())
    if output:
        with Path(output).open("a") as destination:
            destination.write(text)
    else:
        sys.stdout.write(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare")
    prepare.add_argument("--tag", required=True)
    prepare.add_argument("--publish", choices=("true", "false"), required=True)
    prepare.add_argument("--output")
    classify = commands.add_parser("classify")
    classify.add_argument("--tag", required=True)
    classify.add_argument("--version-file", type=Path, required=True)
    classify.add_argument("--output")
    check = commands.add_parser("check-needs")
    check.add_argument("--needs-json", required=True)
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            classify_tag(args.tag)  # Reject malformed or shell-active refs before invoking Git.
            source = git("rev-parse", "--verify", f"refs/tags/{args.tag}^{{commit}}")
            metadata = release_metadata(args.tag, git("show", f"{source}:VERSION"))
            metadata.update(tag=args.tag, checkout_ref=args.tag, source_commit=source, publish=args.publish)
            emit(metadata, args.output)
        elif args.command == "classify":
            metadata = release_metadata(args.tag, args.version_file.read_text())
            emit({name: metadata[name] for name in ("version", "prerelease")}, args.output)
        else:
            validate_needs(json.loads(args.needs_json))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"release configuration failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
