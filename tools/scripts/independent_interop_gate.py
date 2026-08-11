#!/usr/bin/env python3
"""Fail independent interop CI on new gaps while allowing confirmed peer divergences."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


RNS_RS_ALLOWED = {
    ("path request and establishment", "LXMF-rs -> rns-rs", "FAIL", "peer_divergence"),
    ("Channel ordered exchange and proof", "bidirectional", "FAIL", "peer_divergence"),
    ("Link establishment proof RTT", "LXMF-rs -> rns-rs", "FAIL", "peer_divergence"),
    ("Link teardown", "rns-rs -> LXMF-rs", "BLOCKED", "dependency_failed"),
    ("Link teardown", "LXMF-rs -> rns-rs", "BLOCKED", "dependency_failed"),
}

REQUIRED_RNS_RS = {
    "Resource 1 MiB",
    "multi-hop Resource 1 MiB",
    "five-node Resource 1 MiB",
    "interface failure selects viable alternate path",
    "daemon restart preserves client identity and reconnects shared-instance traffic",
    "Resource recovery with 1% deterministic frame loss",
    "Resource terminal timeout under complete frame loss",
}

REQUIRED_RETICULUM_GO = {
    "Link data",
    "Resource 1 MiB",
    "Resource 50 MiB",
    "multi-hop encrypted packet and delivery proof",
    "multi-hop Resource 1 MiB",
}


def validate(report: dict[str, Any]) -> list[str]:
    peer = report.get("peer", {}).get("implementation")
    scenarios = report.get("scenarios")
    if not isinstance(scenarios, list) or not scenarios:
        return ["report contains no scenarios"]
    errors: list[str] = []
    if peer == "rns-rs":
        for row in scenarios:
            if row.get("status") == "PASS":
                continue
            key = (
                row.get("scenario"),
                row.get("direction"),
                row.get("status"),
                row.get("classification"),
            )
            if key in RNS_RS_ALLOWED and row.get("failure_owner") == "rns-rs":
                continue
            else:
                errors.append(f"unexpected rns-rs result: {key}: {row.get('failure_reason')}")
        names = {str(row.get("scenario")) for row in scenarios if row.get("status") == "PASS"}
        for required in sorted(REQUIRED_RNS_RS - names):
            errors.append(f"required passing rns-rs scenario missing: {required}")
    elif peer == "Reticulum-Go":
        for row in scenarios:
            if row.get("status") not in {"PASS", "UNSUPPORTED"}:
                errors.append(
                    f"unexpected Reticulum-Go result: {row.get('scenario')}: {row.get('status')}"
                )
        names = {str(row.get("scenario")) for row in scenarios if row.get("status") == "PASS"}
        for required in sorted(REQUIRED_RETICULUM_GO - names):
            errors.append(f"required passing Reticulum-Go scenario missing: {required}")
    else:
        errors.append(f"unknown peer implementation: {peer!r}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    report = json.loads(args.report.read_text(encoding="utf-8"))
    errors = validate(report)
    if errors:
        for error in errors:
            print(f"independent_interop_gate: {error}")
        return 1
    print("independent interop gate: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
