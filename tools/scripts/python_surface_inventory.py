#!/usr/bin/env python3
"""Build and validate the pinned Python RNS/LXMF public surface inventory."""

from __future__ import annotations

import argparse
import ast
import fnmatch
import json
import re
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "tools/interop/independent-implementations.toml"
EXCLUDED_PARTS = {"vendor", "__pycache__"}
EXCLUDED_FILES = {"_version.py"}
VALID_IMPLEMENTATION = {"complete", "partial", "not-applicable"}
VALID_EVIDENCE = {
    "unit",
    "simulated",
    "pinned-python",
    "prepared-host",
    "hardware-unverified",
}
VALID_BEHAVIORAL_COVERAGE = {"incomplete", "complete", "blocked", "hardware-unverified"}
VALID_BEHAVIORAL_EVIDENCE = VALID_EVIDENCE | {
    "cross-implementation",
    "third-party-client",
    "hardware",
    "public-network",
    "planned",
}
VALID_BEHAVIORAL_EVIDENCE_STATUS = {
    "unverified",
    "verified",
    "blocked",
    "hardware-unverified",
}
VALID_REFERENCE_PROJECTS = {"reticulum", "lxmf", "both", "operational"}
FULL_REVISION = re.compile(r"[0-9a-f]{40}")


@dataclass(frozen=True)
class SurfaceItem:
    item_id: str
    kind: str
    source: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-rns-path", type=Path)
    parser.add_argument("--python-lxmf-path", type=Path)
    parser.add_argument(
        "--mapping",
        type=Path,
        default=Path("docs/status/python-surface-mapping.json"),
    )
    parser.add_argument(
        "--json-out",
        type=Path,
        default=Path("docs/status/python-surface-parity.json"),
    )
    parser.add_argument(
        "--rust-out",
        type=Path,
        default=Path("crates/libs/lxmf-reference/src/python_software_parity.rs"),
    )
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--require-complete", action="store_true")
    parser.add_argument("--require-behavioral-complete", action="store_true")
    return parser.parse_args()


def load_manifest() -> dict[str, Any]:
    try:
        with MANIFEST.open("rb") as handle:
            payload = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ValueError(f"failed to load canonical reference manifest: {error}") from error
    if not isinstance(payload, dict):
        raise ValueError("canonical reference manifest must be an object")
    return payload


def canonical_active_reference(manifest: dict[str, Any]) -> dict[str, str]:
    version = manifest.get("rns_reference_version")
    revision = manifest.get("rns_reference_revision")
    python_reference = manifest.get("python_reference")
    if not isinstance(version, str) or not isinstance(revision, str):
        raise ValueError("canonical RNS baseline version and revision are required")
    if not isinstance(python_reference, dict):
        raise ValueError("canonical python_reference section is required")
    if python_reference.get("version") != version or python_reference.get("revision") != revision:
        raise ValueError("canonical RNS baseline and python_reference pins differ")
    if FULL_REVISION.fullmatch(revision) is None:
        raise ValueError(f"canonical RNS baseline revision is not a full Git commit: {revision}")
    return {"version": version, "revision": revision}


def canonical_parity_target(manifest: dict[str, Any]) -> dict[str, Any]:
    target = manifest.get("parity_target")
    if not isinstance(target, dict):
        raise ValueError("canonical parity_target section is required")
    required_strings = ("implementation", "repository", "version", "revision", "scope")
    if any(not isinstance(target.get(key), str) or not target[key] for key in required_strings):
        raise ValueError("parity_target contains a missing string field")
    if target["implementation"] != "Reticulum-Python":
        raise ValueError("parity_target implementation must be Reticulum-Python")
    if target["repository"] != "https://github.com/markqvist/Reticulum.git":
        raise ValueError("parity_target repository is not the canonical Reticulum repository")
    if FULL_REVISION.fullmatch(target["revision"]) is None:
        raise ValueError(f"parity target revision is not a full Git commit: {target['revision']}")
    if target.get("owner_issue") != 605:
        raise ValueError("parity_target owner_issue must be issue 605")
    return {key: target[key] for key in (*required_strings, "owner_issue")}


