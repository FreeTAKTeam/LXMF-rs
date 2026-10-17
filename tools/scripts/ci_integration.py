#!/usr/bin/env python3
"""Run selected Cargo integration targets without repeating lib/bin suites."""
import json
import os
import shlex
import subprocess

from ci_selection import validate_selection


def commands(selection):
    validate_selection(selection)
    if not selection["jobs"]["test-integration"]:
        raise ValueError("integration job was not selected")
    if selection["workspace_tests"]:
        return [["cargo", "test", "--locked", "--workspace", "--tests", "--", "--test-threads=4"]]
    result = []
    for row in selection["integration"]:
        args = ["cargo", "test", "--locked", "--package", row["package"]]
        for target in row["tests"]:
            args.extend(["--test", target])
        if row["features"]:
            args.extend(["--features", ",".join(row["features"])])
        result.append(args + ["--", "--test-threads=4"])
    return result


def main():
    selection = json.loads(os.environ["CI_SELECTION"])
    for args in commands(selection):
        command = shlex.join(args)
        print(f"::group::{command}", flush=True)
        try:
            completed = subprocess.run(args, check=False, timeout=1800)
        finally:
            print("::endgroup::", flush=True)
        if os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as summary:
                summary.write(f"- `{command}`: exit {completed.returncode}\n")
        if completed.returncode:
            raise SystemExit(completed.returncode)


if __name__ == "__main__":
    main()
