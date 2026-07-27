#!/usr/bin/env python3
"""Build and validate the pinned Python RNS/LXMF public surface inventory."""

from __future__ import annotations

import argparse
import ast
import fnmatch
import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


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
    parser.add_argument("--require-complete", action="store_true")
    return parser.parse_args()


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


def build_inventory(args: argparse.Namespace) -> dict[str, Any]:
    if args.python_rns_path is None or args.python_lxmf_path is None:
        raise ValueError("--python-rns-path and --python-lxmf-path are required outside --check mode")

    mapping = load_mapping(args.mapping)
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
        entries.append(
            {
                "id": item.item_id,
                "kind": item.kind,
                "source": item.source,
                "implementation": rule["implementation"],
                "rust_surface": rule["rust_surface"],
                "evidence": rule["evidence"],
                **({"notes": rule["notes"]} if rule.get("notes") else {}),
            }
        )

    counts: dict[str, int] = {}
    for entry in entries:
        counts[entry["implementation"]] = counts.get(entry["implementation"], 0) + 1
    return {
        "schema_version": 1,
        "references": {
            "reticulum": git_revision(args.python_rns_path),
            "lxmf": git_revision(args.python_lxmf_path),
        },
        "scope": {
            "includes": "public RNS/LXMF callables and committed manual product contracts",
            "excludes": ["private and dunder callables", "vendor", "_version.py"],
        },
        "summary": {"total": len(entries), **dict(sorted(counts.items()))},
        "items": entries,
    }


def validate_inventory(payload: dict[str, Any], require_complete: bool) -> list[str]:
    errors: list[str] = []
    items = payload.get("items")
    if payload.get("schema_version") != 1 or not isinstance(items, list):
        return ["inventory schema is invalid"]
    seen: set[str] = set()
    for entry in items:
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
    return errors


def canonical_json(payload: dict[str, Any]) -> str:
    return json.dumps(payload, indent=2, sort_keys=True) + "\n"


def render_rust_parity(payload: dict[str, Any]) -> str:
    summary = payload.get("summary")
    if not isinstance(summary, dict):
        raise ValueError("inventory summary is invalid")

    counts: dict[str, int] = {}
    for key in ("total", "complete", "partial", "not-applicable"):
        value = summary.get(key, 0)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise ValueError(f"inventory summary {key!r} count is invalid")
        counts[key] = value

    classified = counts["complete"] + counts["partial"] + counts["not-applicable"]
    if counts["total"] != classified:
        raise ValueError(
            "inventory summary total does not match complete, partial, and "
            "not-applicable counts"
        )

    level = "partial" if counts["partial"] else "complete"
    return (
        "// This file is generated by tools/scripts/python_surface_inventory.py.\n"
        "// Do not edit manually.\n\n"
        f'pub const PYTHON_SOFTWARE_PARITY_LEVEL: &str = "{level}";\n'
        f"pub const PYTHON_SOFTWARE_PARITY_TOTAL: usize = {counts['total']:_};\n"
        f"pub const PYTHON_SOFTWARE_PARITY_COMPLETE: usize = {counts['complete']:_};\n"
        f"pub const PYTHON_SOFTWARE_PARITY_PARTIAL: usize = {counts['partial']:_};\n"
        "pub const PYTHON_SOFTWARE_PARITY_NOT_APPLICABLE: usize = "
        f"{counts['not-applicable']:_};\n"
    )


def main() -> int:
    args = parse_args()
    try:
        if args.check and args.python_rns_path is None and args.python_lxmf_path is None:
            payload = json.loads(args.json_out.read_text(encoding="utf-8"))
        else:
            payload = build_inventory(args)
            rendered = canonical_json(payload)
            if args.check:
                committed = args.json_out.read_text(encoding="utf-8")
                if committed != rendered:
                    print(f"inventory drift: regenerate {args.json_out}", file=sys.stderr)
                    return 1
        errors = validate_inventory(payload, args.require_complete)
        if not errors:
            rust_parity = render_rust_parity(payload)
            if args.check:
                committed_rust = args.rust_out.read_text(encoding="utf-8")
                if committed_rust != rust_parity:
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
