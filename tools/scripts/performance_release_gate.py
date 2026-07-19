#!/usr/bin/env python3
"""Apply release performance budgets to same-runner datasets."""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def load(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def indexed(rows: list[dict[str, Any]], key: str) -> dict[str, dict[str, Any]]:
    return {str(row[key]): row for row in rows}


def geometric_mean(values: list[float]) -> float:
    if not values or any(value <= 0 for value in values):
        raise ValueError("geometric mean inputs must be positive")
    return math.exp(sum(math.log(value) for value in values) / len(values))


def require_matching_environment(candidate: dict[str, Any], baseline: dict[str, Any]) -> None:
    candidate_env = candidate["environment"]
    baseline_env = baseline["environment"]
    for key in ("cpu", "uname", "rustc_version", "python_version"):
        if key in baseline_env and candidate_env.get(key) != baseline_env.get(key):
            raise ValueError(f"candidate and baseline differ in environment field {key}")
    if candidate.get("profile") != baseline.get("profile"):
        raise ValueError("candidate and baseline profiles differ")


def validate_dispersion(data: dict[str, Any]) -> list[str]:
    failures: list[str] = []
    for row in data.get("comparisons", []):
        for implementation in ("rust", "python"):
            relative = float(row["dispersion"][implementation]["p50_relative_mad"])
            if relative > 0.10:
                failures.append(
                    f"core dispersion {row['label']} {implementation} {relative:.2%} > 10%"
                )
    for row in data.get("e2e_comparisons", []):
        for implementation in ("rust", "python"):
            relative = float(row[implementation]["dispersion"]["p50_relative_mad"])
            if relative > 0.20:
                failures.append(
                    f"E2E dispersion {row['label']} {implementation} {relative:.2%} > 20%"
                )
    return failures


def evaluate(candidate: dict[str, Any], baseline: dict[str, Any]) -> dict[str, Any]:
    require_matching_environment(candidate, baseline)
    failures = validate_dispersion(candidate)
    candidate_core = indexed(candidate["comparisons"], "label")
    baseline_core = indexed(baseline["comparisons"], "label")
    shared = sorted(candidate_core.keys() & baseline_core.keys())
    if not shared:
        raise ValueError("candidate and baseline have no shared core workloads")

    throughput_ratios = [
        float(candidate_core[label]["rust"]["throughput_ops_per_sec"])
        / float(baseline_core[label]["rust"]["throughput_ops_per_sec"])
        for label in shared
    ]
    cpu_ratios = [
        float(candidate_core[label]["rust_resources"]["median_cpu_seconds_per_1k_ops"])
        / float(baseline_core[label]["rust_resources"]["median_cpu_seconds_per_1k_ops"])
        for label in shared
    ]
    rss_ratios = [
        float(candidate_core[label]["rust_resources"]["median_peak_rss_bytes"])
        / float(baseline_core[label]["rust_resources"]["median_peak_rss_bytes"])
        for label in shared
    ]
    throughput_ratio = geometric_mean(throughput_ratios)
    cpu_ratio = geometric_mean(cpu_ratios)
    rss_ratio = geometric_mean(rss_ratios)
    if throughput_ratio < 0.90:
        failures.append(f"Rust geometric-mean throughput {throughput_ratio:.3f}x < 0.900x")
    if cpu_ratio > 1.20:
        failures.append(f"Rust geometric-mean CPU {cpu_ratio:.3f}x > 1.200x")
    if rss_ratio > 1.20:
        failures.append(f"Rust geometric-mean peak RSS {rss_ratio:.3f}x > 1.200x")

    candidate_zmq = indexed(candidate.get("sdk_transport_comparisons", []), "operation")
    baseline_zmq = indexed(baseline.get("sdk_transport_comparisons", []), "operation")
    zmq_ratios: dict[str, float] = {}
    for operation in sorted(candidate_zmq.keys() & baseline_zmq.keys()):
        ratio = float(candidate_zmq[operation]["zmq_p95_ns"]) / float(
            baseline_zmq[operation]["zmq_p95_ns"]
        )
        zmq_ratios[operation] = ratio
        if ratio > 1.20:
            failures.append(f"critical ZeroMQ {operation} p95 {ratio:.3f}x > 1.200x")

    return {
        "status": "pass" if not failures else "fail",
        "candidate_sha": candidate["environment"]["git_commit"],
        "baseline_sha": baseline["environment"]["git_commit"],
        "shared_core_workloads": shared,
        "rust_geomean_throughput_ratio": throughput_ratio,
        "rust_geomean_cpu_ratio": cpu_ratio,
        "rust_geomean_peak_rss_ratio": rss_ratio,
        "zmq_p95_ratios": zmq_ratios,
        "failures": failures,
    }


def main() -> int:
    args = parse_args()
    try:
        report = evaluate(load(args.candidate), load(args.baseline))
        rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(rendered, encoding="utf-8")
        print(rendered, end="")
        return 0 if report["status"] == "pass" else 1
    except (OSError, KeyError, TypeError, ValueError, ZeroDivisionError, json.JSONDecodeError) as error:
        print(f"performance_release_gate: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
