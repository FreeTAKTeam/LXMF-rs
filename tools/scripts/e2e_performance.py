#!/usr/bin/env python3
"""Run matched Rust/Python sender workloads on the same loopback TCP topology."""

from __future__ import annotations

import argparse
import json
import os
import re
import statistics
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
HARNESS = ROOT / "tools/scripts/python-lxmd-rust-lxmd-smoke.sh"
RNS_REPO = ROOT / "target/python-references/Reticulum"
LXMF_REPO = ROOT / "target/python-references/LXMF"
RESUME_EXISTING = False

WORKLOADS = (
    (
        "cold_discovery",
        "rns_path_request_rust_to_python",
        "rns_path_request_python_to_rust",
        0,
    ),
    (
        "link_setup",
        "link_setup_rust_to_python",
        "link_setup_python_to_rust",
        0,
    ),
    ("direct", "direct_rust_to_python", "direct_python_to_rust", 256),
    ("opportunistic", "opportunistic_rust_to_python", "opportunistic_python_to_rust", 256),
    ("propagated", "propagated_rust_to_python", "propagated_python_to_rust", 256),
    ("resource", "resource_transfer", "direct_python_to_rust", 16384),
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=("smoke", "report"), default="smoke")
    parser.add_argument("--runs", type=int)
    parser.add_argument("--output", type=Path, default=ROOT / "target/performance/e2e.json")
    parser.add_argument("--resume", action="store_true", help="Reuse valid raw sample reports")
    return parser.parse_args()


def percentile(samples: list[float], fraction: float) -> float:
    ordered = sorted(samples)
    return ordered[round((len(ordered) - 1) * fraction)]


def relative_mad(samples: list[float]) -> float:
    median = statistics.median(samples)
    mad = statistics.median(abs(sample - median) for sample in samples)
    return mad / median if median else 0.0


def git_revision(path: Path) -> str:
    return subprocess.check_output(
        ["git", "-C", str(path), "rev-parse", "HEAD"], text=True
    ).strip()