def validate_behavioral_contract(
    contract: Any,
    *,
    baseline: dict[str, str] | None = None,
    target: dict[str, Any] | None = None,
    reference_revisions: dict[str, Any] | None = None,
    artifact_root: Path | None = None,
    require_references: bool = False,
    require_complete: bool = False,
) -> list[str]:
    errors: list[str] = []
    if not isinstance(contract, dict):
        return ["behavioral contract must be an object"]
    if contract.get("schema_version") != 1:
        errors.append("behavioral contract schema_version must be 1")
    if not isinstance(contract.get("scope"), str) or not contract["scope"]:
        errors.append("behavioral contract scope must be a non-empty string")
    coverage_status = contract.get("coverage_status")
    if coverage_status not in VALID_BEHAVIORAL_COVERAGE:
        errors.append(f"behavioral contract has invalid coverage_status: {coverage_status!r}")

    reference = contract.get("reference")
    if reference is not None:
        if not isinstance(reference, dict):
            errors.append("behavioral contract reference must be an object")
        elif target is not None:
            reticulum = reference.get("reticulum")
            if not isinstance(reticulum, dict):
                errors.append("behavioral contract reference is missing reticulum")
            elif (
                reticulum.get("version") != target["version"]
                or reticulum.get("revision") != target["revision"]
            ):
                errors.append("behavioral contract reference does not match parity_target")
            active_baseline = reference.get("active_baseline")
            if active_baseline is not None and active_baseline != baseline:
                errors.append("behavioral contract active_baseline does not match the manifest")

    requirements = contract.get("requirements")
    if not isinstance(requirements, list) or not requirements:
        errors.append("behavioral contract requirements must be a non-empty list")
        return errors

    seen: set[str] = set()
    for requirement in requirements:
        if not isinstance(requirement, dict):
            errors.append("behavioral contract requirement must be an object")
            continue
        requirement_id = requirement.get("id")
        if not isinstance(requirement_id, str) or not requirement_id:
            errors.append("behavioral contract requirement is missing id")
            continue
        if requirement_id in seen:
            errors.append(f"duplicate behavioral requirement: {requirement_id}")
        seen.add(requirement_id)

        if requirement.get("kind") != "behavioral-requirement":
            errors.append(f"{requirement_id}: kind must be behavioral-requirement")
        project = requirement.get("reference_project")
        if project not in VALID_REFERENCE_PROJECTS:
            errors.append(f"{requirement_id}: invalid reference_project {project!r}")
        for field in ("reference_paths", "rust_surface", "evidence"):
            values = requirement.get(field)
            if not isinstance(values, list) or not values or any(
                not isinstance(value, str) or not value for value in values
            ):
                errors.append(f"{requirement_id}: {field} must be a non-empty string list")
        evidence = requirement.get("evidence")
        if isinstance(evidence, list):
            invalid_evidence = sorted(set(evidence) - VALID_BEHAVIORAL_EVIDENCE)
            if invalid_evidence:
                errors.append(f"{requirement_id}: invalid evidence values {invalid_evidence}")
        evidence_status = requirement.get("evidence_status")
        if evidence_status not in VALID_BEHAVIORAL_EVIDENCE_STATUS:
            errors.append(f"{requirement_id}: invalid evidence_status {evidence_status!r}")
        test_command = requirement.get("test_command")
        if not (
            isinstance(test_command, str)
            and test_command
            or isinstance(test_command, list)
            and test_command
            and all(isinstance(value, str) and value for value in test_command)
        ):
            errors.append(f"{requirement_id}: test_command must be a non-empty string or list")
        artifact = requirement.get("evidence_artifact")
        if not (
            isinstance(artifact, str)
            and artifact
            or isinstance(artifact, list)
            and artifact
            and all(isinstance(value, str) and value for value in artifact)
        ):
            errors.append(f"{requirement_id}: evidence_artifact must be a non-empty string or list")
        artifact_values = (
            [artifact]
            if isinstance(artifact, str)
            else artifact
            if isinstance(artifact, list)
            else []
        )
        for artifact_value in artifact_values:
            artifact_path = Path(artifact_value)
            if artifact_path.is_absolute() or ".." in artifact_path.parts:
                errors.append(
                    f"{requirement_id}: evidence_artifact must stay within the repository"
                )
            elif (
                artifact_root is not None
                and evidence_status == "verified"
                and not (artifact_root / artifact_path).is_file()
            ):
                errors.append(
                    f"{requirement_id}: verified evidence artifact is missing: {artifact_value}"
                )
        owner_issue = requirement.get("owner_issue")
        if isinstance(owner_issue, bool) or not isinstance(owner_issue, int) or owner_issue <= 0:
            errors.append(f"{requirement_id}: owner_issue must be a positive integer")
        implementation = requirement.get("implementation")
        if implementation not in VALID_IMPLEMENTATION:
            errors.append(f"{requirement_id}: invalid implementation {implementation!r}")
        if implementation == "not-applicable" and not requirement.get("notes"):
            errors.append(f"{requirement_id}: not-applicable requirements need notes")
        requirement_reference = requirement.get("reference")
        if requirement_reference is None:
            if require_references:
                errors.append(f"{requirement_id}: exact generated reference is missing")
        elif not isinstance(requirement_reference, dict):
            errors.append(f"{requirement_id}: generated reference must be an object")
        else:
            expected_projects = {
                "reticulum": ("revision",),
                "lxmf": ("revision",),
                "both": ("reticulum", "lxmf"),
                "operational": ("reticulum", "lxmf"),
            }[project] if project in VALID_REFERENCE_PROJECTS else ()
            for reference_key in expected_projects:
                if reference_key not in requirement_reference:
                    errors.append(f"{requirement_id}: generated reference is missing {reference_key}")
            if project in {"reticulum", "lxmf"}:
                revision = requirement_reference.get("revision")
                if not isinstance(revision, str) or FULL_REVISION.fullmatch(revision) is None:
                    errors.append(f"{requirement_id}: generated reference revision is invalid")
            else:
                for reference_key in ("reticulum", "lxmf"):
                    nested = requirement_reference.get(reference_key)
                    if not isinstance(nested, dict):
                        continue
                    revision = nested.get("revision")
                    if not isinstance(revision, str) or FULL_REVISION.fullmatch(revision) is None:
                        errors.append(
                            f"{requirement_id}: generated {reference_key} revision is invalid"
                        )
            if target is not None and project in {"reticulum", "both", "operational"}:
                reticulum_reference = (
                    requirement_reference
                    if project == "reticulum"
                    else requirement_reference.get("reticulum")
                )
                if isinstance(reticulum_reference, dict) and (
                    reticulum_reference.get("revision") != target["revision"]
                    or reticulum_reference.get("version") != target["version"]
                ):
                    errors.append(f"{requirement_id}: generated Reticulum reference is stale")
            if reference_revisions is not None and project in {"lxmf", "both", "operational"}:
                lxmf_reference = (
                    requirement_reference
                    if project == "lxmf"
                    else requirement_reference.get("lxmf")
                )
                if isinstance(lxmf_reference, dict) and lxmf_reference.get("revision") != reference_revisions.get("lxmf"):
                    errors.append(f"{requirement_id}: generated LXMF reference is stale")
        if evidence_status == "verified" and isinstance(evidence, list) and "planned" in evidence:
            errors.append(f"{requirement_id}: verified evidence cannot remain planned")
        if implementation == "complete" and evidence_status != "verified":
            errors.append(f"{requirement_id}: complete implementation requires verified evidence")

    if coverage_status == "complete":
        for requirement in requirements:
            if not isinstance(requirement, dict):
                continue
            if requirement.get("implementation") not in {"complete", "not-applicable"}:
                errors.append(
                    f"{requirement.get('id', '<unknown>')}: complete coverage has a non-complete implementation"
                )
            if requirement.get("evidence_status") != "verified":
                errors.append(
                    f"{requirement.get('id', '<unknown>')}: complete coverage has unverified evidence"
                )
    if require_complete and coverage_status != "complete":
        errors.append(f"behavioral contract coverage is {coverage_status!r}, not complete")
    return errors


