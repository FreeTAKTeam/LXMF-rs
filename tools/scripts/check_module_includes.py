#!/usr/bin/env python3
"""Ratchet `include!` module fragments so per-file size limits cannot be bypassed.

A root is a Rust file that includes `.rs` fragments without being included
itself; its effective size is its own lines plus every fragment it reaches.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

SCAN_DIRS = ("crates/libs", "crates/apps", "xtask/src")
BASELINE = "docs/architecture/include-fragment-baseline.txt"
PROD_LIMIT = 500
INCLUDE = re.compile(r'(?<![\w])include!\(\s*"([^"]+\.rs)"\s*\)')


def includes_of(path: Path) -> list[Path]:
    found = []
    for line in path.read_text().splitlines():
        if line.lstrip().startswith("//"):
            continue
        for target in INCLUDE.findall(line):
            fragment = (path.parent / target).resolve()
            if not fragment.is_file():
                raise ValueError(f"{path}: include! target not found: {target}")
            found.append(fragment)
    return found


def scan(root: Path) -> dict[str, tuple[int, int]]:
    """Map each root's repo-relative path to (fragment count, effective lines)."""
    edges = {}
    for scan_dir in SCAN_DIRS:
        for path in sorted((root / scan_dir).rglob("*.rs")):
            edges[path.resolve()] = includes_of(path)
    included = {fragment for fragments in edges.values() for fragment in fragments}
    roots = {}
    for path, fragments in edges.items():
        if not fragments or path in included:
            continue
        seen, stack = set(), list(fragments)
        while stack:
            fragment = stack.pop()
            if fragment not in seen:
                seen.add(fragment)
                stack.extend(edges.get(fragment) or includes_of(fragment))
        lines = sum(len(p.read_text().splitlines()) for p in seen | {path})
        roots[path.relative_to(root.resolve()).as_posix()] = (len(seen), lines)
    return roots


def read_baseline(path: Path) -> dict[str, int]:
    baseline = {}
    for line in path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            name, count = line.split()
            baseline[name] = int(count)
    return baseline


def violations(roots: dict[str, tuple[int, int]], baseline: dict[str, int]) -> list[str]:
    found = []
    for name, (count, _) in sorted(roots.items()):
        if name not in baseline:
            found.append(f"{name}: new include! of .rs fragments; use `mod` instead")
        elif count > baseline[name]:
            found.append(f"{name}: {count} fragments > baseline {baseline[name]}; use `mod` instead")
    for name in sorted(baseline.keys() - roots.keys()):
        found.append(f"{name}: no longer includes fragments; remove it from the baseline")
    return found


def write_baseline(path: Path, roots: dict[str, tuple[int, int]], baseline: dict[str, int]) -> None:
    grown = [v for v in violations(roots, baseline) if "no longer" not in v]
    if grown:
        raise ValueError("refusing to raise the baseline:\n" + "\n".join(grown))
    body = "".join(f"{name} {count}\n" for name, (count, _) in sorted(roots.items()))
    path.write_text(
        "# Files still split into include! fragments, with their fragment count.\n"
        "# Counts may only shrink; regenerate with check_module_includes.py --write-baseline.\n"
        + body
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--write-baseline", action="store_true")
    args = parser.parse_args()
    baseline_path = args.root / BASELINE
    roots = scan(args.root)
    if args.write_baseline and not baseline_path.exists():
        baseline = {name: count for name, (count, _) in roots.items()}
    else:
        baseline = read_baseline(baseline_path)
    if args.write_baseline:
        write_baseline(baseline_path, roots, baseline)
        print(f"wrote {baseline_path.relative_to(args.root)} ({len(roots)} roots)")
        return 0
    largest = sorted(roots.items(), key=lambda item: -item[1][1])[:15]
    print(f"include! fragment roots: {len(roots)}; largest effective modules (limit {PROD_LIMIT}):")
    for name, (count, lines) in largest:
        print(f"  {lines:6} lines  {count:3} fragments  {name}")
    found = violations(roots, baseline)
    if found:
        print("\n".join(found), file=sys.stderr)
        return 1
    print("include! fragment ratchet: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
