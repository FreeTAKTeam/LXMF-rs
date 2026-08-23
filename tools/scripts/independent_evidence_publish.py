#!/usr/bin/env python3
"""Generate versioned public independent interop JSON, Markdown, and HTML."""

from __future__ import annotations

import argparse
import copy
import html
import json
import time
from pathlib import Path
from typing import Any

from independent_interop_gate import validate as validate_interop


def five_node_timings(report: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        {
            "topology": row.get("topology"),
            "scenario": row.get("scenario"),
            "direction": row.get("direction"),
            "runtime_seconds": row.get("runtime_seconds"),
        }
        for row in report.get("scenarios", [])
        if row.get("status") == "PASS"
        and str(row.get("scenario", "")).startswith("five-node ")
    ]


def performance_evidence_status(performance: dict[str, Any] | None) -> str:
    if performance is None:
        return "NOT_RUN"
    for cells in performance.get("public_cells", {}).values():
        if any(cell.get("status") == "failed" for cell in cells.values()):
            return "FAIL"
    rows = list(performance.get("path_convergence", {}).values())
    rows.append(performance.get("link_setup", {}))
    for implementations in performance.get("resources", {}).values():
        rows.extend(implementations.values())
    if any(row.get("variation_class") == "hard_failure" for row in rows):
        return "FAIL"
    return "PASS"


def parity_readiness(parity: dict[str, Any]) -> dict[str, Any]:
    summary = parity.get("summary", {})
    counts: dict[str, int] = {}
    for item in parity.get("items", []):
        implementation = str(item.get("implementation", "unmapped"))
        counts[implementation] = counts.get(implementation, 0) + 1
    total = int(summary.get("total", sum(counts.values())))
    not_applicable = int(
        summary.get("not-applicable", counts.get("not-applicable", 0))
    )
    complete = int(summary.get("complete", counts.get("complete", 0)))
    partial = int(summary.get("partial", counts.get("partial", 0)))
    unmapped = total - complete - partial - not_applicable
    applicable = total - not_applicable
    return {
        "status": (
            "PASS"
            if complete == applicable and partial == 0 and unmapped == 0
            else "FAIL"
        ),
        "total": total,
        "applicable": applicable,
        "not_applicable": not_applicable,
        "complete": complete,
        "partial": partial,
        "unmapped": unmapped,
        "source": "docs/status/python-surface-parity.json",
    }


def build_bundle(
    version: str,
    rns_rs: dict[str, Any],
    reticulum_go: dict[str, Any],
    performance: dict[str, Any] | None,
    parity: dict[str, Any],
) -> dict[str, Any]:
    rns_gate_errors = validate_interop(rns_rs)
    retgo_gate_errors = validate_interop(reticulum_go)
    public_rns_rs = copy.deepcopy(rns_rs)
    public_reticulum_go = copy.deepcopy(reticulum_go)
    public_rns_rs.pop("artifact_root", None)
    public_reticulum_go.pop("artifact_root", None)
    return {
        "schema": "lxmf-rs-independent-evidence-v1",
        "version": version,
        "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "reference_boundary": (
            "Python RNS/LXMF is the compatibility reference; rns-rs and Reticulum-Go "
            "are independently authored peer implementations."
        ),
        "interop": {
            "rns-rs": public_rns_rs,
            "Reticulum-Go": public_reticulum_go,
        },
        "performance": performance,
        "topology_performance": five_node_timings(rns_rs),
        "readiness_axes": {
            "rns_1_5_0_software_parity": parity_readiness(parity),
            "pinned_python_interoperability": {
                "status": "SEPARATE_REQUIRED_GATE",
                "source": ".github/workflows/verify.yml",
            },
            "independent_interoperability": {
                "rns-rs": "PASS" if not rns_gate_errors else "FAIL",
                "Reticulum-Go": "PASS_SUPPORTED_SUBSET" if not retgo_gate_errors else "FAIL",
            },
            "performance_evidence": {"status": performance_evidence_status(performance)},
            "physical_hil": {"status": "SEPARATE_EVIDENCE_AXIS"},
            "third_party_clients": {"status": "SEPARATE_EVIDENCE_AXIS"},
        },
        "known_peer_divergences": [
            row
            for row in rns_rs.get("scenarios", [])
            if row.get("classification") in {"peer_divergence", "dependency_failed"}
        ],
        "raw_assets": [
            "independent-rns-rs-raw.tar.gz",
            "independent-reticulum-go-raw.tar.gz",
            "lxmf-rs-performance-raw.tar.gz",
        ],
    }