def public_name(name: str) -> bool:
    return not name.startswith("_")


def module_name(root_name: str, root: Path, path: Path) -> str:
    relative = path.relative_to(root).with_suffix("")
    parts = list(relative.parts)
    if parts[-1] == "__init__":
        parts.pop()
    return ".".join([root_name, *parts])


def scan_root(root_name: str, root: Path) -> list[SurfaceItem]:
    if not root.is_dir():
        raise ValueError(f"Python {root_name} path is not a directory: {root}")

    items: list[SurfaceItem] = []
    for path in sorted(root.rglob("*.py")):
        relative = path.relative_to(root)
        if path.name in EXCLUDED_FILES or EXCLUDED_PARTS.intersection(relative.parts):
            continue
        try:
            tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        except (OSError, SyntaxError) as error:
            raise ValueError(f"failed to parse {path}: {error}") from error

        module = module_name(root_name, root, path)
        source = relative.as_posix()
        for node in tree.body:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and public_name(node.name):
                items.append(SurfaceItem(f"{module}.{node.name}", "function", source))
            elif isinstance(node, ast.ClassDef) and public_name(node.name):
                items.append(SurfaceItem(f"{module}.{node.name}", "class", source))
                for member in node.body:
                    if isinstance(member, (ast.FunctionDef, ast.AsyncFunctionDef)) and public_name(
                        member.name
                    ):
                        items.append(
                            SurfaceItem(
                                f"{module}.{node.name}.{member.name}",
                                "method",
                                source,
                            )
                        )
    return items


