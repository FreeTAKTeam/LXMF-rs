#!/usr/bin/env python3
"""Fail independent interop CI on new gaps while allowing confirmed peer divergences."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


ScenarioSignature = tuple[str, str, str]


RNS_RS_ALLOWED = {
    ("path request and establishment", "LXMF-rs -> rns-rs", "FAIL", "peer_divergence"),
    ("Channel ordered exchange and proof", "bidirectional", "FAIL", "peer_divergence"),
    ("Link establishment proof RTT", "LXMF-rs -> rns-rs", "FAIL", "peer_divergence"),
    ("Link teardown", "rns-rs -> LXMF-rs", "BLOCKED", "dependency_failed"),
    ("Link teardown", "LXMF-rs -> rns-rs", "BLOCKED", "dependency_failed"),
}

RNS_RS_REQUIRED_PRESENT: set[ScenarioSignature] = {
    ("two-node", "LXMF-rs -> rns-rs", "path request and establishment"),
    ("two-node", "bidirectional", "Channel ordered exchange and proof"),
    ("two-node", "LXMF-rs -> rns-rs", "Link establishment proof RTT"),
    ("two-node", "rns-rs -> LXMF-rs", "Link teardown"),
    ("two-node", "LXMF-rs -> rns-rs", "Link teardown"),
}

RNS_RS_REQUIRED_PR_PASS: set[ScenarioSignature] = {
    ("two-node", "bidirectional", "path unknown"),
    ("two-node", "rns-rs -> LXMF-rs", "path request and establishment"),
    ("two-node", "bidirectional", "announce identity signature and app-data"),
    ("two-node", "bidirectional", "cached path"),
    ("two-node", "rns-rs -> LXMF-rs", "encrypted data packet and delivery proof"),
    ("two-node", "LXMF-rs -> rns-rs", "encrypted data packet and delivery proof"),
    ("two-node", "rns-rs -> LXMF-rs", "Link establishment proof RTT"),
    ("two-node", "LXMF-rs -> rns-rs", "packet-sized request response and correlation"),
    ("two-node", "rns-rs -> LXMF-rs", "packet-sized request response and correlation"),
    ("two-node", "LXMF-rs -> rns-rs -> LXMF-rs", "compressed resource-sized request response"),
    ("two-node", "rns-rs -> LXMF-rs", "request timeout"),
    ("two-node", "rns-rs receiver -> LXMF-rs sender", "Resource cancellation"),
    ("two-node", "bidirectional", "Link data"),
    ("two-node", "rns-rs -> LXMF-rs", "Resource small"),
    ("two-node", "LXMF-rs -> rns-rs", "Resource small"),
    ("two-node", "rns-rs -> LXMF-rs", "Resource 1 MiB"),
    ("two-node", "LXMF-rs -> rns-rs", "Resource 1 MiB"),
    ("LXMF-rs — LXMF-rs — rns-rs", "bidirectional", "multi-hop announce propagation and path establishment"),
    ("LXMF-rs — LXMF-rs — rns-rs", "rns-rs -> LXMF-rs", "multi-hop encrypted packet and delivery proof"),
    ("LXMF-rs — LXMF-rs — rns-rs", "LXMF-rs -> rns-rs", "multi-hop encrypted packet"),
    ("LXMF-rs — LXMF-rs — rns-rs", "rns-rs -> LXMF-rs", "multi-hop delivery proof"),
    ("LXMF-rs — LXMF-rs — rns-rs", "rns-rs -> LXMF-rs", "multi-hop Link establishment proof RTT"),
    ("LXMF-rs — LXMF-rs — rns-rs", "rns-rs -> LXMF-rs", "multi-hop Resource 1 MiB"),
    ("LXMF-rs — LXMF-rs — rns-rs", "LXMF-rs -> rns-rs", "multi-hop Resource 1 MiB"),
    ("LXMF-rs — rns-rs — LXMF-rs", "bidirectional", "multi-hop announce propagation and path establishment"),
    ("LXMF-rs — rns-rs — LXMF-rs", "left -> right", "multi-hop encrypted packet and delivery proof"),
    ("LXMF-rs — rns-rs — LXMF-rs", "right -> left", "multi-hop encrypted packet and delivery proof"),
    ("LXMF-rs — rns-rs — LXMF-rs", "left -> right", "multi-hop Link establishment proof"),
    ("LXMF-rs — rns-rs — LXMF-rs", "right -> left", "multi-hop Link establishment proof"),
    ("LXMF-rs — rns-rs — LXMF-rs", "left -> right", "multi-hop Resource 1 MiB"),
    ("LXMF-rs — rns-rs — LXMF-rs", "right -> left", "multi-hop Resource 1 MiB"),
    ("A — B — C — D — E (all LXMF-rs)", "bidirectional", "five-node path convergence"),
    ("A — B — C — D — E (all LXMF-rs)", "A -> E", "five-node encrypted packet and proof"),
    ("A — B — C — D — E (all LXMF-rs)", "E -> A", "five-node encrypted packet and proof"),
    ("A — B — C — D — E (all LXMF-rs)", "A -> E", "five-node Link establishment proof"),
    ("A — B — C — D — E (all LXMF-rs)", "A -> E", "five-node Resource 1 MiB"),
    ("A — B — C — D — E (all LXMF-rs)", "E -> A", "five-node Resource 1 MiB"),
    ("A — B — C — D — E (all LXMF-rs)", "C restart", "five-node intermediate C restart"),
    ("A — B — C — D — E (all LXMF-rs)", "A -> E", "five-node path and traffic recovery"),
    ("A — B — C — D — E (mixed LXMF-rs/rns-rs)", "bidirectional", "five-node path convergence"),
    ("A — B — C — D — E (mixed LXMF-rs/rns-rs)", "A -> E", "five-node encrypted packet and proof"),
    ("A — B — C — D — E (mixed LXMF-rs/rns-rs)", "E -> A", "five-node encrypted packet and proof"),
    ("A — B — C — D — E (mixed LXMF-rs/rns-rs)", "A -> E", "five-node Link establishment proof"),
    ("A — B — C — D — E (mixed LXMF-rs/rns-rs)", "A -> E", "five-node Resource 1 MiB"),
    ("A — B — C — D — E (mixed LXMF-rs/rns-rs)", "E -> A", "five-node Resource 1 MiB"),
    ("A — B — C — D — E (mixed LXMF-rs/rns-rs)", "C restart", "five-node intermediate C restart"),
    ("A — B — C — D — E (mixed LXMF-rs/rns-rs)", "A -> E", "five-node path and traffic recovery"),
    ("rns-rs — LXMF-rs transports — LXMF-rs selector", "rns-rs -> LXMF-rs", "same-hop path selection uses higher interface gravity"),
    ("rns-rs — LXMF-rs transports — LXMF-rs selector", "LXMF-rs -> rns-rs", "observable forwarding over gravity-selected path"),
    ("rns-rs — LXMF-rs transports — LXMF-rs selector", "rns-rs -> LXMF-rs", "dynamic path rebalancing after gravity change"),
    ("rns-rs — LXMF-rs transports — LXMF-rs selector", "LXMF-rs -> rns-rs", "observable forwarding after dynamic rebalancing"),
    ("rns-rs — LXMF-rs transports — LXMF-rs selector", "LXMF-rs -> rns-rs", "interface failure selects viable alternate path"),
    ("rns-rs — LXMF-rs transports — LXMF-rs selector", "LXMF-rs -> rns-rs", "communication resumes after interface failure"),
    ("rns-rs — LXMF-rs transports — LXMF-rs selector", "rns-rs -> LXMF-rs", "different hop count takes the shorter route despite higher alternate gravity"),
    ("rns-rs — LXMF-rs transports — LXMF-rs selector", "LXMF-rs -> rns-rs", "observable forwarding over shorter lower-gravity path"),
    ("rns-rs boundary — LXMF-rs — boundary/gateway endpoints", "rns-rs -> LXMF-rs", "boundary path request to boundary interface"),
    ("rns-rs boundary — LXMF-rs — boundary/gateway endpoints", "rns-rs -> LXMF-rs", "boundary path request to gateway interface"),
    ("rns-rs boundary — LXMF-rs — boundary/gateway endpoints", "rns-rs -> LXMF-rs", "boundary path request suppresses full-interface recursion"),
    ("LXMF-rs — rns-rs — LXMF-rs restart topology", "right endpoint", "endpoint restart, rediscovery and identity continuity"),
    ("LXMF-rs — rns-rs — LXMF-rs restart topology", "left -> right", "traffic resumes after endpoint restart"),
    ("LXMF-rs — rns-rs — LXMF-rs restart topology", "rns-rs intermediary", "intermediate transport restart and rediscovery"),
    ("LXMF-rs — rns-rs — LXMF-rs restart topology", "left -> right", "traffic resumes after intermediate restart"),
    ("LXMF-rs — rns-rs — LXMF-rs restart topology", "right -> left", "traffic resumes after intermediate restart"),
    ("rns-rs local client — LXMF-rs reticulumd — LXMF-rs remote endpoint", "bidirectional discovery", "shared daemon starts, local client attaches, and remote peer is discovered"),
    ("rns-rs local client — LXMF-rs reticulumd — LXMF-rs remote endpoint", "LXMF-rs remote -> rns-rs local client", "shared-instance encrypted packet and delivery proof before restart"),
    ("rns-rs local client — LXMF-rs reticulumd — LXMF-rs remote endpoint", "rns-rs local client -> LXMF-rs remote", "shared-instance encrypted packet and delivery proof before restart"),
    ("rns-rs local client — LXMF-rs reticulumd — LXMF-rs remote endpoint", "daemon restart", "daemon restart preserves client identity and reconnects shared-instance traffic"),
    ("rns-rs local client — LXMF-rs reticulumd — LXMF-rs remote endpoint", "LXMF-rs remote -> rns-rs local client", "shared-instance encrypted packet and delivery proof after restart"),
    ("rns-rs local client — LXMF-rs reticulumd — LXMF-rs remote endpoint", "rns-rs local client -> LXMF-rs remote", "shared-instance encrypted packet and delivery proof after restart"),
    ("rns-rs — HDLC fault proxy — LXMF-rs", "rns-rs -> LXMF-rs", "Resource recovery with 1% deterministic frame loss"),
    ("rns-rs — HDLC fault proxy — LXMF-rs", "rns-rs -> LXMF-rs", "Resource terminal timeout under complete frame loss"),
    ("rns-rs — HDLC fault proxy — LXMF-rs", "rns-rs -> LXMF-rs", "packet and proof with 50 ms per-frame latency"),
}

RNS_RS_REQUIRED_EXPANDED_PASS: set[ScenarioSignature] = {
    ("two-node", "rns-rs -> LXMF-rs", "Resource 50 MiB"),
    ("two-node", "LXMF-rs -> rns-rs", "Resource 50 MiB"),
    ("rns-rs — HDLC fault proxy — LXMF-rs", "rns-rs -> LXMF-rs", "Resource recovery with 5% deterministic frame loss"),
    ("rns-rs — HDLC fault proxy — LXMF-rs", "rns-rs -> LXMF-rs", "Resource recovery with 10% deterministic frame loss"),
    ("rns-rs — HDLC fault proxy — LXMF-rs", "rns-rs -> LXMF-rs", "packet and proof with 250 ms per-frame latency"),
    ("rns-rs — HDLC fault proxy — LXMF-rs", "rns-rs -> LXMF-rs", "packet and proof with 500 ms per-frame latency"),
    ("rns-rs — HDLC fault proxy — LXMF-rs", "rns-rs -> LXMF-rs", "Resource robustness with duplicated frames"),
    ("rns-rs — HDLC fault proxy — LXMF-rs", "rns-rs -> LXMF-rs", "Resource robustness with reordered adjacent frames"),
}

RETICULUM_GO_REQUIRED_PASS: set[ScenarioSignature] = {
    ("two-node", "bidirectional", "path unknown, request, response, establishment and cache"),
    ("two-node", "bidirectional", "announce identity signature and app-data"),
    ("two-node", "Reticulum-Go -> LXMF-rs", "Link establishment proof RTT"),
    ("two-node", "bidirectional", "Link data"),
    ("two-node", "Reticulum-Go -> LXMF-rs", "packet-sized request response and correlation"),
    ("two-node", "Reticulum-Go -> LXMF-rs", "Resource small"),
    ("two-node", "LXMF-rs -> Reticulum-Go", "Resource small"),
    ("two-node", "LXMF-rs -> Reticulum-Go", "Resource 1 MiB"),
    ("two-node", "LXMF-rs -> Reticulum-Go", "Resource 50 MiB"),
    ("two-node", "Reticulum-Go -> LXMF-rs", "Link teardown"),
    ("two-node", "LXMF-rs -> Reticulum-Go", "Link establishment proof RTT"),
    ("two-node", "LXMF-rs -> Reticulum-Go", "packet-sized request response and correlation"),
    ("two-node", "LXMF-rs -> Reticulum-Go", "Link teardown"),
    ("LXMF-rs — Reticulum-Go — LXMF-rs", "bidirectional", "multi-hop announce propagation, path expiry and rediscovery"),
    ("LXMF-rs — Reticulum-Go — LXMF-rs", "left -> right", "multi-hop encrypted packet and delivery proof"),
    ("LXMF-rs — Reticulum-Go — LXMF-rs", "right -> left", "multi-hop encrypted packet and delivery proof"),
    ("LXMF-rs — Reticulum-Go — LXMF-rs", "left -> right", "multi-hop Link establishment proof"),
    ("LXMF-rs — Reticulum-Go — LXMF-rs", "right -> left", "multi-hop Link establishment proof"),
    ("LXMF-rs — Reticulum-Go — LXMF-rs", "left -> right", "multi-hop Resource 1 MiB"),
    ("LXMF-rs — Reticulum-Go — LXMF-rs", "right -> left", "multi-hop Resource 1 MiB"),
    ("LXMF-rs — Reticulum-Go — LXMF-rs", "bidirectional", "Link teardown"),
}

RETICULUM_GO_ALLOWED_UNSUPPORTED = {
    ("Resource 1 MiB", "Reticulum-Go control API -> LXMF-rs", "UNSUPPORTED", "peer_surface_unavailable"),
    ("Resource 50 MiB", "Reticulum-Go control API -> LXMF-rs", "UNSUPPORTED", "peer_surface_unavailable"),
}


def signature(row: dict[str, Any]) -> ScenarioSignature:
    return (str(row.get("topology")), str(row.get("direction")), str(row.get("scenario")))


def require_rows(
    rows: list[dict[str, Any]], required: set[ScenarioSignature], peer: str
) -> list[str]:
    passing = {signature(row) for row in rows if row.get("status") == "PASS"}
    return [
        f"required passing {peer} scenario missing: {item}"
        for item in sorted(required - passing)
    ]


def validate(report: dict[str, Any]) -> list[str]:
    peer = report.get("peer", {}).get("implementation")
    level = report.get("level")
    scenarios = report.get("scenarios")
    if not isinstance(scenarios, list) or not scenarios:
        return ["report contains no scenarios"]
    errors: list[str] = []
    if level not in {"pr", "nightly", "release"}:
        errors.append(f"unknown evidence level: {level!r}")
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
                if row.get("classification") == "peer_divergence" and not str(
                    row.get("normative_reference") or ""
                ).startswith("Python RNS 1.5.0"):
                    errors.append(f"peer divergence lacks Python RNS 1.5.0 evidence: {key}")
                continue
            else:
                errors.append(f"unexpected rns-rs result: {key}: {row.get('failure_reason')}")
        present = {signature(row) for row in scenarios}
        for required in sorted(RNS_RS_REQUIRED_PRESENT - present):
            errors.append(f"required rns-rs scenario missing: {required}")
        required_pass = set(RNS_RS_REQUIRED_PR_PASS)
        if level in {"nightly", "release"}:
            required_pass.update(RNS_RS_REQUIRED_EXPANDED_PASS)
        errors.extend(require_rows(scenarios, required_pass, "rns-rs"))
    elif peer == "Reticulum-Go":
        for row in scenarios:
            if row.get("status") == "PASS":
                continue
            key = (
                row.get("scenario"),
                row.get("direction"),
                row.get("status"),
                row.get("classification"),
            )
            if (
                key in RETICULUM_GO_ALLOWED_UNSUPPORTED
                and row.get("failure_owner") == "Reticulum-Go"
                and row.get("failure_reason")
            ):
                continue
            else:
                errors.append(
                    f"unexpected Reticulum-Go result: {row.get('scenario')}: {row.get('status')}"
                )
        errors.extend(require_rows(scenarios, RETICULUM_GO_REQUIRED_PASS, "Reticulum-Go"))
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
