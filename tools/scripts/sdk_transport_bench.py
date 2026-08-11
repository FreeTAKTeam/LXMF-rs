#!/usr/bin/env python3
"""Benchmark in-process and daemon-backed SDK transports on one runner."""

from __future__ import annotations

import argparse
import json
import signal
import socket
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


TRANSPORTS = ("in_process", "zmq", "http", "unix")
MIN_IN_PROCESS_ITERATIONS = 1_000
IN_PROCESS_UNSUPPORTED = {
    "operation_registry": "in-process backend does not advertise the operation registry capability",
}
OPERATIONS = (
    "negotiate",
    "snapshot",
    "status",
    "poll_events",
    "operation_registry",
    "router_stats",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--daemon", type=Path, default=Path("target/release/reticulumd"))
    parser.add_argument(
        "--runner",
        type=Path,
        default=Path("target/release/examples/sdk_transport_benchmark"),
    )
    parser.add_argument(
        "--in-process-runner",
        type=Path,
        default=Path("target/release/examples/in_process_sdk_benchmark"),
    )
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--iterations", type=int, default=100)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_for_port(port: int, process: subprocess.Popen[str]) -> None:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"reticulumd exited with {process.returncode}")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError(f"reticulumd did not listen on port {port}")


def wait_for_unix_socket(path: Path, process: subprocess.Popen[str]) -> None:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"reticulumd exited with {process.returncode}")
        if path.exists():
            return
        time.sleep(0.05)
    raise RuntimeError(f"reticulumd did not listen on {path}")


def run_sample(
    runner: Path,
    transport: str,
    endpoint: str,
    operation: str,
    iterations: int,
) -> dict:
    command = [
        str(runner),
        "--transport",
        transport,
        "--endpoint",
        endpoint,
        "--operation",
        operation,
        "--iterations",
        str(iterations),
    ]
    output = subprocess.run(command, capture_output=True, text=True)
    if output.returncode != 0:
        raise RuntimeError(
            f"SDK transport workload failed ({' '.join(command)}): {output.stderr.strip()}"
        )
    return json.loads(output.stdout)


def relative_mad(values: list[float]) -> float:
    median = statistics.median(values)
    return statistics.median(abs(value - median) for value in values) / median


def run_isolated_sample(
    args: argparse.Namespace,
    temp_path: Path,
    transport: str,
    operation: str,
    sample_index: int,
) -> dict:
    if transport == "in_process":
        return run_sample(
            args.in_process_runner,
            transport,
            "in-process://local",
            operation,
            max(args.iterations, MIN_IN_PROCESS_ITERATIONS),
        )
    rpc_port, zmq_port = free_port(), free_port()
    log_path = temp_path / f"reticulumd-{sample_index:02}-{transport}-{operation}.log"
    with log_path.open("w", encoding="utf-8") as log:
        process = subprocess.Popen(
            [
                str(args.daemon),
                "--rpc",
                f"127.0.0.1:{rpc_port}",
                "--rpc-unix",
                str(temp_path / f"rpc-{sample_index}.sock"),
                "--zmq-rpc-endpoint",
                f"tcp://127.0.0.1:{zmq_port}",
                "--db",
                str(temp_path / f"reticulum-{sample_index}.db"),
            ],
            stdout=log,
            stderr=subprocess.STDOUT,
            text=True,
        )
        try:
            wait_for_port(rpc_port, process)
            unix_path = temp_path / f"rpc-{sample_index}.sock"
            if transport == "http":
                endpoint = f"http://127.0.0.1:{rpc_port}"
            elif transport == "unix":
                wait_for_unix_socket(unix_path, process)
                endpoint = f"unix://{unix_path}"
            else:
                endpoint = f"tcp://127.0.0.1:{zmq_port}"
            return run_sample(args.runner, transport, endpoint, operation, args.iterations)
        finally:
            process.send_signal(signal.SIGINT)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=5)