def run_sample(
    workload: str,
    implementation: str,
    case: str,
    payload_bytes: int,
    ordinal: int,
    output_root: Path,
) -> dict[str, float]:
    sample_root = output_root / "raw" / workload / implementation / f"sample-{ordinal:02d}"
    sample_root.mkdir(parents=True, exist_ok=True)
    report = sample_root / "report.json"
    if RESUME_EXISTING and report.is_file():
        existing = json.loads(report.read_text(encoding="utf-8"))
        timing = existing.get("performance")
        if timing and float(timing.get("enqueue_to_delivery_ns", 0)) > 0:
            return {
                "latency_ns": float(timing["enqueue_to_delivery_ns"]),
                "cpu_seconds": float(timing.get("cpu_seconds", 0.0)),
                "peak_rss_bytes": float(timing.get("peak_rss_bytes", 0.0)),
            }
    env = os.environ.copy()
    env.update(
        {
            "COMPAT_CASE": case,
            "PERFORMANCE_MODE": "1",
            "PERFORMANCE_PAYLOAD_BYTES": str(payload_bytes),
            "RETICULUM_PY_REPO": str(RNS_REPO),
            "LXMF_PY_REPO": str(LXMF_REPO),
            "LOG_DIR": str(sample_root / "logs"),
            "REPORT_PATH": str(report),
            "TIMEOUT_SECS": "90",
        }
    )
    completed = subprocess.run(
        [
            "/usr/bin/time",
            "-f",
            "__LXMF_E2E_CPU_SECONDS__ %U %S\n__LXMF_E2E_MAX_RSS_KIB__ %M",
            "bash",
            str(HARNESS),
        ],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    combined_output = completed.stdout + (completed.stderr or "")
    (sample_root / "harness.log").write_text(combined_output, encoding="utf-8")
    if completed.returncode != 0:
        raise RuntimeError(
            f"{workload} {implementation} sample {ordinal} failed; see {sample_root / 'harness.log'}"
        )
    data = json.loads(report.read_text(encoding="utf-8"))
    timing = data.get("performance")
    expected_boundary = {
        "cold_discovery": "cold_path_request_to_route_available",
        "link_setup": "link_request_to_active",
    }.get(workload, "enqueue_to_receiver_evidence")
    if not timing or timing.get("boundary") != expected_boundary:
        raise RuntimeError(f"{report} did not contain the E2E performance boundary")
    if timing.get("startup_included") or timing.get("route_warmup_included"):
        raise RuntimeError(f"{report} included setup in the timed boundary")
    cpu_match = re.search(r"__LXMF_E2E_CPU_SECONDS__ ([0-9.]+) ([0-9.]+)", completed.stderr)
    rss_match = re.search(r"__LXMF_E2E_MAX_RSS_KIB__ ([0-9]+)", completed.stderr)
    if not cpu_match or not rss_match:
        raise RuntimeError(f"{sample_root} did not contain process resource evidence")
    cpu_seconds = float(cpu_match.group(1)) + float(cpu_match.group(2))
    peak_rss_bytes = float(rss_match.group(1)) * 1024.0
    timing["cpu_seconds"] = cpu_seconds
    timing["peak_rss_bytes"] = peak_rss_bytes
    report.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return {
        "latency_ns": float(timing["enqueue_to_delivery_ns"]),
        "cpu_seconds": cpu_seconds,
        "peak_rss_bytes": peak_rss_bytes,
    }


def aggregate(samples: list[dict[str, float]], payload_bytes: int, retried: bool) -> dict[str, Any]:
    latency = [sample["latency_ns"] for sample in samples]
    median = statistics.median(latency)
    return {
        "sample_count": len(latency),
        "p50_ns": median,
        "p95_ns": percentile(latency, 0.95),
        "p99_ns": percentile(latency, 0.99),
        "messages_per_second": 1_000_000_000.0 / median,
        "bytes_per_second": payload_bytes * 1_000_000_000.0 / median,
        "median_cpu_seconds": statistics.median(sample["cpu_seconds"] for sample in samples),
        "median_peak_rss_bytes": statistics.median(
            sample["peak_rss_bytes"] for sample in samples
        ),
        "dispersion": {
            "p50_relative_mad": relative_mad(latency),
            "samples_ns": latency,
            "cpu_seconds": [sample["cpu_seconds"] for sample in samples],
            "peak_rss_bytes": [sample["peak_rss_bytes"] for sample in samples],
            "unstable_retry_performed": retried,
        },
    }


def main() -> int:
    global RESUME_EXISTING
    args = parse_args()
    RESUME_EXISTING = args.resume
    runs = args.runs if args.runs is not None else (1 if args.profile == "smoke" else 5)
    if runs < 1:
        print("e2e_performance: --runs must be positive", file=sys.stderr)
        return 2
    try:
        if not RNS_REPO.is_dir() or not LXMF_REPO.is_dir():
            raise RuntimeError("pinned Python references are missing under target/python-references")
        output_root = args.output.parent / "e2e-runs"
        rows = []
        for workload, rust_case, python_case, payload_bytes in WORKLOADS:
            samples: dict[str, list[dict[str, float]]] = {"rust": [], "python": []}
            cases = {"rust": rust_case, "python": python_case}
            for run_index in range(runs):
                first_is_rust = (len(rows) + run_index) % 2 == 0
                order = ("rust", "python") if first_is_rust else ("python", "rust")
                for implementation in order:
                    samples[implementation].append(
                        run_sample(
                            workload,
                            implementation,
                            cases[implementation],
                            payload_bytes,
                            run_index + 1,
                            output_root,
                        )
                    )
            retried = {"rust": False, "python": False}
            for implementation in ("rust", "python"):
                if runs > 1 and relative_mad([sample["latency_ns"] for sample in samples[implementation]]) > 0.20:
                    retried[implementation] = True
                    samples[implementation].append(
                        run_sample(
                            workload,
                            implementation,
                            cases[implementation],
                            payload_bytes,
                            runs + 1,
                            output_root,
                        )
                    )
                if runs > 1 and relative_mad([sample["latency_ns"] for sample in samples[implementation]]) > 0.20:
                    raise RuntimeError(
                        f"{workload} {implementation} relative MAD remained "
                        f"{relative_mad([sample['latency_ns'] for sample in samples[implementation]]):.2%} after retry"
                    )
            rust = aggregate(samples["rust"], payload_bytes, retried["rust"])
            python = aggregate(samples["python"], payload_bytes, retried["python"])
            is_discovery = workload == "cold_discovery"
            rows.append(
                {
                    "label": (
                        "Loopback TCP cold destination discovery"
                        if is_discovery
                        else (
                            "Loopback TCP link setup"
                            if workload == "link_setup"
                            else f"Loopback TCP {workload} delivery"
                        )
                    ),
                    "topology": "two-node loopback TCP, one Rust and one pinned-Python endpoint",
                    "route_state": "cold" if is_discovery else "warm",
                    "payload_size_bytes": payload_bytes,
                    "timed_boundary": {
                        "cold_discovery": "cold_path_request_to_route_available",
                        "link_setup": "link_request_to_active",
                    }.get(workload, "enqueue_to_receiver_evidence"),
                    "rust": rust,
                    "python": python,
                    "rust_p50_speedup_vs_python": python["p50_ns"] / rust["p50_ns"],
                }
            )
        report = {
            "e2e_profile": args.profile,
            "e2e_runs": runs,
            "python_rns_revision": git_revision(RNS_REPO),
            "python_lxmf_revision": git_revision(LXMF_REPO),
            "e2e_comparisons": rows,
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"E2E performance report: {args.output}")
        return 0
    except (OSError, KeyError, TypeError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        print(f"e2e_performance: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
