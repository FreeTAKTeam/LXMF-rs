#!/usr/bin/env python3
"""Probe the pinned Python Reticulum Resolver behavior."""

from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path


def load_resolver(python_rns_path: Path):
    resolver_path = python_rns_path / "RNS" / "Resolver.py"
    if not resolver_path.is_file():
        raise FileNotFoundError(f"Python Resolver.py not found: {resolver_path}")
    spec = importlib.util.spec_from_file_location("pinned_rns_resolver", resolver_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"unable to load Python Resolver.py: {resolver_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.Resolver, resolver_path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--python-rns-path", type=Path, required=True)
    parser.add_argument("--json-out", type=Path)
    args = parser.parse_args()

    resolver, resolver_path = load_resolver(args.python_rns_path.resolve())
    cases = ["example.destination", "", None]
    results = [resolver.resolve_identity(case) for case in cases]
    if results != [None, None, None]:
        raise AssertionError(f"unexpected pinned Resolver behavior: {results!r}")

    report = {
        "status": "pass",
        "reference_file": str(resolver_path),
        "surface": "RNS.Resolver.resolve_identity",
        "cases": len(cases),
        "result": "none",
    }
    encoded = json.dumps(report, sort_keys=True)
    print(encoded)
    if args.json_out is not None:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(encoded + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