def git_revision(path: Path) -> str | None:
    try:
        result = subprocess.run(
            ["git", "-C", str(path), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return None
    return result.stdout.strip()


def load_mapping(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"failed to load mapping {path}: {error}") from error
    if not isinstance(payload, dict) or not isinstance(payload.get("rules"), list):
        raise ValueError("mapping must be an object with a rules array")
    return payload


def matching_rule(item_id: str, rules: list[dict[str, Any]]) -> dict[str, Any] | None:
    matches = [rule for rule in rules if fnmatch.fnmatchcase(item_id, str(rule.get("pattern", "")))]
    if not matches:
        return None
    matches.sort(key=lambda rule: len(str(rule["pattern"])), reverse=True)
    if len(matches) > 1 and len(str(matches[0]["pattern"])) == len(str(matches[1]["pattern"])):
        raise ValueError(f"ambiguous mapping rules for {item_id}")
    return matches[0]


def validate_rule(item_id: str, rule: dict[str, Any]) -> None:
    implementation = rule.get("implementation")
    evidence = rule.get("evidence")
    rust_surface = rule.get("rust_surface")
    if implementation not in VALID_IMPLEMENTATION:
        raise ValueError(f"{item_id}: invalid implementation status {implementation!r}")
    if not isinstance(evidence, list) or not evidence:
        raise ValueError(f"{item_id}: evidence must be a non-empty list")
    invalid_evidence = sorted(set(evidence) - VALID_EVIDENCE)
    if invalid_evidence:
        raise ValueError(f"{item_id}: invalid evidence values {invalid_evidence}")
    if not isinstance(rust_surface, list) or not rust_surface:
        raise ValueError(f"{item_id}: rust_surface must be a non-empty list")
    if implementation == "not-applicable" and not rule.get("notes"):
        raise ValueError(f"{item_id}: not-applicable mappings require notes")


def classify_rule_for_candidate(
    rule: dict[str, Any], *, forward_candidate: bool
) -> tuple[str, str | None]:
    """Keep callable mappings provisional when scanning the forward reference."""

    implementation = rule["implementation"]
    notes = rule.get("notes")
    if forward_candidate and implementation == "complete":
        candidate_note = (
            "Forward-target callable classification is provisional; this mapping does not "
            "constitute behavioral evidence."
        )
        notes = f"{notes} {candidate_note}" if notes else candidate_note
        implementation = "partial"
    return implementation, notes


def materialize_behavioral_contract(
    contract: dict[str, Any],
    *,
    baseline: dict[str, str],
    target: dict[str, Any],
    references: dict[str, str],
) -> dict[str, Any]:
    materialized = dict(contract)
    materialized["reference"] = {
        "reticulum": {
            "implementation": target["implementation"],
            "repository": target["repository"],
            "version": target["version"],
            "revision": target["revision"],
        },
        "active_baseline": baseline,
        "lxmf": {"revision": references["lxmf"]},
    }
    requirements: list[dict[str, Any]] = []
    for requirement in contract["requirements"]:
        materialized_requirement = dict(requirement)
        project = requirement["reference_project"]
        if project == "reticulum":
            reference = {
                "version": target["version"],
                "revision": target["revision"],
            }
        elif project == "lxmf":
            reference = {"revision": references["lxmf"]}
        else:
            reference = {
                "reticulum": {
                    "version": target["version"],
                    "revision": target["revision"],
                },
                "lxmf": {"revision": references["lxmf"]},
            }
        materialized_requirement["reference"] = reference
        requirements.append(materialized_requirement)
    materialized["requirements"] = requirements
    return materialized


def build_inventory(args: argparse.Namespace) -> dict[str, Any]:
    if args.python_rns_path is None or args.python_lxmf_path is None:
        raise ValueError("--python-rns-path and --python-lxmf-path are required outside --check mode")

    mapping = load_mapping(args.mapping)
    manifest = load_manifest()
    baseline = canonical_active_reference(manifest)
    target = canonical_parity_target(manifest)
    contract_errors = validate_behavioral_contract(
        mapping.get("behavioral_contract"),
        baseline=baseline,
        target=target,
        artifact_root=ROOT,
    )
    if contract_errors:
        raise ValueError("; ".join(contract_errors))
    rules = mapping["rules"]
    scanned_items = scan_root("RNS", args.python_rns_path) + scan_root(
        "LXMF", args.python_lxmf_path
    )
    scanned_by_id = {item.item_id: item for item in scanned_items}
    manual_items = mapping.get("manual_items", [])
    for manual in manual_items:
        item = SurfaceItem(
            str(manual["id"]),
            str(manual.get("kind", "contract")),
            str(manual.get("source", "manual")),
        )
        if item.item_id in scanned_by_id:
            raise ValueError(f"manual inventory item duplicates scanned callable: {item.item_id}")
        scanned_by_id[item.item_id] = item

    references = {
        "reticulum": git_revision(args.python_rns_path),
        "lxmf": git_revision(args.python_lxmf_path),
    }
    if any(
        not isinstance(revision, str) or FULL_REVISION.fullmatch(revision) is None
        for revision in references.values()
    ):
        raise ValueError("both Python reference paths must resolve to full Git revisions")
    forward_candidate = (
        references["reticulum"] == target["revision"]
        and references["reticulum"] != baseline["revision"]
    )

    entries: list[dict[str, Any]] = []
    for item in sorted(scanned_by_id.values(), key=lambda value: value.item_id):
        rule = matching_rule(item.item_id, rules)
        if rule is None:
            entries.append(
                {
                    "id": item.item_id,
                    "kind": item.kind,
                    "source": item.source,
                    "implementation": "unmapped",
                    "rust_surface": [],
                    "evidence": [],
                }
            )
            continue
        validate_rule(item.item_id, rule)
        implementation, notes = classify_rule_for_candidate(
            rule, forward_candidate=forward_candidate
        )
        entries.append(
            {
                "id": item.item_id,
                "kind": item.kind,
                "source": item.source,
                "implementation": implementation,
                "rust_surface": rule["rust_surface"],
                "evidence": rule["evidence"],
                **({"notes": notes} if notes else {}),
            }
        )

    counts: dict[str, int] = {}
    for entry in entries:
        counts[entry["implementation"]] = counts.get(entry["implementation"], 0) + 1
    behavioral_contract = materialize_behavioral_contract(
        mapping["behavioral_contract"],
        baseline=baseline,
        target=target,
        references=references,
    )
    return {
        "schema_version": 1,
        "inventory_profile": "forward-parity-candidate" if forward_candidate else "active-baseline",
        "references": references,
        "parity_target": target,
        "scope": {
            "includes": "public RNS/LXMF callables and committed manual product contracts",
            "excludes": ["private and dunder callables", "vendor", "_version.py"],
        },
        "summary": {"total": len(entries), **dict(sorted(counts.items()))},
        "items": entries,
        "behavioral_contract": behavioral_contract,
    }


def validate_inventory(
    payload: dict[str, Any],
    require_complete: bool,
    require_behavioral_complete: bool = False,
    expected_baseline: dict[str, str] | None = None,
    expected_target: dict[str, Any] | None = None,
    artifact_root: Path | None = None,
) -> list[str]:
    errors: list[str] = []
    items = payload.get("items")
    if payload.get("schema_version") != 1 or not isinstance(items, list):
        return ["inventory schema is invalid"]
    profile = payload.get("inventory_profile")
    if profile not in {"active-baseline", "forward-parity-candidate"}:
        errors.append("inventory profile is missing or invalid")
    if expected_target is not None and payload.get("parity_target") != expected_target:
        errors.append("inventory parity_target does not match the canonical manifest")
    references = payload.get("references")
    if not isinstance(references, dict):
        errors.append("inventory references are missing")
    else:
        expected_revision = None
        if profile == "active-baseline" and expected_baseline is not None:
            expected_revision = expected_baseline["revision"]
        elif profile == "forward-parity-candidate" and expected_target is not None:
            expected_revision = expected_target["revision"]
        if expected_revision is not None and references.get("reticulum") != expected_revision:
            errors.append("inventory Reticulum reference does not match its declared profile")
    seen: set[str] = set()
    for entry in items:
        if not isinstance(entry, dict):
            errors.append("inventory item must be an object")
            continue
        item_id = entry.get("id")
        if not isinstance(item_id, str) or not item_id:
            errors.append("inventory item is missing id")
            continue
        if item_id in seen:
            errors.append(f"duplicate inventory item: {item_id}")
        seen.add(item_id)
        implementation = entry.get("implementation")
        if implementation not in VALID_IMPLEMENTATION:
            errors.append(f"{item_id}: invalid or unmapped implementation status")
        if not entry.get("rust_surface"):
            errors.append(f"{item_id}: missing Rust surface mapping")
        if not entry.get("evidence"):
            errors.append(f"{item_id}: missing evidence mapping")
        if require_complete and implementation == "partial":
            errors.append(f"{item_id}: partial implementation is not release-complete")
    summary = payload.get("summary")
    if not isinstance(summary, dict):
        errors.append("inventory is missing a summary")
    else:
        actual = {
            "total": summary.get("total", 0),
            "complete": summary.get("complete", 0),
            "partial": summary.get("partial", 0),
            "not-applicable": summary.get("not-applicable", 0),
        }
        try:
            validate_counts("summary", actual)
        except ValueError as error:
            errors.append(str(error))
        if isinstance(items, list):
            try:
                if inventory_counts(items) != actual:
                    errors.append("inventory summary does not match item classifications")
            except ValueError as error:
                errors.append(str(error))

    contract = payload.get("behavioral_contract")
    errors.extend(
        validate_behavioral_contract(
            contract,
            baseline=expected_baseline,
            target=expected_target,
            reference_revisions=references if isinstance(references, dict) else None,
            artifact_root=artifact_root,
            require_references=True,
            require_complete=require_behavioral_complete,
        )
    )
    return errors


def canonical_json(payload: dict[str, Any]) -> str:
    return json.dumps(payload, indent=2, sort_keys=True) + "\n"


def generated_file_matches(path: Path, expected: str) -> bool:
    return path.read_text(encoding="utf-8") == expected


def validate_generated_behavioral_contract(
    payload: dict[str, Any],
    mapping_path: Path,
    *,
    baseline: dict[str, str],
    target: dict[str, Any],
) -> list[str]:
    """Reject a generated inventory whose behavioral contract is stale."""

    mapping = load_mapping(mapping_path)
    mapping_contract = mapping.get("behavioral_contract")
    errors = validate_behavioral_contract(
        mapping_contract,
        baseline=baseline,
        target=target,
        artifact_root=ROOT,
    )
    if errors:
        return [f"behavioral mapping: {error}" for error in errors]

    references = payload.get("references")
    if not isinstance(references, dict) or any(
        not isinstance(references.get(project), str)
        for project in ("reticulum", "lxmf")
    ):
        return ["generated inventory references are missing or invalid"]

    expected = materialize_behavioral_contract(
        mapping_contract,
        baseline=baseline,
        target=target,
        references={"reticulum": references["reticulum"], "lxmf": references["lxmf"]},
    )
    if payload.get("behavioral_contract") != expected:
        return [
            "generated behavioral contract drift: regenerate "
            "the parity inventory from the pinned Python checkouts"
        ]
    return []


def inventory_counts(items: list[dict[str, Any]], prefix: str | None = None) -> dict[str, int]:
    counts: dict[str, int] = {}
    selected = (
        items if prefix is None else [item for item in items if item.get("id", "").startswith(prefix)]
    )
    for item in selected:
        implementation = item.get("implementation")
        if implementation not in VALID_IMPLEMENTATION:
            raise ValueError(f"inventory item has invalid implementation status: {implementation!r}")
        counts[implementation] = counts.get(implementation, 0) + 1
    return {
        "total": len(selected),
        "complete": counts.get("complete", 0),
        "partial": counts.get("partial", 0),
        "not-applicable": counts.get("not-applicable", 0),
    }


def validate_counts(name: str, counts: dict[str, int]) -> None:
    for key in ("total", "complete", "partial", "not-applicable"):
        value = counts.get(key, 0)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise ValueError(f"{name} inventory {key!r} count is invalid")

    classified = counts["complete"] + counts["partial"] + counts["not-applicable"]
    if counts["total"] != classified:
        raise ValueError(
            f"{name} inventory total does not match complete, partial, and "
            "not-applicable counts"
        )


def rust_parity_constants(name: str, counts: dict[str, int]) -> str:
    applicable = counts["complete"] + counts["partial"]
    level = "unknown" if applicable == 0 else ("partial" if counts["partial"] else "complete")
    return (
        f'pub const PYTHON_{name}_PARITY_LEVEL: &str = "{level}";\n'
        f"pub const PYTHON_{name}_PARITY_TOTAL: usize = {counts['total']:_};\n"
        f"pub const PYTHON_{name}_PARITY_COMPLETE: usize = {counts['complete']:_};\n"
        f"pub const PYTHON_{name}_PARITY_PARTIAL: usize = {counts['partial']:_};\n"
        f"pub const PYTHON_{name}_PARITY_NOT_APPLICABLE: usize = "
        f"{counts['not-applicable']:_};\n"
    )


def rust_behavioral_constants(contract: dict[str, Any]) -> str:
    requirements = contract.get("requirements")
    reference = contract.get("reference")
    if not isinstance(requirements, list) or not isinstance(reference, dict):
        raise ValueError("behavioral contract requirements or reference are invalid")
    reticulum = reference.get("reticulum")
    if not isinstance(reticulum, dict):
        raise ValueError("behavioral contract reticulum reference is invalid")
    coverage_status = contract.get("coverage_status")
    if coverage_status not in VALID_BEHAVIORAL_COVERAGE:
        raise ValueError("behavioral contract coverage status is invalid")
    verified = sum(
        1
        for requirement in requirements
        if isinstance(requirement, dict) and requirement.get("evidence_status") == "verified"
    )
    complete = sum(
        1
        for requirement in requirements
        if isinstance(requirement, dict) and requirement.get("implementation") == "complete"
    )
    applicable = sum(
        1
        for requirement in requirements
        if isinstance(requirement, dict)
        and requirement.get("implementation") != "not-applicable"
    )
    partial = sum(
        1
        for requirement in requirements
        if isinstance(requirement, dict) and requirement.get("implementation") == "partial"
    )
    not_applicable = len(requirements) - applicable
    level = "unknown" if not applicable else ("complete" if coverage_status == "complete" else "partial")
    return (
        f'pub const PYTHON_BEHAVIORAL_PARITY_LEVEL: &str = "{level}";\n'
        f'pub const PYTHON_BEHAVIORAL_PARITY_COVERAGE_STATUS: &str = "{coverage_status}";\n'
        f"pub const PYTHON_BEHAVIORAL_PARITY_REQUIREMENTS: usize = {len(requirements)};\n"
        f"pub const PYTHON_BEHAVIORAL_PARITY_COMPLETE: usize = {complete};\n"
        f"pub const PYTHON_BEHAVIORAL_PARITY_PARTIAL: usize = {partial};\n"
        f"pub const PYTHON_BEHAVIORAL_PARITY_NOT_APPLICABLE: usize = {not_applicable};\n"
        f"pub const PYTHON_BEHAVIORAL_PARITY_VERIFIED: usize = {verified};\n"
        f"pub const PYTHON_BEHAVIORAL_PARITY_APPLICABLE: usize = {applicable};\n"
        f'pub const PYTHON_BEHAVIORAL_PARITY_REFERENCE_VERSION: &str = "{reticulum["version"]}";\n'
        f'pub const PYTHON_BEHAVIORAL_PARITY_REFERENCE_REF: &str = "{reticulum["revision"]}";'
    )


def render_rust_parity(payload: dict[str, Any]) -> str:
    items = payload.get("items")
    summary = payload.get("summary")
    behavioral_contract = payload.get("behavioral_contract")
    if (
        not isinstance(items, list)
        or not isinstance(summary, dict)
        or not isinstance(behavioral_contract, dict)
    ):
        raise ValueError("inventory items or summary are invalid")

    overall = inventory_counts(items)
    reticulum = inventory_counts(items, "RNS.")
    lxmf = inventory_counts(items, "LXMF.")
    provenance = inventory_counts(items, "CRNS.")
    for name, counts in (
        ("overall", overall),
        ("Reticulum", reticulum),
        ("LXMF", lxmf),
        ("CRNS provenance", provenance),
    ):
        validate_counts(name, counts)

    known_total = reticulum["total"] + lxmf["total"] + provenance["total"]
    if known_total != overall["total"]:
        raise ValueError("inventory contains items outside RNS, LXMF, and CRNS groups")
    for key in ("complete", "partial", "not-applicable"):
        grouped = reticulum[key] + lxmf[key] + provenance[key]
        if grouped != overall[key]:
            raise ValueError(f"inventory grouped {key!r} count does not match overall count")

    expected_summary = {
        "total": summary.get("total", 0),
        "complete": summary.get("complete", 0),
        "partial": summary.get("partial", 0),
        "not-applicable": summary.get("not-applicable", 0),
    }
    validate_counts("summary", expected_summary)
    if overall != expected_summary:
        raise ValueError("inventory summary does not match item classifications")

    return (
        "// This file is generated by tools/scripts/python_surface_inventory.py.\n"
        "// Do not edit manually.\n\n"
        f"{rust_parity_constants('SOFTWARE', overall)}\n"
        f"{rust_parity_constants('RETICULUM', reticulum)}\n"
        f"{rust_parity_constants('LXMF', lxmf)}\n"
        f"{rust_behavioral_constants(behavioral_contract)}"
    )


def run_generator_self_tests() -> None:
    def expect(condition: bool, message: str) -> None:
        if not condition:
            raise ValueError(f"generator self-test failed: {message}")

    items = [
        {"id": "RNS.complete", "implementation": "complete"},
        {"id": "RNS.partial", "implementation": "partial"},
        {"id": "LXMF.complete", "implementation": "complete"},
        {"id": "CRNS.provenance", "implementation": "not-applicable"},
    ]
    expect(
        inventory_counts(items)
        == {"total": 4, "complete": 2, "partial": 1, "not-applicable": 1},
        "overall grouping",
    )
    expect(
        inventory_counts(items, "RNS.")
        == {"total": 2, "complete": 1, "partial": 1, "not-applicable": 0},
        "Reticulum grouping",
    )
    expect(
        inventory_counts(items, "LXMF.")
        == {"total": 1, "complete": 1, "partial": 0, "not-applicable": 0},
        "LXMF grouping",
    )

    behavioral_contract = {
        "schema_version": 1,
        "scope": "self-test behavioral contract",
        "coverage_status": "incomplete",
        "reference": {
            "reticulum": {"version": "1.5.4-dev", "revision": "a" * 40}
        },
        "requirements": [
            {
                "id": "self-test.partial",
                "kind": "behavioral-requirement",
                "reference_project": "reticulum",
                "reference_paths": ["RNS/Transport.py"],
                "rust_surface": ["reticulum-rs-transport"],
                "implementation": "partial",
                "evidence": ["planned"],
                "evidence_status": "unverified",
                "test_command": "cargo test -p reticulum-rs-transport",
                "evidence_artifact": "target/self-test.json",
                "owner_issue": 605,
            },
            {
                "id": "self-test.na",
                "kind": "behavioral-requirement",
                "reference_project": "operational",
                "reference_paths": ["RNS/Interfaces/Interface.py"],
                "rust_surface": ["docs/status"],
                "implementation": "not-applicable",
                "evidence": ["planned"],
                "evidence_status": "unverified",
                "test_command": "documented workflow",
                "evidence_artifact": "target/self-test.json",
                "owner_issue": 616,
                "notes": "Operational validation is a separate evidence axis.",
            },
        ],
    }
    expect(not validate_behavioral_contract(behavioral_contract), "behavioral contract schema")
    behavioral_rust = rust_behavioral_constants(behavioral_contract)
    expect(
        "PYTHON_BEHAVIORAL_PARITY_PARTIAL: usize = 1" in behavioral_rust
        and "PYTHON_BEHAVIORAL_PARITY_NOT_APPLICABLE: usize = 1" in behavioral_rust,
        "behavioral Rust advisory counts derive from contract classifications",
    )
    malformed_contract = dict(behavioral_contract)
    malformed_contract["requirements"] = [{"id": "broken"}]
    expect(validate_behavioral_contract(malformed_contract), "malformed behavioral contract")
    contradictory_contract = json.loads(json.dumps(behavioral_contract))
    contradictory_contract["coverage_status"] = "complete"
    expect(
        validate_behavioral_contract(contradictory_contract),
        "contradictory complete coverage claim",
    )
    hardware_contract = json.loads(json.dumps(behavioral_contract))
    hardware_contract["coverage_status"] = "hardware-unverified"
    hardware_contract["requirements"][0]["evidence_status"] = "hardware-unverified"
    expect(
        not validate_behavioral_contract(hardware_contract),
        "hardware-unverified evidence status",
    )
    blocked_contract = json.loads(json.dumps(behavioral_contract))
    blocked_contract["coverage_status"] = "blocked"
    blocked_contract["requirements"][0]["evidence_status"] = "blocked"
    expect(not validate_behavioral_contract(blocked_contract), "blocked evidence status")
    wildcard_rule = {
        "pattern": "RNS.*",
        "implementation": "complete",
        "rust_surface": ["reticulum-rs"],
        "evidence": ["unit"],
    }
    validate_rule("RNS.future_callable", wildcard_rule)
    candidate_implementation, candidate_notes = classify_rule_for_candidate(
        wildcard_rule, forward_candidate=True
    )
    expect(candidate_implementation == "partial", "forward wildcard remains provisional")
    expect(candidate_notes and "provisional" in candidate_notes, "forward wildcard is explained")
    baseline_implementation, _ = classify_rule_for_candidate(
        wildcard_rule, forward_candidate=False
    )
    expect(baseline_implementation == "complete", "active baseline keeps historical mapping")

    baseline = {"version": "1.5.2", "revision": "c" * 40}
    target = {
        "implementation": "Reticulum-Python",
        "repository": "https://github.com/markqvist/Reticulum.git",
        "version": "1.5.4-dev",
        "revision": "a" * 40,
    }
    references = {"reticulum": "a" * 40, "lxmf": "b" * 40}
    with tempfile.TemporaryDirectory(prefix="python-surface-mapping-") as temp_dir:
        mapping_path = Path(temp_dir) / "mapping.json"
        mapping_path.write_text(
            json.dumps({"rules": [], "behavioral_contract": behavioral_contract}),
            encoding="utf-8",
        )
        materialized_contract = materialize_behavioral_contract(
            behavioral_contract,
            baseline=baseline,
            target=target,
            references=references,
        )
        generated_payload = {
            "references": references,
            "behavioral_contract": materialized_contract,
        }
        expect(
            not validate_generated_behavioral_contract(
                generated_payload, mapping_path, baseline=baseline, target=target
            ),
            "generated behavioral contract matches mapping",
        )
        generated_payload["behavioral_contract"] = dict(materialized_contract)
        generated_payload["behavioral_contract"]["coverage_status"] = "complete"
        expect(
            validate_generated_behavioral_contract(
                generated_payload, mapping_path, baseline=baseline, target=target
            ),
            "generated behavioral contract drift",
        )
        stale_payload = {
            "references": references,
            "behavioral_contract": json.loads(json.dumps(materialized_contract)),
        }
        stale_payload["behavioral_contract"]["requirements"][0]["reference"]["revision"] = "d" * 40
        expect(
            validate_generated_behavioral_contract(
                stale_payload, mapping_path, baseline=baseline, target=target
            ),
            "stale generated reference",
        )

        verified_contract = json.loads(json.dumps(materialized_contract))
        verified_requirement = verified_contract["requirements"][0]
        verified_requirement["evidence"] = ["unit"]
        verified_requirement["evidence_status"] = "verified"
        expect(
            validate_behavioral_contract(
                verified_contract,
                baseline=baseline,
                target=target,
                artifact_root=Path(temp_dir),
            ),
            "missing verified evidence artifact",
        )
        artifact_path = Path(temp_dir) / "target" / "self-test.json"
        artifact_path.parent.mkdir(parents=True)
        artifact_path.write_text("{}\n", encoding="utf-8")
        expect(
            not validate_behavioral_contract(
                verified_contract,
                baseline=baseline,
                target=target,
                artifact_root=Path(temp_dir),
            ),
            "verified evidence artifact exists",
        )

        unmapped_payload = {
            "schema_version": 1,
            "inventory_profile": "forward-parity-candidate",
            "references": references,
            "parity_target": target,
            "summary": {"total": 1, "complete": 0, "partial": 0, "not-applicable": 0},
            "items": [
                {
                    "id": "RNS.new_callable",
                    "kind": "function",
                    "source": "extra.py",
                    "implementation": "unmapped",
                    "rust_surface": [],
                    "evidence": [],
                }
            ],
            "behavioral_contract": materialized_contract,
        }
        expect(
            validate_inventory(
                unmapped_payload,
                False,
                expected_baseline=baseline,
                expected_target=target,
            ),
            "unmapped target callable",
        )
        expanded_payload = json.loads(json.dumps(unmapped_payload))
        expanded_payload["items"][0].update(
            {
                "implementation": "partial",
                "rust_surface": ["reticulum-rs"],
                "evidence": ["unit"],
            }
        )
        expanded_payload["summary"] = {
            "total": 1,
            "complete": 0,
            "partial": 1,
            "not-applicable": 0,
        }
        expect(
            not validate_inventory(
                expanded_payload,
                False,
                expected_baseline=baseline,
                expected_target=target,
            ),
            "inventory accepts a variable item count",
        )
        expanded_payload["items"].append(
            {
                "id": "RNS.another_callable",
                "kind": "function",
                "source": "extra.py",
                "implementation": "partial",
                "rust_surface": ["reticulum-rs"],
                "evidence": ["unit"],
            }
        )
        expanded_payload["summary"]["total"] = 2
        expanded_payload["summary"]["partial"] = 2
        expect(
            not validate_inventory(
                expanded_payload,
                False,
                expected_baseline=baseline,
                expected_target=target,
            ),
            "inventory count is derived from items",
        )

    rendered = render_rust_parity(
        {
            "items": items,
            "summary": {
                "total": 4,
                "complete": 2,
                "partial": 1,
                "not-applicable": 1,
            },
            "behavioral_contract": behavioral_contract,
        }
    )
    expect(
        'PYTHON_SOFTWARE_PARITY_LEVEL: &str = "partial"' in rendered,
        "overall partial level",
    )
    expect(
        'PYTHON_RETICULUM_PARITY_LEVEL: &str = "partial"' in rendered,
        "Reticulum partial level",
    )
    expect(
        'PYTHON_LXMF_PARITY_LEVEL: &str = "complete"' in rendered,
        "LXMF complete level",
    )
    expect(
        'PYTHON_EMPTY_PARITY_LEVEL: &str = "unknown"'
        in rust_parity_constants(
            "EMPTY",
            {"total": 1, "complete": 0, "partial": 0, "not-applicable": 1},
        ),
        "zero-applicable unknown level",
    )
    expect(
        'PYTHON_BEHAVIORAL_PARITY_COVERAGE_STATUS: &str = "incomplete"' in rendered,
        "behavioral incomplete level",
    )
    try:
        render_rust_parity(
            {
                "items": [{"id": "LXMF.complete", "implementation": "complete"}],
                "summary": {
                    "total": True,
                    "complete": 1,
                    "partial": 0,
                    "not-applicable": 0,
                },
            }
        )
    except ValueError:
        pass
    else:
        raise ValueError("generator self-test failed: malformed summary validation")

    with tempfile.TemporaryDirectory(prefix="python-surface-inventory-") as temp_dir:
        generated = Path(temp_dir) / "generated.rs"
        generated.write_text(rendered, encoding="utf-8")
        expect(generated_file_matches(generated, rendered), "generated file match")
        generated.write_text(rendered + "// drift\n", encoding="utf-8")
        expect(not generated_file_matches(generated, rendered), "generated file drift")


def main() -> int:
    args = parse_args()
    try:
        if args.self_test:
            run_generator_self_tests()
            print("python-surface-inventory: self-test ok")
            return 0
        manifest = load_manifest()
        expected_baseline = canonical_active_reference(manifest)
        expected_target = canonical_parity_target(manifest)
        generated_contract_errors: list[str] = []
        if args.check and args.python_rns_path is None and args.python_lxmf_path is None:
            payload = json.loads(args.json_out.read_text(encoding="utf-8"))
            generated_contract_errors = validate_generated_behavioral_contract(
                payload,
                args.mapping,
                baseline=expected_baseline,
                target=expected_target,
            )
        else:
            payload = build_inventory(args)
            rendered = canonical_json(payload)
            if args.check:
                if not generated_file_matches(args.json_out, rendered):
                    print(f"inventory drift: regenerate {args.json_out}", file=sys.stderr)
                    return 1
        errors = validate_inventory(
            payload,
            args.require_complete,
            args.require_behavioral_complete,
            expected_baseline,
            expected_target,
            ROOT,
        )
        errors = generated_contract_errors + errors
        if not errors:
            rust_parity = render_rust_parity(payload)
            if args.check:
                if not generated_file_matches(args.rust_out, rust_parity):
                    print(
                        f"inventory drift: regenerate {args.rust_out}",
                        file=sys.stderr,
                    )
                    return 1
            else:
                args.json_out.parent.mkdir(parents=True, exist_ok=True)
                with args.json_out.open("w", encoding="utf-8", newline="\n") as output:
                    output.write(rendered)
                args.rust_out.parent.mkdir(parents=True, exist_ok=True)
                with args.rust_out.open("w", encoding="utf-8", newline="\n") as output:
                    output.write(rust_parity)
    except (KeyError, TypeError, ValueError, OSError, json.JSONDecodeError) as error:
        print(f"python-surface-inventory: {error}", file=sys.stderr)
        return 1

    if errors:
        for error in errors:
            print(f"python-surface-inventory: {error}", file=sys.stderr)
        return 1
    summary = payload.get("summary", {})
    print(
        "python-surface-inventory: ok "
        f"total={summary.get('total', len(payload.get('items', [])))} "
        f"complete={summary.get('complete', 0)} "
        f"partial={summary.get('partial', 0)} "
        f"not-applicable={summary.get('not-applicable', 0)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
