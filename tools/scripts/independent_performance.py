#!/usr/bin/env python3
"""Measure rns-rs and LXMF-rs over the existing independent interop fixture."""

from __future__ import annotations

import argparse
import json
import os
import platform
import statistics
import subprocess
import time
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator

from independent_interop import (
    ROOT,
    build_lxmf_probe,
    build_peer,
    build_rns_rs_control,
    load_pins,
    prepare_peer,
)
from independent_interop_rns_rs import rns_link, rust_event, write_rns_rs_config
from independent_interop_support import (
    ManagedProcess,
    RnsRsControl,
    RnsRsNode,
    RustProbe,
    command_output,
    cpu_model,
    free_port,
    wait_until,
)
from performance_variation import classify_relative_mad


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/performance/independent.json")
    parser.add_argument("--peer-root", type=Path)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--links", type=int, default=1000)
    parser.add_argument("--link-timeout", type=float, default=300.0)
    parser.add_argument("--skip-50-mib", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    return parser.parse_args()


def percentile(samples: list[float], fraction: float) -> float:
    ordered = sorted(samples)
    return ordered[round((len(ordered) - 1) * fraction)]


def relative_mad(samples: list[float]) -> float:
    median = statistics.median(samples)
    mad = statistics.median(abs(sample - median) for sample in samples)
    return mad / median if median else 0.0


def process_stats(pid: int) -> dict[str, float]:
    stat = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8")
    fields = stat[stat.rfind(")") + 2 :].split()
    ticks = os.sysconf("SC_CLK_TCK")
    cpu_seconds = (int(fields[11]) + int(fields[12])) / ticks
    peak_kib = 0
    for line in Path(f"/proc/{pid}/status").read_text(encoding="utf-8").splitlines():
        if line.startswith("VmHWM:"):
            peak_kib = int(line.split()[1])
            break
    return {"cpu_seconds": cpu_seconds, "peak_rss_bytes": float(peak_kib * 1024)}


def sample_processes(processes: dict[str, ManagedProcess]) -> dict[str, dict[str, float]]:
    return {name: process_stats(process.process.pid) for name, process in processes.items()}


def resource_sample(
    implementation: str,
    elapsed: float,
    size: int,
    before: dict[str, dict[str, float]],
    after: dict[str, dict[str, float]],
    digest: str,
) -> dict[str, Any]:
    process_name = "rns_rs" if implementation == "rns_rs" else "lxmf_rs"
    return {
        "seconds": elapsed,
        "bytes": size,
        "sha256": digest,
        "cpu_seconds": after[process_name]["cpu_seconds"] - before[process_name]["cpu_seconds"],
        "peer_cpu_seconds": after["lxmf_rs" if process_name == "rns_rs" else "rns_rs"]["cpu_seconds"]
        - before["lxmf_rs" if process_name == "rns_rs" else "rns_rs"]["cpu_seconds"],
        "peak_rss_bytes": after[process_name]["peak_rss_bytes"],
        "peer_peak_rss_bytes": after["lxmf_rs" if process_name == "rns_rs" else "rns_rs"][
            "peak_rss_bytes"
        ],
    }


def aggregate_timing(samples: list[float]) -> dict[str, Any]:
    relative = relative_mad(samples)
    return {
        "sample_count": len(samples),
        "p50_seconds": statistics.median(samples),
        "p95_seconds": percentile(samples, 0.95),
        "p99_seconds": percentile(samples, 0.99),
        "samples_seconds": samples,
        "p50_relative_mad": relative,
        "variation_class": classify_relative_mad(relative),
    }


def aggregate_resources(samples: list[dict[str, Any]], size: int) -> dict[str, Any]:
    timing = aggregate_timing([float(sample["seconds"]) for sample in samples])
    mib = size / 1_048_576
    timing.update(
        {
            "bytes": size,
            "sha256": samples[0]["sha256"],
            "throughput_mib_per_second": mib / timing["p50_seconds"],
            "cpu_ms_per_mib": statistics.median(sample["cpu_seconds"] for sample in samples)
            * 1000.0
            / mib,
            "peer_cpu_ms_per_mib": statistics.median(
                sample["peer_cpu_seconds"] for sample in samples
            )
            * 1000.0
            / mib,
            "peak_rss_mib": statistics.median(
                sample["peak_rss_bytes"] for sample in samples
            )
            / 1_048_576,
            "peer_peak_rss_mib": statistics.median(
                sample["peer_peak_rss_bytes"] for sample in samples
            )
            / 1_048_576,
            "raw_samples": samples,
        }
    )
    return timing


@contextmanager
def session(
    root: Path,
    peer_root: Path,
    control_binary: Path,
    label: str,
) -> Iterator[tuple[RustProbe, RnsRsNode, RnsRsControl, dict[str, ManagedProcess], str, str]]:
    rns_port, rust_control, http_port, peer_control = (free_port() for _ in range(4))
    config_dir = root / "config" / label
    write_rns_rs_config(config_dir / "config", rns_port)
    logs = root / "logs"
    rust_process = ManagedProcess(
        f"LXMF-rs performance probe ({label})",
        [
            str(ROOT / "target/release/independent-interop-node"),
            "--name",
            f"lxmf-rs-perf-{label}",
            "--identity-seed",
            f"lxmf-rs-perf-{label}",
            "--control",
            f"127.0.0.1:{rust_control}",
            "--listen",
            f"127.0.0.1:{rns_port}",
        ],
        ROOT,
        logs / f"lxmf-rs-{label}.log",
        {"RUST_LOG": "warn"},
    )
    peer_process: ManagedProcess | None = None
    try:
        rust = RustProbe(rust_control)
        rust_status = wait_until("LXMF-rs performance control", lambda: rust.call("status"), timeout=15)
        peer_process = ManagedProcess(
            f"rns-rs performance node ({label})",
            [
                str(control_binary),
                "http",
                "--disable-auth",
                "--host",
                "127.0.0.1",
                "--port",
                str(http_port),
                "--interop-control",
                f"127.0.0.1:{peer_control}",
                "--config",
                str(config_dir),
            ],
            peer_root,
            logs / f"rns-rs-{label}.log",
            {"RUST_LOG": "warn"},
        )
        rns = RnsRsNode(http_port)
        control = RnsRsControl(peer_control, timeout=300)
        wait_until("rns-rs performance control", lambda: control.call("health"), timeout=15)
        wait_until("rns-rs performance HTTP", lambda: rns.get("/health"), timeout=15)
        rns_destination = rns.post(
            "/api/destination",
            {
                "type": "single",
                "app_name": "interop",
                "aspects": ["probe"],
                "direction": "in",
                "proof_strategy": "all",
            },
        )["dest_hash"]
        yield (
            rust,
            rns,
            control,
            {"lxmf_rs": rust_process, "rns_rs": peer_process},
            rust_status["destination_hash"],
            rns_destination,
        )
    finally:
        try:
            RustProbe(rust_control).call("shutdown")
        except Exception:
            pass
        if peer_process is not None:
            peer_process.stop()
        rust_process.stop()


def establish_rns_link(rust: RustProbe, rns: RnsRsNode, rust_destination: str) -> str:
    rns.post("/api/path/request", {"dest_hash": rust_destination})
    wait_until(
        "rns-rs performance path",
        lambda: any(
            row.get("hash") == rust_destination
            for row in rns.get(f"/api/paths?dest_hash={rust_destination}")["paths"]
        ),
    )
    created = rns.post("/api/link", {"dest_hash": rust_destination})
    rns_link(rns, created["link_id"])
    wait_until(
        "LXMF-rs performance Link",
        lambda: next(
            (
                row
                for row in rust.call("links")["links"]
                if row["link_id"] == created["link_id"] and row["state"] == "activated"
            ),
            None,
        ),
    )
    return str(created["link_id"])


def measure_control_plane(
    root: Path, peer_root: Path, control_binary: Path, runs: int
) -> tuple[dict[str, Any], dict[str, Any]]:
    cold: list[float] = []
    warm: list[float] = []
    links: list[float] = []
    for index in range(runs):
        with session(root, peer_root, control_binary, f"control-{index}") as (
            rust,
            rns,
            _control,
            _processes,
            rust_destination,
            _rns_destination,
        ):
            started = time.monotonic()
            rns.post("/api/path/request", {"dest_hash": rust_destination})
            wait_until(
                "cold rns-rs path",
                lambda: any(
                    row.get("hash") == rust_destination
                    for row in rns.get(f"/api/paths?dest_hash={rust_destination}")["paths"]
                ),
            )
            cold.append(time.monotonic() - started)
            started = time.monotonic()
            paths = rns.get(f"/api/paths?dest_hash={rust_destination}")["paths"]
            if not any(row.get("hash") == rust_destination for row in paths):
                raise AssertionError("warm path lookup did not return the cached path")
            warm.append(time.monotonic() - started)
            started = time.monotonic()
            created = rns.post("/api/link", {"dest_hash": rust_destination})
            rns_link(rns, created["link_id"])
            wait_until(
                "LXMF-rs measured Link",
                lambda: next(
                    (
                        row
                        for row in rust.call("links")["links"]
                        if row["link_id"] == created["link_id"] and row["state"] == "activated"
                    ),
                    None,
                ),
            )
            links.append(time.monotonic() - started)
            rns.post("/api/link/close", {"link_id": created["link_id"]})
    return ({"cold": aggregate_timing(cold), "warm": aggregate_timing(warm)}, aggregate_timing(links))


def measure_resources(
    root: Path,
    peer_root: Path,
    control_binary: Path,
    runs: int,
    size: int,
) -> dict[str, Any]:
    samples: dict[str, list[dict[str, Any]]] = {"rns_rs": [], "lxmf_rs": []}
    with session(root, peer_root, control_binary, f"resource-{size}") as (
        rust,
        rns,
        control,
        processes,
        rust_destination,
        _rns_destination,
    ):
        link_id = establish_rns_link(rust, rns, rust_destination)
        for index in range(runs):
            seed = 0x4C584D4600000000 + index
            prepared = control.call("prepare_resource", {"size": size, "seed": seed})
            rust.events(clear=True)
            before = sample_processes(processes)
            started = time.monotonic()
            control.call(
                "send_prepared_resource", {"link_id": link_id, "key": prepared["key"]}
            )
            received = rust_event(
                rust,
                lambda row: row.get("type") == "resource"
                and row.get("link_id") == link_id
                and row.get("details", {}).get("state") == "complete"
                and row.get("details", {}).get("bytes") == size
                and row.get("details", {}).get("sha256") == prepared["sha256"],
                "prepared rns-rs Resource at LXMF-rs",
                timeout=240,
            )
            elapsed = time.monotonic() - started
            after = sample_processes(processes)
            samples["rns_rs"].append(
                resource_sample(
                    "rns_rs", elapsed, size, before, after, received["details"]["sha256"]
                )
            )

            prepared = rust.call("prepare_resource", {"size": size, "seed": seed})
            control.call("clear_resource_events")
            before = sample_processes(processes)
            started = time.monotonic()
            rust.call(
                "send_prepared_resource", {"link_id": link_id, "key": prepared["key"]}
            )
            received = wait_until(
                "prepared LXMF-rs Resource at rns-rs",
                lambda: control.call("received_resource_digest", {"link_id": link_id}),
                timeout=240,
                interval=0.1,
            )
            elapsed = time.monotonic() - started
            if received != {"bytes": size, "sha256": prepared["sha256"]}:
                raise AssertionError(f"received Resource mismatch: {received}")
            after = sample_processes(processes)
            samples["lxmf_rs"].append(
                resource_sample(
                    "lxmf_rs", elapsed, size, before, after, received["sha256"]
                )
            )
    return {
        implementation: aggregate_resources(rows, size)
        for implementation, rows in samples.items()
    }


def measure_large_links(
    root: Path,
    peer_root: Path,
    control_binary: Path,
    count: int,
    timeout: float,
) -> dict[str, Any]:
    if count != 1000:
        return {"status": "not_supported", "reason": "release workload is defined as exactly 1000 Links"}
    try:
        with session(root, peer_root, control_binary, "links-1000") as (
            rust,
            rns,
            control,
            processes,
            rust_destination,
            _rns_destination,
        ):
            rns.post("/api/path/request", {"dest_hash": rust_destination})
            wait_until(
                "1000-Link path",
                lambda: any(
                    row.get("hash") == rust_destination
                    for row in rns.get(f"/api/paths?dest_hash={rust_destination}")["paths"]
                ),
            )
            before = sample_processes(processes)
            started = time.monotonic()
            batch_control = RnsRsControl(control.port, timeout=timeout)
            created = batch_control.call(
                "create_links", {"destination_hash": rust_destination, "count": count}
            )
            link_ids = created["link_ids"]
            if len(link_ids) != count:
                raise AssertionError(f"peer created {len(link_ids)} Links instead of {count}")
            wait_until(
                "1000 active rns-rs Links",
                lambda: control.call("link_event_counts")["established"] == count,
                timeout=timeout,
                interval=0.25,
            )
            wait_until(
                "1000 active LXMF-rs Links",
                lambda: sum(
                    row.get("state") == "activated" for row in rust.call("links")["links"]
                )
                == count,
                timeout=timeout,
                interval=0.25,
            )
            creation = time.monotonic() - started
            active = sample_processes(processes)
            teardown_started = time.monotonic()
            control.call("teardown_links", {"link_ids": link_ids})
            wait_until(
                "1000 Link teardown",
                lambda: sum(row.get("state") == "closed" for row in rust.call("links")["links"])
                == count,
                timeout=timeout,
                interval=0.25,
            )
            teardown = time.monotonic() - teardown_started
            return {
                "status": "measured",
                "links": count,
                "creation_seconds": creation,
                "teardown_seconds": teardown,
                "lxmf_rs": {
                    "cpu_seconds": active["lxmf_rs"]["cpu_seconds"]
                    - before["lxmf_rs"]["cpu_seconds"],
                    "peak_rss_mib": active["lxmf_rs"]["peak_rss_bytes"] / 1_048_576,
                    "role": "responder",
                },
                "rns_rs": {
                    "cpu_seconds": active["rns_rs"]["cpu_seconds"]
                    - before["rns_rs"]["cpu_seconds"],
                    "peak_rss_mib": active["rns_rs"]["peak_rss_bytes"] / 1_048_576,
                    "role": "initiator",
                },
            }
    except TimeoutError:
        return {
            "status": "not_supported",
            "reason": (
                f"pinned rns-rs public create_link surface did not complete exactly {count} "
                f"real Links within the bounded {timeout:.0f}-second workload; no smaller count substituted"
            ),
            "attempted_links": count,
            "timeout_seconds": timeout,
        }


def measured(value: float, **details: Any) -> dict[str, Any]:
    return {"status": "measured", "value": value, "details": details}


def public_cells(report: dict[str, Any]) -> dict[str, dict[str, Any]]:
    path = report["path_convergence"]
    link = report["link_setup"]
    cells: dict[str, dict[str, Any]] = {
        "packet_encode": {
            "rns_rs": {"status": "not_supported", "reason": "peer exposes network send APIs, not an isolated packet codec benchmark"}
        },
        "announce_validation": {
            "rns_rs": {"status": "not_supported", "reason": "peer does not expose an isolated announce-validation timing boundary"}
        },
        "path_convergence_cold": {
            "rns_rs": {
                "status": "measured",
                "value": path["cold"]["p50_seconds"],
                "p99": path["cold"]["p99_seconds"],
                "details": path["cold"],
            }
        },
        "path_lookup_warm": {
            "rns_rs": {
                "status": "measured",
                "value": path["warm"]["p50_seconds"],
                "p99": path["warm"]["p99_seconds"],
                "details": path["warm"],
            }
        },
        "link_setup": {
            "rns_rs": {
                "status": "measured",
                "value": link["p50_seconds"],
                "p99": link["p99_seconds"],
                "details": link,
            }
        },
    }
    for size_key, row_id, ram_id, cpu_id in (
        ("1048576", "resource_1mib", "resource_1mib_peak_ram", "resource_1mib_cpu"),
        ("52428800", "resource_50mib", "resource_50mib_peak_ram", "resource_50mib_cpu"),
    ):
        resource = report["resources"].get(size_key)
        if resource is None:
            continue
        for implementation in ("lxmf_rs", "rns_rs"):
            result = resource[implementation]
            cells.setdefault(row_id, {})[implementation] = measured(
                result["throughput_mib_per_second"],
                p50_seconds=result["p50_seconds"],
                p99_seconds=result["p99_seconds"],
                bytes=result["bytes"],
                sha256=result["sha256"],
            )
        cells.setdefault(ram_id, {})["rns_rs"] = measured(
            resource["rns_rs"]["peak_rss_mib"], workload_bytes=resource["rns_rs"]["bytes"]
        )
        cells.setdefault(ram_id, {})["lxmf_rs"] = measured(
            resource["lxmf_rs"]["peak_rss_mib"], workload_bytes=resource["lxmf_rs"]["bytes"]
        )
        cells.setdefault(cpu_id, {})["rns_rs"] = measured(
            resource["rns_rs"]["cpu_ms_per_mib"], workload_bytes=resource["rns_rs"]["bytes"]
        )
        cells.setdefault(cpu_id, {})["lxmf_rs"] = measured(
            resource["lxmf_rs"]["cpu_ms_per_mib"], workload_bytes=resource["lxmf_rs"]["bytes"]
        )
    large = report["active_links_1000"]
    if large.get("status") == "measured":
        cells["active_links_1000"] = {
            implementation: measured(
                1000,
                creation_seconds=large["creation_seconds"],
                teardown_seconds=large["teardown_seconds"],
                **large[implementation],
            )
            for implementation in ("lxmf_rs", "rns_rs")
        }
    else:
        cells["active_links_1000"] = {
            implementation: {"status": "not_supported", "reason": large["reason"]}
            for implementation in ("lxmf_rs", "rns_rs")
        }
    return cells


def main() -> int:
    args = parse_args()
    if args.samples < 1:
        print("independent_performance: --samples must be positive", file=os.sys.stderr)
        return 2
    output_root = args.output.resolve().parent / "independent-runs"
    output_root.mkdir(parents=True, exist_ok=True)
    try:
        pins = load_pins()
        peer = pins["peers"]["rns_rs"]
        peer_root = prepare_peer(peer, ROOT / "target/interop/independent/external", args.peer_root)
        if args.skip_build:
            peer_binary = peer_root / peer["binary"]
            control_binary = ROOT / "target/release/lxmf-rs-rns-rs-control"
            if not peer_binary.is_file() or not control_binary.is_file():
                raise RuntimeError("--skip-build requires existing rns-rs and adapter binaries")
        else:
            build_peer(peer, peer_root)
            build_lxmf_probe()
            control_binary = build_rns_rs_control()
        path, link = measure_control_plane(output_root, peer_root, control_binary, args.samples)
        resources = {
            str(1_048_576): measure_resources(
                output_root, peer_root, control_binary, args.samples, 1_048_576
            )
        }
        if not args.skip_50_mib:
            resources[str(50 * 1_048_576)] = measure_resources(
                output_root, peer_root, control_binary, args.samples, 50 * 1_048_576
            )
        report: dict[str, Any] = {
            "schema": "lxmf-rs-independent-performance-v1",
            "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "environment": {
                "os": platform.platform(),
                "architecture": platform.machine(),
                "cpu": cpu_model(),
                "rustc": command_output(["rustc", "--version"]),
                "cargo": command_output(["cargo", "--version"]),
                "python": platform.python_version(),
                "build_mode": "release",
                "same_runner": True,
            },
            "implementations": {
                "lxmf_rs": command_output(["git", "rev-parse", "HEAD"], ROOT),
                "rns_rs": command_output(["git", "rev-parse", "HEAD"], peer_root),
            },
            "configuration": {
                "samples": args.samples,
                "topology": "two-node isolated loopback TCP",
                "resource_payload": "seeded xorshift64 bytes prepared outside timed boundary",
                "resource_boundary": "prepared send dispatch to receiver SHA-256 evidence",
                "large_links": args.links,
                "large_links_timeout_seconds": args.link_timeout,
            },
            "path_convergence": path,
            "link_setup": link,
            "resources": resources,
            "active_links_1000": measure_large_links(
                output_root, peer_root, control_binary, args.links, args.link_timeout
            ),
        }
        report["public_cells"] = public_cells(report)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"Independent performance report: {args.output}")
        return 0
    except (OSError, KeyError, TypeError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"independent_performance: {error}", file=os.sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