def comparison_row(operation: str, args: argparse.Namespace, values: dict) -> dict:
    zmq = values[(operation, "zmq")]
    http = values[(operation, "http")]
    unix = values[(operation, "unix")]
    zmq_p50 = [float(item["p50_ns"]) for item in zmq]
    http_p50 = [float(item["p50_ns"]) for item in http]
    unix_p50 = [float(item["p50_ns"]) for item in unix]
    row = {
        "operation": operation,
        "runs": args.runs,
        "iterations_per_run": args.iterations,
        "zmq_p50_ns": statistics.median(zmq_p50),
        "http_p50_ns": statistics.median(http_p50),
        "unix_p50_ns": statistics.median(unix_p50),
        "http_unix_p50_ns": statistics.median(http_p50),
        "zmq_p95_ns": statistics.median(float(item["p95_ns"]) for item in zmq),
        "http_p95_ns": statistics.median(float(item["p95_ns"]) for item in http),
        "unix_p95_ns": statistics.median(float(item["p95_ns"]) for item in unix),
        "http_unix_p95_ns": statistics.median(float(item["p95_ns"]) for item in http),
        "zmq_p50_relative_mad": relative_mad(zmq_p50),
        "http_unix_p50_relative_mad": relative_mad(http_p50),
        "unix_p50_relative_mad": relative_mad(unix_p50),
        "raw_runs": {"zmq": zmq, "http": http, "unix": unix},
    }
    in_process = values.get((operation, "in_process"), [])
    if in_process:
        in_process_p50 = [float(item["p50_ns"]) for item in in_process]
        row.update(
            {
                "in_process_status": "measured",
                "in_process_iterations_per_run": max(
                    args.iterations, MIN_IN_PROCESS_ITERATIONS
                ),
                "in_process_p50_ns": statistics.median(in_process_p50),
                "in_process_p95_ns": statistics.median(
                    float(item["p95_ns"]) for item in in_process
                ),
                "in_process_p50_relative_mad": relative_mad(in_process_p50),
                "in_process_batch_size": in_process[0]["batch_size"],
                "in_process_timed_boundary": in_process[0]["timed_boundary"],
            }
        )
        row["raw_runs"]["in_process"] = in_process
    else:
        row.update(
            {
                "in_process_status": "not_supported",
                "in_process_reason": IN_PROCESS_UNSUPPORTED[operation],
            }
        )
    return row


def measured_variation(row: dict) -> list[float]:
    variations = [
        row["zmq_p50_relative_mad"],
        row["http_unix_p50_relative_mad"],
        row["unix_p50_relative_mad"],
    ]
    if row["in_process_status"] == "measured":
        variations.append(row["in_process_p50_relative_mad"])
    return variations


def retry_operation(args: argparse.Namespace, operation: str) -> dict:
    values: dict[tuple[str, str], list[dict]] = {}
    with tempfile.TemporaryDirectory(prefix="lxmf-sdk-transport-retry-") as temp:
        temp_path = Path(temp)
        sample_index = 0
        for run in range(args.runs):
            order = TRANSPORTS if run % 2 == 0 else tuple(reversed(TRANSPORTS))
            for transport in order:
                if transport == "in_process" and operation in IN_PROCESS_UNSUPPORTED:
                    continue
                result = run_isolated_sample(args, temp_path, transport, operation, sample_index)
                values.setdefault((operation, transport), []).append(result)
                sample_index += 1
    return comparison_row(operation, args, values)


def main() -> int:
    args = parse_args()
    if args.runs < 3 or args.iterations < 1:
        raise SystemExit("--runs must be at least 3 and --iterations must be positive")
    samples: dict[tuple[str, str], list[dict]] = {}
    with tempfile.TemporaryDirectory(prefix="lxmf-sdk-transport-bench-") as temp:
        temp_path = Path(temp)
        sample_index = 0
        for run in range(args.runs):
            order = TRANSPORTS if run % 2 == 0 else tuple(reversed(TRANSPORTS))
            for transport in order:
                for operation in OPERATIONS:
                    if transport == "in_process" and operation in IN_PROCESS_UNSUPPORTED:
                        continue
                    result = run_isolated_sample(
                        args, temp_path, transport, operation, sample_index
                    )
                    samples.setdefault((operation, transport), []).append(result)
                    sample_index += 1

    rows = []
    for operation in OPERATIONS:
        row = comparison_row(operation, args, samples)
        if max(measured_variation(row)) > 0.20:
            row = retry_operation(args, operation)
            row["automatic_retry"] = True
        if max(measured_variation(row)) > 0.20:
            raise RuntimeError(f"unstable SDK transport workload after retry: {operation}")
        rows.append(row)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps({"sdk_transport_comparisons": rows}, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