def render_markdown(bundle: dict[str, Any]) -> str:
    lines = [
        f"# Independent interoperability evidence — {bundle['version']}",
        "",
        "<!-- GENERATED: tools/scripts/independent_evidence_publish.py -->",
        "",
        bundle["reference_boundary"],
        "",
        "| Peer | Pin | PASS | FAIL | BLOCKED | UNSUPPORTED |",
        "|---|---|---:|---:|---:|---:|",
    ]
    for name, report in bundle["interop"].items():
        counts = report.get("summary", {}).get("counts", {})
        peer = report.get("peer", {})
        lines.append(
            f"| {name} | `{peer.get('revision', 'unknown')}` | {counts.get('PASS', 0)} | "
            f"{counts.get('FAIL', 0)} | {counts.get('BLOCKED', 0)} | "
            f"{counts.get('UNSUPPORTED', 0)} |"
        )
    lines.extend(
        [
            "",
            "## Readiness axes",
            "",
            "| Axis | Result |",
            "|---|---|",
            f"| RNS 1.5.0 software parity | {bundle['readiness_axes']['rns_1_5_0_software_parity']['complete']} / {bundle['readiness_axes']['rns_1_5_0_software_parity']['applicable']} PASS |",
            f"| Pinned Python interoperability | {bundle['readiness_axes']['pinned_python_interoperability']['status']} |",
            f"| Independent rns-rs interoperability | {bundle['readiness_axes']['independent_interoperability']['rns-rs']} |",
            f"| Independent Reticulum-Go interoperability | {bundle['readiness_axes']['independent_interoperability']['Reticulum-Go']} |",
            f"| Performance evidence | {bundle['readiness_axes']['performance_evidence']['status']} |",
            f"| Physical HIL | {bundle['readiness_axes']['physical_hil']['status']} |",
            f"| Third-party clients | {bundle['readiness_axes']['third_party_clients']['status']} |",
            "",
            "## Scenario results",
            "",
            "| Peer | Topology | Scenario | Direction | Result | Runtime |",
            "|---|---|---|---|---:|---:|",
        ]
    )
    for name, report in bundle["interop"].items():
        for row in report.get("scenarios", []):
            lines.append(
                f"| {name} | {row.get('topology', '-')} | {row.get('scenario', '-')} | "
                f"{row.get('direction', '-')} | {row.get('status', '-')} | "
                f"{float(row.get('runtime_seconds') or 0):.3f}s |"
            )
    lines.extend(
        [
            "",
            "## Five-node network timings",
            "",
            "| Topology | Workload | Direction | Runtime |",
            "|---|---|---|---:|",
        ]
    )
    for row in bundle["topology_performance"]:
        lines.append(
            f"| {row.get('topology', '-')} | {row.get('scenario', '-')} | "
            f"{row.get('direction', '-')} | {float(row.get('runtime_seconds') or 0):.3f}s |"
        )
    lines.extend(
        [
            "",
            "## Interpretation",
            "",
            "The rns-rs gate permits only the explicitly classified peer divergences in the "
            "JSON. Reticulum-Go unsupported rows are capability boundaries, not passes. "
            "All other non-PASS rows fail CI. Raw logs and checksummed bundles are release assets.",
            "",
        ]
    )
    return "\n".join(lines)


def render_html(bundle: dict[str, Any]) -> str:
    rows = []
    for peer, report in bundle["interop"].items():
        for row in report.get("scenarios", []):
            values = [
                peer,
                row.get("topology", "-"),
                row.get("scenario", "-"),
                row.get("direction", "-"),
                row.get("status", "-"),
                f"{float(row.get('runtime_seconds') or 0):.3f}s",
            ]
            rows.append("<tr>" + "".join(f"<td>{html.escape(str(v))}</td>" for v in values) + "</tr>")
    embedded = json.dumps(bundle, sort_keys=True).replace("</", "<\\/")
    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>LXMF-rs independent evidence {html.escape(bundle['version'])}</title>
<style>body{{font:15px system-ui;margin:2rem;max-width:1500px}}table{{border-collapse:collapse;width:100%}}th,td{{border-bottom:1px solid #ddd;padding:.55rem;text-align:left}}th{{position:sticky;top:0;background:#fff}}code{{font-size:.9em}}</style></head>
<body><h1>Independent interoperability evidence — {html.escape(bundle['version'])}</h1>
<p>{html.escape(bundle['reference_boundary'])}</p>
<h2>Readiness axes</h2><pre>{html.escape(json.dumps(bundle['readiness_axes'], indent=2, sort_keys=True))}</pre>
<table><thead><tr><th>Peer</th><th>Topology</th><th>Scenario</th><th>Direction</th><th>Result</th><th>Runtime</th></tr></thead>
<tbody>{''.join(rows)}</tbody></table>
<script id="independent-evidence" type="application/json">{embedded}</script></body></html>\n"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--rns-rs", type=Path, required=True)
    parser.add_argument("--reticulum-go", type=Path, required=True)
    parser.add_argument("--performance", type=Path)
    parser.add_argument(
        "--parity",
        type=Path,
        default=Path("docs/status/python-surface-parity.json"),
    )
    parser.add_argument("--output-dir", type=Path, default=Path("docs/interop"))
    args = parser.parse_args()
    read = lambda path: json.loads(path.read_text(encoding="utf-8"))
    bundle = build_bundle(
        args.version,
        read(args.rns_rs),
        read(args.reticulum_go),
        read(args.performance) if args.performance else None,
        read(args.parity),
    )
    args.output_dir.mkdir(parents=True, exist_ok=True)
    stem = f"{args.version}-independent"
    (args.output_dir / f"{stem}.json").write_text(
        json.dumps(bundle, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    (args.output_dir / f"{stem}.md").write_text(render_markdown(bundle), encoding="utf-8")
    (args.output_dir / f"{stem}.html").write_text(render_html(bundle), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
