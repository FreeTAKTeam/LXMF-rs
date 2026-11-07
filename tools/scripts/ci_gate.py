#!/usr/bin/env python3
"""Evaluate all CI dependencies; missing evidence is never success."""
import json
import os
import sys

from ci_selection import validate_selection


def evaluate(selection, needs):
    try:
        validate_selection(selection)
    except (ValueError, TypeError, KeyError) as error:
        return [f"invalid selection: {error}"]
    expected = dict(selection["jobs"], selection=True)
    if not isinstance(needs, dict) or set(needs) != set(expected):
        return ["missing or unexpected workflow dependencies"]
    failures = []
    for job, required in expected.items():
        value = needs[job]
        status = value.get("result") if isinstance(value, dict) else None
        allowed = {"success"} if required else {"success", "skipped"}
        if status not in allowed:
            failures.append(f"{job}: required={required}, result={status!r}")
    return failures


def main():
    try:
        selection = json.loads(os.environ.get("CI_SELECTION", "null"))
        needs = json.loads(os.environ.get("CI_NEEDS", "null"))
        failures = evaluate(selection, needs)
    except (ValueError, TypeError) as error:
        selection, needs, failures = None, None, [f"invalid gate input: {error}"]
    report = {"selection": selection, "outcomes": needs, "failures": failures}
    encoded = json.dumps(report, indent=2)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as summary:
            summary.write("## Required CI result\n\n```json\n" + encoded + "\n```\n")
    print(encoded)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
