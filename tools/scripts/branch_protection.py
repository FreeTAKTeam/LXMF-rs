#!/usr/bin/env python3
"""Read-only native policy check, or explicit conflict-preserving initial apply."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import quote


class Inaccessible(RuntimeError):
    def __init__(self, status, message):
        super().__init__(f"GitHub {status}: {message}")
        self.status, self.message = str(status), message


def api(endpoint, payload=None):
    args = ["gh", "api", endpoint]
    if payload is not None:
        args.extend(["--method", "PUT", "--input", "-"])
    result = subprocess.run(args, input=json.dumps(payload) if payload is not None else None,
                            text=True, capture_output=True, check=False, timeout=60)
    try:
        value = json.loads(result.stdout)
    except ValueError as error:
        raise Inaccessible("unknown", "GitHub returned no readable API response") from error
    if result.returncode:
        raise Inaccessible(value.get("status", "unknown"), value.get("message", "request failed"))
    return value


def normalize(current):
    value = {key: item for key, item in current.items() if key != "url"}
    for key, item in list(value.items()):
        if isinstance(item, dict) and set(item) <= {"enabled", "url"} and "enabled" in item:
            value[key] = item["enabled"]
    if value.get("required_signatures") is False:
        del value["required_signatures"]
    checks = value.get("required_status_checks")
    if isinstance(checks, dict):
        checks = {key: item for key, item in checks.items() if key not in {"url", "contexts_url"}}
        rows = checks.get("checks", [])
        contexts = checks.pop("contexts", [row["context"] for row in rows])
        if sorted(contexts) != sorted(row["context"] for row in rows):
            raise ValueError("ambiguous required status contexts")
        checks["checks"] = sorted(rows, key=lambda row: row["context"])
        value["required_status_checks"] = checks
    reviews = value.get("required_pull_request_reviews")
    if isinstance(reviews, dict):
        reviews = {key: item for key, item in reviews.items() if key != "url"}
        for key in ("dismissal_restrictions", "bypass_pull_request_allowances"):
            restriction = reviews.get(key)
            if isinstance(restriction, dict) and set(restriction) <= {"users", "teams", "apps", "url", "users_url", "teams_url"}:
                if not any(restriction.get(kind) for kind in ("users", "teams", "apps")):
                    reviews.pop(key)
        value["required_pull_request_reviews"] = reviews
    return value


def classify(current, effective_rules, policy):
    if not isinstance(effective_rules, list):
        return "conflicting", "Unrecognized effective-rules response."
    if effective_rules:
        return "conflicting", "Applicable native rulesets exist; do not layer another policy."
    if current is None:
        return "missing", "Branch exists and is explicitly reported as unprotected."
    try:
        actual, expected = normalize(current), normalize(policy)
    except (ValueError, KeyError, TypeError) as error:
        return "conflicting", str(error)
    if actual == expected:
        return "correct", "Native protection matches the repository-owned policy."
    differences = sorted(key for key in actual.keys() | expected.keys() if actual.get(key) != expected.get(key))
    return "conflicting", "Existing controls differ; preserve them: " + ", ".join(differences)


def verify_checks(checks, policy):
    for required in policy["required_status_checks"]["checks"]:
        matching = [row for row in checks if row["name"] == required["context"]]
        if (len(matching) != 1 or matching[0].get("app", {}).get("id") != required["app_id"]
                or matching[0].get("status") != "completed" or matching[0].get("conclusion") != "success"):
            raise ValueError(f"No unambiguous successful Actions check for {required['context']}")


def inspect(repository, branch, policy):
    base = f"repos/{repository}"
    # Establish accessibility before interpreting the protection endpoint's 404.
    api(base)
    api(f"{base}/branches/{quote(branch, safe='')}")
    rules = api(f"{base}/rules/branches/{quote(branch, safe='')}")
    rulesets = api(f"{base}/rulesets?includes_parents=true&per_page=100")
    if not isinstance(rules, list) or not isinstance(rulesets, list) or not all(isinstance(row, dict) for row in rulesets):
        raise Inaccessible("unknown", "unrecognized rules response")
    if len(rulesets) >= 100:
        raise Inaccessible("unknown", "ruleset pagination requires manual inspection")
    # Even evaluation/disabled branch policies deserve review before layering new protection.
    if any(row.get("target") != "tag" for row in rulesets):
        rules = rules or [{"type": "existing branch ruleset"}]
    endpoint = f"{base}/branches/{quote(branch, safe='')}/protection"
    try:
        current = api(endpoint)
    except Inaccessible as error:
        if error.status != "404" or error.message != "Branch not protected":
            raise
        current = None
    return endpoint, classify(current, rules, policy)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["check", "apply"])
    parser.add_argument("--config", type=Path, default=Path(__file__).resolve().parents[2] / ".github/branch-protection.json")
    parser.add_argument("--head", help="Exact reviewed PR head whose required checks have all passed")
    args = parser.parse_args()
    config = json.loads(args.config.read_text())
    repository, branch, policy = config["repository"], config["branch"], config["protection"]
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("invalid repository")
    endpoint, (state, detail) = inspect(repository, branch, policy)
    if args.mode == "apply" and state == "missing":
        if not args.head or not re.fullmatch(r"[0-9a-f]{40}", args.head):
            raise ValueError("apply requires an exact --head with successful required contexts")
        observed = api(f"repos/{repository}/commits/{args.head}/check-runs?filter=latest&per_page=100")
        if observed["total_count"] > 100:
            raise ValueError("ambiguous check pagination; inspect before applying")
        verify_checks(observed["check_runs"], policy)
        # Re-read just before mutation to avoid replacing a newly installed policy.
        _, fresh = inspect(repository, branch, policy)
        if fresh[0] != "missing":
            raise ValueError("protection changed while preparing apply; recheck")
        api(endpoint, policy)
        _, (state, detail) = inspect(repository, branch, policy)
    print(json.dumps({"state": state, "repository": repository, "branch": branch, "detail": detail}))
    return 0 if state == "correct" else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (Inaccessible, subprocess.SubprocessError, OSError) as error:
        print(json.dumps({"state": "inaccessible", "detail": str(error)}))
        sys.exit(2)
    except (KeyError, TypeError, ValueError) as error:
        print(json.dumps({"state": "conflicting", "detail": str(error)}))
        sys.exit(1)
