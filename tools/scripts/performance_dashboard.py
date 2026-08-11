#!/usr/bin/env python3
"""Render the public, self-contained performance dashboard."""

from __future__ import annotations

import argparse
import html
import json
from pathlib import Path
from typing import Any


IMPLEMENTATIONS = (
    ("python", "Python"),
    ("lxmf_rs", "LXMF-rs"),
    ("rns_rs", "rns-rs"),
)

PUBLIC_ROWS = (
    ("packet_encode", "Packet encode/s", "ops/s"),
    ("announce_validation", "Announce validation/s", "ops/s"),
    ("path_convergence_cold", "Cold path convergence", "s"),
    ("path_lookup_warm", "Warm path lookup", "s"),
    ("link_setup", "Link setup p50/p99", "s"),
    ("resource_1mib", "Exact 1 MiB Resource", "MiB/s"),
    ("resource_50mib", "Exact 50 MiB Resource", "MiB/s"),
    ("resource_1mib_peak_ram", "1 MiB Resource peak RAM", "MiB"),
    ("resource_50mib_peak_ram", "50 MiB Resource peak RAM", "MiB"),
    ("resource_1mib_cpu", "1 MiB Resource CPU", "ms/MiB"),
    ("resource_50mib_cpu", "50 MiB Resource CPU", "ms/MiB"),
    ("active_links_1000", "1000 active links", "links"),
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--release", required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def load(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def find_comparison(data: dict[str, Any], label: str) -> dict[str, Any] | None:
    return next((row for row in data.get("comparisons", []) if row.get("label") == label), None)


def find_e2e(data: dict[str, Any], label: str) -> dict[str, Any] | None:
    return next((row for row in data.get("e2e_comparisons", []) if row.get("label") == label), None)


def unavailable(reason: str) -> dict[str, Any]:
    return {"status": "not_available", "reason": reason}


def unavailable_reason(row_id: str, implementation: str) -> str:
    if implementation == "rns_rs":
        return "No exact rns-rs measurement is present in this release dataset"
    return {
        "resource_1mib": "No exact 1 MiB Resource measurement is present",
        "resource_50mib": "No exact 50 MiB Resource measurement is present",
        "link_setup": "No exact link setup measurement is present",
        "path_lookup_warm": "No exact warm path lookup measurement is present",
        "resource_1mib_peak_ram": "No exact 1 MiB Resource peak-RAM measurement is present",
        "resource_50mib_peak_ram": "No exact 50 MiB Resource peak-RAM measurement is present",
        "resource_1mib_cpu": "No exact 1 MiB Resource CPU measurement is present",
        "resource_50mib_cpu": "No exact 50 MiB Resource CPU measurement is present",
        "active_links_1000": "No exact 1000-active-link measurement is present",
    }.get(row_id, "No exact release measurement is present")


def measured(value: float, *, p99: float | None = None, details: dict[str, Any] | None = None) -> dict[str, Any]:
    result: dict[str, Any] = {"status": "measured", "value": value}
    if p99 is not None:
        result["p99"] = p99
    if details:
        result["details"] = details
    return result


def comparison_cell(row: dict[str, Any], implementation: str) -> dict[str, Any]:
    source = row["rust"] if implementation == "lxmf_rs" else row["python"]
    return measured(
        float(source["throughput_ops_per_sec"]),
        details={
            "p50_ns": source.get("p50_ns"),
            "p95_ns": source.get("p95_ns"),
            "p99_ns": source.get("p99_ns"),
        },
    )


def fallback_matrix(data: dict[str, Any]) -> list[dict[str, Any]]:
    packet = find_comparison(data, "Reticulum packet pack")
    announce = find_comparison(data, "Reticulum announce validate")
    discovery = find_e2e(data, "Loopback TCP cold destination discovery")
    link_setup = find_e2e(data, "Loopback TCP link setup")
    rows: list[dict[str, Any]] = []

    for row_id, label, unit in PUBLIC_ROWS:
        cells = {
            implementation: unavailable(unavailable_reason(row_id, implementation))
            for implementation, _ in IMPLEMENTATIONS
        }
        if row_id == "packet_encode" and packet:
            cells["python"] = comparison_cell(packet, "python")
            cells["lxmf_rs"] = comparison_cell(packet, "lxmf_rs")
        elif row_id == "announce_validation" and announce:
            cells["python"] = comparison_cell(announce, "python")
            cells["lxmf_rs"] = comparison_cell(announce, "lxmf_rs")
        elif row_id == "path_convergence_cold" and discovery:
            for implementation, source_key in (("python", "python"), ("lxmf_rs", "rust")):
                source = discovery[source_key]
                cells[implementation] = measured(
                    float(source["p50_ns"]) / 1_000_000_000.0,
                    p99=float(source["p99_ns"]) / 1_000_000_000.0,
                    details={"topology": discovery.get("topology"), "timed_boundary": discovery.get("timed_boundary")},
                )
        elif row_id == "link_setup" and link_setup:
            for implementation, source_key in (("python", "python"), ("lxmf_rs", "rust")):
                source = link_setup[source_key]
                cells[implementation] = measured(
                    float(source["p50_ns"]) / 1_000_000_000.0,
                    p99=float(source["p99_ns"]) / 1_000_000_000.0,
                    details={"topology": link_setup.get("topology"), "timed_boundary": link_setup.get("timed_boundary")},
                )
        independent = data.get("independent_performance", {}).get("public_cells", {})
        if isinstance(independent, dict) and isinstance(independent.get(row_id), dict):
            cells.update(independent[row_id])
        rows.append({"id": row_id, "label": label, "unit": unit, "cells": cells})

    return rows


def public_rows(data: dict[str, Any]) -> list[dict[str, Any]]:
    public = data.get("public_benchmark")
    if isinstance(public, dict) and isinstance(public.get("rows"), list):
        return public["rows"]
    return fallback_matrix(data)


def format_value(cell: dict[str, Any], unit: str) -> str:
    status = cell.get("status")
    if status != "measured":
        return {
            "failed": "FAILED",
            "not_supported": "UNSUPPORTED",
        }.get(str(status), "N/A")
    value = float(cell["value"])
    if cell.get("p99") is not None:
        return f"{value:.3f} / {float(cell['p99']):.3f}"
    if unit == "ops/s":
        return f"{value:,.0f}"
    if unit in ("MB/s", "MiB/s", "MiB", "ms/MB", "ms/MiB"):
        return f"{value:,.2f}"
    if unit == "links":
        return f"{value:,.0f}"
    return f"{value:.3f}"


def cell_title(cell: dict[str, Any]) -> str:
    if cell.get("status") == "measured":
        details = cell.get("details")
        return json.dumps(details, sort_keys=True) if details else "Measured"
    return str(cell.get("reason", "No measurement"))


def render_dashboard(data: dict[str, Any], release: str) -> str:
    environment = data.get("environment", {})
    rows = public_rows(data)
    rows_json = json.dumps(rows, sort_keys=True).replace("</", "<\\/")
    source_commit = html.escape(str(environment.get("git_commit", "unknown")))
    generated_at = html.escape(str(environment.get("timestamp_utc", "unknown")))
    profile = html.escape(str(data.get("profile", data.get("e2e_profile", "unknown"))))

    table_rows = []
    for row in rows:
        cells = []
        for implementation, _ in IMPLEMENTATIONS:
            cell = row["cells"].get(implementation, unavailable("Implementation did not report this workload"))
            status = html.escape(str(cell.get("status", "unknown")))
            title = html.escape(cell_title(cell), quote=True)
            value = html.escape(format_value(cell, row["unit"]))
            cells.append(f'<td class="{status}" title="{title}">{value}</td>')
        table_rows.append(
            f'<tr><th scope="row">{html.escape(row["label"])}</th>'
            f'<td class="unit">{html.escape(row["unit"])}</td>{"".join(cells)}</tr>'
        )

    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>LXMF-rs performance — {html.escape(release)}</title>
<style>
:root {{ color-scheme: dark; --bg:#0d1117; --panel:#161b22; --line:#30363d; --text:#e6edf3; --muted:#8b949e; --good:#3fb950; --warn:#d29922; }}
* {{ box-sizing:border-box; }} body {{ margin:0; background:var(--bg); color:var(--text); font:15px/1.5 system-ui,sans-serif; }}
main {{ max-width:1180px; margin:0 auto; padding:32px 20px 56px; }} h1 {{ margin:0 0 6px; font-size:32px; }} h2 {{ margin-top:30px; }}
.muted {{ color:var(--muted); }} .panel {{ background:var(--panel); border:1px solid var(--line); border-radius:10px; padding:18px; margin-top:18px; }}
table {{ width:100%; border-collapse:collapse; }} th,td {{ border-bottom:1px solid var(--line); padding:12px 10px; text-align:right; white-space:nowrap; }} th:first-child, td:first-child {{ text-align:left; }} thead th {{ color:var(--muted); font-weight:600; }} td.unit {{ color:var(--muted); font-size:13px; }} td.measured {{ color:var(--good); font-variant-numeric:tabular-nums; }} td.not_available,td.not_supported,td.failed {{ color:var(--warn); }}
.meta {{ display:grid; grid-template-columns:repeat(auto-fit,minmax(220px,1fr)); gap:10px; }} .meta div {{ color:var(--muted); }} code {{ color:var(--text); }} a {{ color:#58a6ff; }} .note {{ color:var(--muted); font-size:13px; }}
@media (max-width:760px) {{ main {{ padding:22px 10px; }} .panel {{ overflow-x:auto; }} table {{ min-width:760px; }} }}
</style>
</head>
<body>
<main>
<h1>LXMF-rs performance dashboard</h1>
<div class="muted">Release {html.escape(release)} · matched local workloads · generated from canonical JSON</div>
<section class="panel">
<table>
<thead><tr><th>Test</th><th>Unit</th><th>Python</th><th>LXMF-rs</th><th>rns-rs</th></tr></thead>
<tbody>{"".join(table_rows)}</tbody>
</table>
</section>
<section class="panel meta">
<div>Release SHA<br><code>{source_commit}</code></div>
<div>Generated<br><code>{generated_at}</code></div>
<div>Profile<br><code>{profile}</code></div>
<div>Raw data<br><code>{html.escape(release)}.json</code></div>
</section>
<h2>Methodology</h2>
<p class="note">Values are comparable only within the recorded fixture, runner, toolchain, topology, and timing boundary. N/A means that this release did not produce an exact measurement for the requested workload; it is not a zero and must not be inferred from a nearby workload.</p>
<p class="note">The dashboard is generated locally or by an explicitly requested/release benchmark workflow. Ordinary pushes and pull requests do not run performance measurements.</p>
<script type="application/json" id="benchmark-data">{rows_json}</script>
</main>
</body>
</html>
"""


def main() -> int:
    args = parse_args()
    data = load(args.dataset)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(render_dashboard(data, args.release), encoding="utf-8")
    print(f"performance_dashboard: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
