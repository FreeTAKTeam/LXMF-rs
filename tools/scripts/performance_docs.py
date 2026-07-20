#!/usr/bin/env python3
"""Publish or verify generated release performance documentation."""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
README = ROOT / "README.md"
PAGE = ROOT / "docs/performance.md"
SCALE_DATASET = ROOT / "docs/performance/100-node-chain-2026-07-20.json"
START = "<!-- performance-summary:start -->"
END = "<!-- performance-summary:end -->"
PYTHON_INTEROP_WORKFLOW = ROOT / ".github/workflows/python-interop.yml"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release", default="v0.9.5")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--sdk-transport-report", type=Path)
    parser.add_argument("--e2e-report", type=Path)
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def dataset_path(release: str) -> Path:
    return ROOT / "docs/performance" / f"{release}.json"


def load(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def verify_workflow_refs(data: dict[str, Any]) -> None:
    workflow = PYTHON_INTEROP_WORKFLOW.read_text(encoding="utf-8")
    env = data["environment"]
    expected = {
        "PYTHON_RETICULUM_REF": env["python_rns_revision"],
        "PYTHON_LXMF_REF": env["python_lxmf_revision"],
    }
    for name, revision in expected.items():
        if f"{name}: {revision}" not in workflow:
            raise ValueError(f"{name} does not match the pinned performance dataset")


def enrich_dispersion(report_path: Path, data: dict[str, Any]) -> None:
    run_paths = sorted((report_path.parent / "runs").glob("run-*/python-impl-compare.json"))
    if len(run_paths) != data["compare_runs"]:
        raise ValueError("raw comparison run count does not match aggregate report")
    runs = [load(path) for path in run_paths]
    for aggregate in data["comparisons"]:
        values: dict[str, list[float]] = {"rust": [], "python": []}
        for run in runs:
            row = next((item for item in run["comparisons"] if item["label"] == aggregate["label"]), None)
            if row is None:
                raise ValueError(f"raw runs missing {aggregate['label']}")
            values["rust"].append(float(row["rust"]["p50_ns"]))
            values["python"].append(float(row["python"]["p50_ns"]))
        aggregate["dispersion"] = {}
        for implementation, samples in values.items():
            median = statistics.median(samples)
            mad = statistics.median(abs(sample - median) for sample in samples)
            relative = mad / median if median else 0.0
            aggregate["dispersion"][implementation] = {
                "p50_mad_ns": mad,
                "p50_relative_mad": relative,
                "run_p50_ns": samples,
            }
            if relative > 0.10:
                raise ValueError(
                    f"unstable core workload {aggregate['label']} {implementation}: relative MAD {relative:.2%} exceeds 10%"
                )


def fmt_ns(value: float) -> str:
    if value >= 1_000_000:
        return f"{value / 1_000_000:.2f} ms"
    if value >= 1_000:
        return f"{value / 1_000:.2f} us"
    return f"{value:.0f} ns"


def fmt_seconds(value: float | None) -> str:
    if value is None:
        return "-"
    return f"{value:.3f} s"


def scale_test_section(data: dict[str, Any]) -> list[str]:
    scenario = data["scenario"]
    lines = [
        "## 100-node chain scale tests",
        "",
        "Exploratory single-host scale results are stored in "
        "[`docs/performance/100-node-chain-2026-07-20.json`](performance/100-node-chain-2026-07-20.json). "
        f"Each run created `{scenario['nodes']}` nodes in a linear chain over `{scenario['media']}` simulated media "
        f"at `{scenario['bitrate_bps'] // 1_000_000}` Mbit/s, a `{scenario['mtu_bytes']}`-byte MTU, "
        f"`{scenario['propagation_seconds'] * 1_000:.0f}` ms propagation per medium, and "
        f"`{scenario['configured_loss']:.1%}` configured loss. "
        f"The `{scenario['transport_nodes']}` interior nodes acted as transports.",
        "",
        f"After a `{scenario['route_warmup_seconds']:.0f}`-second route warm-up, the endpoints sent "
        f"`{scenario['samples_per_direction']}` concurrent `{scenario['payload_bytes']}`-byte "
        f"{scenario['delivery_mode']} messages in each direction. RTT columns report p50 across delivered samples; "
        f"a dash means no sample was delivered. Readiness required all {scenario['nodes']} nodes to be running, "
        "connected, and addressed.",
        "",
        "| Composition | Ready | n0 -> n99 p50 | n99 -> n0 p50 | Delivered | Media TX | Result |",
        "|---|---:|---:|---:|---:|---:|---:|",
    ]
    for run in data["runs"]:
        forward = run["forward"]
        reverse = run["reverse"]
        delivered = forward["delivered"] + reverse["delivered"]
        samples = forward["samples"] + reverse["samples"]
        forward_label = (
            f"{fmt_seconds(forward['p50_seconds'])} "
            f"({forward['source_implementation']} -> {forward['destination_implementation']})"
        )
        reverse_label = (
            f"{fmt_seconds(reverse['p50_seconds'])} "
            f"({reverse['source_implementation']} -> {reverse['destination_implementation']})"
        )
        lines.append(
            f"| {run['label']} | {fmt_seconds(run['ready_seconds'])} | {forward_label} | {reverse_label} | "
            f"{delivered}/{samples} | {run['media_tx']:,} | {run['status']} |"
        )
    python_run = next((run for run in data["runs"] if run["composition"] == "python"), None)
    if python_run is None:
        raise ValueError("100-node scale dataset is missing the all-Python run")
    environment = data["environment"]
    lines.extend(
        [
            "",
            "The all-Python reverse direction delivered `0/3` samples, logged "
            f"`{python_run['reverse']['failure']}`, and reached the "
            f"`{python_run['reverse']['timeout_seconds']:.0f}`-second action timeout; the missing RTT is not zero.",
            "",
            "These are single runs per composition, not a repeated benchmark distribution. The simulator and all nodes "
            "shared one host, so scheduler and simulator overhead affect the measurements. The Rust binary came from "
            f"LXMF-rs `{environment['lxmf_rs_revision']}`; the Python references were Reticulum "
            f"`{environment['python_rns_revision']}` and LXMF `{environment['python_lxmf_revision']}`. "
            "The reticulated harness working tree contained uncommitted changes, so these results are exploratory "
            "evidence rather than a release threshold.",
        ]
    )
    return lines


def summary_section(data: dict[str, Any], release: str) -> str:
    env = data["environment"]
    rows = data["comparisons"][:4]
    lines = [
        START,
        "## Measured performance",
        "",
        f"Release dataset: `{release}` at `{env['git_commit']}`; Python Reticulum `{env['python_rns_revision'][:12]}` and LXMF `{env['python_lxmf_revision'][:12]}`.",
        "",
        "| Matched workload | Rust p50 | Python p50 | Rust/Python |",
        "|---|---:|---:|---:|",
    ]
    for row in rows:
        lines.append(
            f"| {row['label']} | {fmt_ns(row['rust']['p50_ns'])} | {fmt_ns(row['python']['p50_ns'])} | {row['rust_advantage_vs_python']['p50_speedup']:.2f}x |"
        )
    lines.extend(
        [
            "",
            "These are matched-workload comparisons, not a claim of whole-system superiority. See [methodology, complete results, variability, and limitations](docs/performance.md).",
            END,
        ]
    )
    return "\n".join(lines)


def performance_page(data: dict[str, Any], release: str, scale_data: dict[str, Any]) -> str:
    env = data["environment"]
    lines = [
        "# Performance",
        "",
        "<!-- GENERATED: tools/scripts/performance_docs.py -->",
        "",
        f"Dataset: [`docs/performance/{release}.json`](performance/{release}.json). All numbers below originate from release SHA `{env['git_commit']}`.",
        "",
        "## Methodology",
        "",
        f"The report uses `{data['compare_runs']}` interleaved comparison runs and `{data['resource_runs']}` isolated resource runs. Fixtures and process setup are completed before timed regions. Results are medians; p95 and p99 retain tail visibility. Rust/Python ranking is evidence, not a release threshold.",
        "",
        "## Environment",
        "",
        f"- Timestamp: `{env['timestamp_utc']}`",
        f"- Release SHA: `{env['git_commit']}`",
        f"- Python Reticulum: `{env['python_rns_revision']}`",
        f"- Python LXMF: `{env['python_lxmf_revision']}`",
        f"- Rust: `{env['rustc_version']}`",
        f"- Python: `{env['python_version']}`",
        f"- CPU: `{env['cpu']}`",
        f"- OS/kernel: `{env['uname']}`",
        f"- Profile: `{data['profile']}`",
        "",
        "## Protocol/core and transport hot paths",
        "",
        "| Workload | Class | Payload | Batch | Rust p50 | Python p50 | Rust/Python | Rust variability | Python variability | Rust RSS | Python RSS |",
        "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in data["comparisons"]:
        context = row["context"]
        lines.append(
            "| {label} | {klass} | {payload} | {batch} | {rust50} | {python50} | {ratio:.2f}x | {rust_var:.2%} | {python_var:.2%} | {rust_rss:.1f} MiB | {python_rss:.1f} MiB |".format(
                label=row["label"],
                klass=context.get("workload_class") or "unspecified",
                payload=context.get("payload_size_bytes") or "-",
                batch=context.get("batch_size") or "-",
                rust50=fmt_ns(row["rust"]["p50_ns"]),
                python50=fmt_ns(row["python"]["p50_ns"]),
                ratio=row["rust_advantage_vs_python"]["p50_speedup"],
                rust_var=row["dispersion"]["rust"]["p50_relative_mad"],
                python_var=row["dispersion"]["python"]["p50_relative_mad"],
                rust_rss=row["rust_resources"]["median_peak_rss_bytes"] / 1_048_576,
                python_rss=row["python_resources"]["median_peak_rss_bytes"] / 1_048_576,
            )
        )
    transport = data.get("sdk_transport_comparisons", [])
    lines.extend(["", "## Rust SDK transport comparison", ""])
    if transport:
        lines.extend(["| Operation | ZeroMQ p50 | HTTP p50 | Unix p50 | ZeroMQ/HTTP | ZeroMQ/Unix |", "|---|---:|---:|---:|---:|---:|"])
        for row in transport:
            lines.append(
                f"| {row['operation']} | {fmt_ns(row['zmq_p50_ns'])} | {fmt_ns(row['http_p50_ns'])} | {fmt_ns(row['unix_p50_ns'])} | {row['http_p50_ns'] / row['zmq_p50_ns']:.2f}x | {row['unix_p50_ns'] / row['zmq_p50_ns']:.2f}x |"
            )
    else:
        lines.append("No SDK transport measurements are present in this dataset; release publication must add them before claiming a ZeroMQ performance advantage.")
    lines.extend(["", "## Same-topology end-to-end comparison", ""])
    e2e = data.get("e2e_comparisons", [])
    if e2e:
        lines.extend(
            [
                "These matched sender workloads use the same two-node loopback TCP topology with one Rust and one pinned-Python endpoint. Startup and route warm-up are outside the timed enqueue-to-receiver-evidence boundary.",
                "",
                "| Workload | Route | Payload | Rust p50 | Python p50 | Rust/Python | Rust p95 | Python p95 | Rust CPU | Python CPU | Rust RSS | Python RSS | Rust variability | Python variability |",
                "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
            ]
        )
        for row in e2e:
            lines.append(
                "| {label} | {route} | {payload} | {rust50} | {python50} | {ratio:.2f}x | {rust95} | {python95} | {rust_cpu:.3f}s | {python_cpu:.3f}s | {rust_rss:.1f} MiB | {python_rss:.1f} MiB | {rust_var:.2%} | {python_var:.2%} |".format(
                    label=row["label"],
                    route=row["route_state"],
                    payload=row["payload_size_bytes"],
                    rust50=fmt_ns(row["rust"]["p50_ns"]),
                    python50=fmt_ns(row["python"]["p50_ns"]),
                    ratio=row["rust_p50_speedup_vs_python"],
                    rust95=fmt_ns(row["rust"]["p95_ns"]),
                    python95=fmt_ns(row["python"]["p95_ns"]),
                    rust_cpu=row["rust"]["median_cpu_seconds"],
                    python_cpu=row["python"]["median_cpu_seconds"],
                    rust_rss=row["rust"]["median_peak_rss_bytes"] / 1_048_576,
                    python_rss=row["python"]["median_peak_rss_bytes"] / 1_048_576,
                    rust_var=row["rust"]["dispersion"]["p50_relative_mad"],
                    python_var=row["python"]["dispersion"]["p50_relative_mad"],
                )
            )
    else:
        lines.append("No E2E measurements are present; release publication must add the report before making whole-delivery claims.")
    lines.extend(["", *scale_test_section(scale_data)])
    lines.extend(
        [
            "",
            "## Limitations",
            "",
            "- Scheduler noise, CPU frequency changes, and host background work affect tails and resource readings.",
            "- Cryptographic workloads include randomness where the implementation requires it; fixture construction remains outside timed regions.",
            "- Python wins must be reported without suppression. Ratios below 1.0 mean Python was faster.",
            "- Hardware, public-network, and human-operated workflows are intentionally deferred to v1.0 and are not represented here.",
            "",
            "## Reproduce",
            "",
            "```bash",
            "cargo xtask python-impl-bench-report",
            "python3 tools/scripts/e2e_performance.py --profile report",
            f"python3 tools/scripts/performance_docs.py --release {release} --report target/criterion/python-impl-report/report.json",
            "python3 tools/scripts/performance_docs.py --check",
            "```",
            "",
        ]
    )
    return "\n".join(lines)


def replace_marked(source: str, replacement: str) -> str:
    if START in source and END in source:
        before, tail = source.split(START, 1)
        _, after = tail.split(END, 1)
        return before.rstrip() + "\n\n" + replacement + after
    anchor = "\n## License"
    if anchor not in source:
        raise ValueError("README is missing performance markers and License insertion anchor")
    return source.replace(anchor, "\n\n" + replacement + "\n" + anchor, 1)


def main() -> int:
    args = parse_args()
    target = dataset_path(args.release)
    try:
        if args.report:
            data = load(args.report)
            enrich_dispersion(args.report, data)
            if args.sdk_transport_report:
                data.update(load(args.sdk_transport_report))
            if args.e2e_report:
                e2e = load(args.e2e_report)
                if e2e["python_rns_revision"] != data["environment"]["python_rns_revision"]:
                    raise ValueError("E2E Reticulum revision differs from core report")
                if e2e["python_lxmf_revision"] != data["environment"]["python_lxmf_revision"]:
                    raise ValueError("E2E LXMF revision differs from core report")
                data.update(e2e)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        data = load(target)
        scale_data = load(SCALE_DATASET)
        verify_workflow_refs(data)
        expected_page = performance_page(data, args.release, scale_data)
        expected_readme = replace_marked(README.read_text(encoding="utf-8"), summary_section(data, args.release))
        if args.check:
            if not PAGE.is_file() or PAGE.read_text(encoding="utf-8") != expected_page:
                raise ValueError("docs/performance.md drift")
            if README.read_text(encoding="utf-8") != expected_readme:
                raise ValueError("README performance section drift")
        else:
            PAGE.write_text(expected_page, encoding="utf-8")
            README.write_text(expected_readme, encoding="utf-8")
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        print(f"performance_docs: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
