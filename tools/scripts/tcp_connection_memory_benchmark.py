#!/usr/bin/env python3
"""Run repeatable TCP connection RSS/virtual-memory scenarios.

The benchmark keeps client sockets in the same process as the Rust TCP server.
That adds a small, consistent client-side cost, while isolating the comparison
from external process timing. Linux reports RSS, peak RSS and virtual memory
from /proc; other platforms still exercise the workload and report task and
throughput counters with memory fields set to null.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
PACKAGE = "reticulum-rs-transport"
EXAMPLE = "tcp_connection_memory"


def run(command: list[str], *, capture: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--counts", default="100,500,1000")
    parser.add_argument("--activities", default="idle,small")
    parser.add_argument("--mtu", type=int, default=262_144)
    parser.add_argument("--settle-ms", type=int, default=500)
    parser.add_argument("--broadcasts", type=int, default=0)
    parser.add_argument("--json-out", type=Path)
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()

    counts = [int(value) for value in args.counts.split(",") if value]
    activities = [value for value in args.activities.split(",") if value]
    if not args.skip_build:
        run(["cargo", "build", "--release", "-p", PACKAGE, "--example", EXAMPLE])

    executable = ROOT / "target" / "release" / "examples" / EXAMPLE
    if sys.platform == "win32":
        executable = executable.with_suffix(".exe")

    results: list[dict[str, Any]] = []
    for activity in activities:
        for count in counts:
            completed = run(
                [
                    str(executable),
                    "--connections",
                    str(count),
                    "--activity",
                    activity,
                    "--mtu",
                    str(args.mtu),
                    "--settle-ms",
                    str(args.settle_ms),
                    "--broadcasts",
                    str(args.broadcasts),
                ],
                capture=True,
            )
            results.append(json.loads(completed.stdout))

    output = {
        "schema": "lxmf-rs-tcp-connection-memory-v1",
        "git_revision": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip(),
        "results": results,
    }
    text = json.dumps(output, indent=2, sort_keys=True) + "\n"
    if args.json_out:
        destination = args.json_out if args.json_out.is_absolute() else ROOT / args.json_out
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(text, encoding="utf-8")
    sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
