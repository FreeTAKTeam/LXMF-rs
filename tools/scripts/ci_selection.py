#!/usr/bin/env python3
"""Conservative PR integration selection from Git paths and Cargo ownership."""
import argparse
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess

JOB_IDS = (
    "bluetooth-msrv-185", "bluetooth-companion", "mobile-ble-contract", "quality",
    "architecture-checks", "build-matrix", "windows-rnode-ble", "macos-interface-runtime",
    "linux-udp-runtime", "linux-weave-runtime", "test-nextest-unit",
    "peer-lifecycle-essential", "contracts", "security", "test-integration",
)
PEER_PACKAGES = {"lxmf-sdk", "lxmf-runtime", "reticulum-rs-rpc", "reticulumd"}
PROSE_FILES = {"README.md", "CONTRIBUTING.md", "CHANGELOG.md", "AGENTS.md", "RUST_ENGINEERING_PRINCIPLES.md"}
PROSE_FILES |= {f"docs/goals/open-issues-2026-10/{name}.md"
                for name in ("PLAN", "GOAL", "ACCEPTANCE", "PROGRESS")}


def required_jobs(selection):
    mode = selection["mode"]
    if mode == "schedule":
        return {job: job == "security" for job in JOB_IDS}
    jobs = {job: mode != "docs" or job in {"security", "contracts"} for job in JOB_IDS}
    jobs["test-integration"] = bool(selection["integration"]) or selection["workspace_tests"]
    jobs["peer-lifecycle-essential"] = mode == "broad" or bool(set(selection["packages"]) & PEER_PACKAGES)
    return jobs


def validate_selection(value):
    if not isinstance(value, dict) or set(value) != {
        "version", "mode", "reasons", "packages", "integration", "workspace_tests", "jobs"
    }:
        raise ValueError("missing or unexpected selection fields")
    if type(value["version"]) is not int or value["version"] != 1:
        raise ValueError("unsupported selection version")
    if value["mode"] not in {"docs", "selected", "broad", "schedule"}:
        raise ValueError("invalid selection mode")
    for key in ("reasons", "packages"):
        if not isinstance(value[key], list) or not all(isinstance(item, str) and item for item in value[key]):
            raise ValueError(f"invalid {key}")
    if not value["reasons"] or value["packages"] != sorted(set(value["packages"])):
        raise ValueError("missing reasons or ambiguous packages")
    if type(value["workspace_tests"]) is not bool or not isinstance(value["integration"], list):
        raise ValueError("invalid integration selection")
    seen = set()
    for row in value["integration"]:
        if not isinstance(row, dict) or set(row) != {"package", "tests", "features"}:
            raise ValueError("invalid integration target")
        if row["package"] not in value["packages"] or row["package"] in seen:
            raise ValueError("unknown or duplicate integration package")
        seen.add(row["package"])
        for key in ("tests", "features"):
            if (not isinstance(row[key], list) or not all(isinstance(item, str)
                    and re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_+./-]*", item) for item in row[key])
                    or row[key] != sorted(set(row[key]))):
                raise ValueError(f"invalid integration {key}")
        if not row["tests"]:
            raise ValueError("empty integration target")
    if value["mode"] in {"docs", "schedule"} and (value["packages"] or value["integration"] or value["workspace_tests"]):
        raise ValueError("software work in a non-software selection")
    if value["mode"] == "selected" and (not value["packages"] or value["workspace_tests"]):
        raise ValueError("invalid selected package set")
    if value["mode"] == "broad" and not (value["packages"] or value["workspace_tests"]):
        raise ValueError("empty broad selection")
    if value["mode"] == "broad" and not (value["integration"] or value["workspace_tests"]):
        raise ValueError("broad selection omitted integration work")
    jobs = value["jobs"]
    if (not isinstance(jobs, dict) or set(jobs) != set(JOB_IDS)
            or any(type(required) is not bool for required in jobs.values())
            or jobs != required_jobs(value)):
        raise ValueError("inconsistent required-job selection")


def parse_changes(data):
    """Read --name-status -z, retaining both sides of renames/copies."""
    if not data:
        return []
    if not data.endswith(b"\0"):
        raise ValueError("truncated NUL diff")
    try:
        fields = iter(data[:-1].decode("utf-8").split("\0"))
        changes = []
        for status in fields:
            if not re.fullmatch(r"[AMDT]|[RC][0-9]+", status):
                raise ValueError(f"unsupported diff status {status!r}")
            for _ in range(2 if status[0] in "RC" else 1):
                changes.append((status[0], next(fields)))
        return changes
    except (UnicodeDecodeError, StopIteration) as error:
        raise ValueError("invalid NUL diff") from error


def workspace(metadata):
    root = PurePosixPath(metadata["workspace_root"])
    if not root.is_absolute():
        raise ValueError("workspace root is not absolute")
    members = set(metadata["workspace_members"])
    packages, owners = {}, {}
    for package in metadata["packages"]:
        if package["id"] not in members:
            continue
        name = package["name"]
        owner = str(PurePosixPath(package["manifest_path"]).parent.relative_to(root))
        if not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_-]*", name) or name in packages or owner in owners:
            raise ValueError("ambiguous package ownership")
        packages[name] = package
        owners[owner] = name
    if not packages or len(packages) != len(members):
        raise ValueError("missing workspace packages")
    reverse = {name: set() for name in packages}
    for name, package in packages.items():
        for dependency in package["dependencies"]:
            if dependency.get("path"):
                path = str(PurePosixPath(dependency["path"]).relative_to(root))
                if path in owners:
                    reverse[owners[path]].add(name)
        for target in package["targets"]:
            if "test" in target["kind"] and not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_-]*", target["name"]):
                raise ValueError("invalid test target")
    return packages, owners, reverse


