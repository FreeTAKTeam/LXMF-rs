#!/usr/bin/env python3
"""Require complete, non-duplicated PR HIL evidence from the candidate commit."""
import argparse
from collections import Counter
import json
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def expected_cases(root):
    lab = tomllib.loads((root / "tests/hil/lab.toml").read_text())
    cases = [case for path in sorted(root.glob("tests/hil/cases/*.toml"))
             for case in tomllib.loads(path.read_text()).get("cases", [])]
    return {(profile["id"], case["id"])
            for profile in lab["profiles"] if not profile.get("physical", False)
            for case in cases if "pr" in case["levels"]
            and (not case.get("profiles") or profile["id"] in case["profiles"])
            and case["suite"] in profile["suites"]}


def validate(root, evidence, commit):
    expected = expected_cases(root)
    reports = sorted(evidence.rglob("result.json"))
    if not expected or not reports:
        raise ValueError("missing PR case inventory or shard reports")
    observed = Counter()
    for path in reports:
        report = json.loads(path.read_text())
        if (report.get("schema") != "lxmf-rs-hil-result-v1"
                or report.get("level") != "pr" or report.get("result") != "PASS"
                or report.get("commit_sha") != commit):
            raise ValueError(f"failed, stale, or invalid PR shard: {path}")
        for case in report["cases"]:
            if case.get("result") != "PASS":
                raise ValueError(f"non-passing case in {path}: {case}")
            observed[(case["profile_id"], case["case_id"])] += 1
    missing = sorted(expected - observed.keys())
    unexpected = sorted(observed.keys() - expected)
    duplicated = sorted(key for key, count in observed.items() if count != 1)
    if missing or unexpected or duplicated:
        raise ValueError(f"PR HIL coverage: missing={missing}, unexpected={unexpected}, "
                         f"duplicated={duplicated}")
    return len(reports), len(expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    try:
        reports, cases = validate(ROOT, args.evidence, args.commit)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"HIL PR evidence: FAIL: {error}", file=sys.stderr)
        return 1
    print(f"HIL PR evidence: PASS ({cases} case/profile pairs across {reports} shards)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