def select(metadata, changes, *, event="pull_request", labels=()):
    result = {"version": 1, "mode": "schedule", "reasons": [], "packages": [],
              "integration": [], "workspace_tests": False,
              "jobs": {job: job == "security" for job in JOB_IDS}}
    if event == "schedule":
        result["reasons"] = ["weekly security scan"]
        return result
    reasons, affected = set(), set()
    try:
        packages, owners, reverse = workspace(metadata)
    except (KeyError, TypeError, ValueError):
        packages, owners, reverse = {}, {}, {}
        reasons.add("Cargo metadata unavailable or ambiguous")
        result["workspace_tests"] = True
    if event not in {"pull_request", "push"}:
        reasons.add("explicit or unknown event")
    if set(labels) & {"run-full-CI", "run-peer-lifecycle-CI"}:
        reasons.add("label widens coverage")
    if not changes:
        reasons.add("diff unavailable or empty")
    for status, path in changes or []:
        parts = PurePosixPath(path).parts
        if status not in {"A", "M", "D", "T", "R", "C"} or not parts or path.startswith("/") or ".." in parts:
            reasons.add("unclassified diff entry")
            continue
        if path in PROSE_FILES:
            continue
        # Shared fixture/vendored edges are not necessarily Cargo dependencies.
        if (parts[-1] in {"Cargo.toml", "Cargo.lock"}
                or path.startswith((".github/", ".cargo/", "tests/", "tools/", "vendor/", "xtask/"))
                or "test-support" in parts):
            reasons.add(f"shared input: {path}")
            continue
        matches = [name for owner, name in owners.items() if path.startswith(owner + "/")]
        if len(matches) != 1:
            reasons.add(f"unclassified path: {path}")
        else:
            affected.add(matches[0])
    if reasons:
        affected = set(packages)
        result["mode"] = "broad"
    elif affected:
        result["mode"] = "selected"
        pending = list(affected)
        while pending:
            for consumer in reverse[pending.pop()] - affected:
                affected.add(consumer)
                pending.append(consumer)
    else:
        result["mode"] = "docs"
    result["packages"] = sorted(affected)
    result["reasons"] = sorted(reasons) or ["Cargo reverse dependency closure" if affected else "recognized prose only"]
    for name in sorted(affected):
        tests = sorted(t["name"] for t in packages[name]["targets"] if "test" in t["kind"])
        if tests:
            # Cargo normally skips targets with unmet required features. Never silently do that here.
            features = sorted({f for t in packages[name]["targets"] if "test" in t["kind"]
                               for f in t.get("required-features", [])})
            result["integration"].append({"package": name, "tests": tests, "features": features})
    if result["mode"] == "broad" and not result["integration"]:
        result["workspace_tests"] = True
    result["jobs"] = required_jobs(result)
    validate_selection(result)
    return result


def command_json(args):
    return json.loads(subprocess.check_output(args, stderr=subprocess.PIPE))


def github_selection():
    event_name = os.environ.get("GITHUB_EVENT_NAME", "workflow_dispatch")
    event_file = os.environ.get("GITHUB_EVENT_PATH")
    event = json.loads(Path(event_file).read_text()) if event_file else {}
    labels = [label["name"] for label in event.get("pull_request", {}).get("labels", [])]
    try:
        metadata = command_json(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"])
    except (subprocess.CalledProcessError, OSError, ValueError):
        metadata = None
    changes = None
    try:
        if event_name == "pull_request":
            base, head = event["pull_request"]["base"]["sha"], event["pull_request"]["head"]["sha"]
        else:
            base, head = event["before"], event["after"]
        if not all(re.fullmatch(r"[0-9a-f]{40}", sha) and sha != "0" * 40 for sha in (base, head)):
            raise ValueError("missing commit")
        if event_name == "pull_request":
            base = subprocess.check_output(["git", "merge-base", base, head], stderr=subprocess.PIPE).decode().strip()
        changes = parse_changes(subprocess.check_output(
            ["git", "diff", "--name-status", "-z", "--find-renames", base, head, "--"], stderr=subprocess.PIPE))
    except (KeyError, TypeError, ValueError, OSError, subprocess.CalledProcessError):
        pass  # Missing history selects the full software set below.
    return select(metadata, changes, event=event_name, labels=labels)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", default="target/ci/selection.json")
    args = parser.parse_args()
    result = github_selection()
    encoded = json.dumps(result, sort_keys=True, separators=(",", ":"))
    path = Path(args.output)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(encoded + "\n")
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write(f"selection={encoded}\n")
            for job, required in result["jobs"].items():
                output.write(f"{job}={str(required).lower()}\n")
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as summary:
            summary.write("## CI selection\n\n```json\n" + json.dumps(result, indent=2) + "\n```\n")
    print(encoded)


if __name__ == "__main__":
    main()
