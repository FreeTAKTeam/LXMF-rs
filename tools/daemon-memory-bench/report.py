#!/usr/bin/env python3
"""Summarize two validated exploratory daemon smoke runs."""
import csv
import hashlib
import inspect
import json
import math
import statistics
import sys
import types
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

MIB = 1024 * 1024


def load_frozen_validator(root, spec):
    path = root / "harness" / "validate.py"
    source = path.read_bytes()
    if hashlib.sha256(source).hexdigest() != spec["harness_sha256"]["validate.py"]:
        raise ValueError("the frozen validator hash differs from the manifest")
    validator = types.ModuleType("frozen_daemon_validator")
    validator.__file__ = str(path)
    exec(compile(source, str(path), "exec"), validator.__dict__)
    return validator


def percentile(values, fraction):
    values = sorted(values)
    if not values:
        return None
    index = (len(values) - 1) * fraction
    low = int(index)
    high = min(low + 1, len(values) - 1)
    return values[low] + (values[high] - values[low]) * (index - low)


def slope(rows, field):
    xs = [(row["monotonic_ns"] - rows[0]["monotonic_ns"]) / 3_600_000_000_000 for row in rows]
    ys = [row["cgroup"]["memory.stat"][field] / MIB for row in rows]
    mean_x, mean_y = statistics.mean(xs), statistics.mean(ys)
    divisor = sum((x - mean_x) ** 2 for x in xs)
    return sum((x - mean_x) * (y - mean_y) for x, y in zip(xs, ys)) / divisor if divisor else 0.0


def pressure_total(value):
    for line in value.splitlines():
        fields = line.split()
        if fields and fields[0] == "some":
            return int(next(field.split("=", 1)[1] for field in fields if field.startswith("total=")))
    return 0


def valid_pressure(value):
    if not isinstance(value, str):
        return False
    lines = [line.split() for line in value.splitlines() if line.split()]
    return all(any(fields[0] == kind and any(part.startswith("total=") and part[6:].isdigit() for part in fields[1:])
                   for fields in lines) for kind in ("some", "full"))


def seconds_above_high(samples, end_ns, high):
    boundaries = [row["monotonic_ns"] for row in samples[1:]] + [end_ns]
    return sum((end - row["monotonic_ns"]) / 1e9 for row, end in zip(samples, boundaries)
               if row["cgroup"]["memory.current"] > high)


def process_memory(sample, field):
    total = 0
    for process in sample["processes"]:
        rollup = process.get("smaps_rollup")
        if rollup is None:
            return None
        for line in rollup.splitlines():
            if line.startswith(field + ":"):
                total += int(line.split()[1]) * 1024
                break
        else:
            return None
    return total


def process_io_delta(samples, field):
    readings = []
    for sample in samples:
        processes = sample["processes"]
        if not processes or any("io" not in process for process in processes):
            continue
        readings.append({(process["pid"], process["start_ticks"]): {
            key: int(value) for key, value in
            (line.split(": ", 1) for line in process["io"].splitlines())
        } for process in processes})
    if len(readings) < 2 or any(item.keys() != readings[0].keys() for item in readings[1:]):
        return None
    return sum(readings[-1][pid][field] - readings[0][pid][field] for pid in readings[0])


def recovery_metrics(samples, start_ns, duration_seconds):
    rows = [row for row in samples if row.get("phase") == "recovery"]
    if (len(rows) < 2 or rows[0]["monotonic_ns"] != start_ns
            or rows[-1]["monotonic_ns"] - start_ns < duration_seconds * 1e9):
        raise ValueError("recovery observation window was incomplete")
    return {f"recovery_{field}_change_mib": (rows[-1]["cgroup"]["memory.stat"][field]
                                              - rows[0]["cgroup"]["memory.stat"][field]) / MIB
            for field in ("anon", "file")}


def guest_pair_errors(root, run_ids, spec):
    errors = []
    vm = root.parent / "vm-image"
    boots, markers = [], []

    def require(condition, message):
        if not condition:
            raise ValueError(message)

    try:
        expected_roots = set(spec["subject_runtime_trees_sha256"]) | set(spec["subject_runtime_files_sha256"]) | {spec["rust_executable"]}
        inventory_path = root / "frozen-input-sha256.json"
        inventory = json.loads(inventory_path.read_text())
        required = {"experiment.json", "host-boot-id", "guest/guest-run-frozen-profile.sh",
                    "guest/private-runtime-setup-v3.py", "guest/guest-cold-cache-control.py", "guest/cache-residency-audit-v2.py"}
        required |= {f"harness/{name}" for name in spec["harness_sha256"]}
        required |= {f"fixture/{name}" for name in spec["fixture_sha256"]}
        if "guest_staging_sha256" in spec:
            required.add("guest/copy-frozen-inputs.py")
        require(isinstance(inventory, dict) and required <= inventory.keys(), "incomplete frozen inventory")
        for name, expected in inventory.items():
            path = (root / name).resolve()
            require(root.resolve() in path.parents and isinstance(expected, str)
                    and hashlib.sha256(path.read_bytes()).hexdigest() == expected, "frozen input changed")
        inventory_hash = hashlib.sha256(inventory_path.read_bytes()).hexdigest()
        if "guest_staging_sha256" in spec:
            require(inventory["guest/copy-frozen-inputs.py"] == spec["guest_staging_sha256"], "guest staging helper differs")
        require(all(inventory[f"harness/{name}"] == value for name, value in spec["harness_sha256"].items())
                and all(inventory[f"fixture/{name}"] == value for name, value in spec["fixture_sha256"].items()),
                "frozen inventory differs from the manifest")
        validator = load_frozen_validator(root, spec)
        minimum_memory, minimum_disk = spec["host_minimum_available_memory_bytes"], spec["host_minimum_free_disk_bytes"]
        require(type(minimum_memory) is int and minimum_memory > 0 and type(minimum_disk) is int and minimum_disk > 0,
                "invalid host safety thresholds")
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        return [f"guest frozen inputs are unavailable or malformed ({type(error).__name__}: {error})"]

    for run_id in run_ids:
        try:
            host = json.loads((vm / f"{run_id}-host-monitor-summary.json").read_text())
            control = json.loads((vm / "controls" / f"{run_id}.json").read_text())
            output = vm / "output" / run_id
            status = (output / "guest-status.txt").read_text().strip()
            run = root / "runs" / run_id
            manifest = json.loads((run / "manifest.json").read_text())
            cache = json.loads((run / "guest-cache-control.json").read_text())
            residency = cache["residency"]
            roots = residency["roots"]
            marker, boot = cache["guest_marker"], cache["guest_boot_id"]
            validator.verify_guest_outcome(run, spec, status, (output / "runner-status.txt").read_text().strip(),
                                           (root / "experiment.json").read_bytes())
            require(manifest["guest_cache_control"] == cache == json.loads((output / "cache-control.output").read_text()),
                    "cache helper output differs")
            if "guest_staging_sha256" in spec:
                staged = {name: value for name, value in inventory.items()
                          if name == "experiment.json" or name.startswith(("fixture/", "harness/"))}
                require(json.loads((output / "staging-evidence.json").read_text()) == {"staged_sha256": staged},
                        "guest staged inputs differ from frozen inventory")
            require(control["host_boot_id"] == cache["host_boot_id"] == spec["host_boot_id"]
                    and control["guest_image_sha256"] == spec["guest_image_sha256"]
                    and control["frozen_inventory_sha256"] == inventory_hash, "guest frozen-input binding differs")
            require(isinstance(marker, str) and bool(marker) and control["guest_marker"] == marker
                    and isinstance(boot, str) and bool(boot) and boot != spec["host_boot_id"]
                    and cache["virtualization"] in ("kvm", "qemu"), "invalid guest identity")
            require(isinstance(roots, dict) and set(roots) == expected_roots
                    and type(residency["resident_pages"]) is int and residency["resident_pages"] == 0
                    and residency["skipped_symlinks"] == []
                    and all(type(value["files"]) is int and value["files"] > 0
                            and type(value["resident_pages"]) is int and value["resident_pages"] == 0 for value in roots.values()),
                    "runtime cache was not cold")
            input_errors = []
            samples = validator.jsonl(vm / f"{run_id}-host-samples.jsonl", input_errors)
            require(not input_errors and len(samples) >= 3, "host samples unavailable")

            def check_host_row(row):
                require(row["run_id"] == run_id and row["guest_marker"] == marker
                        and type(row["monotonic_ns"]) is int and row["monotonic_ns"] >= 0
                        and isinstance(row["utc"], str) and bool(row["utc"])
                        and all(valid_pressure(row[f"{kind}_pressure"]) for kind in ("cpu", "memory", "io"))
                        and all(type(row["swap_counters"][key]) is int and row["swap_counters"][key] >= 0 for key in ("pswpin", "pswpout"))
                        and type(row["available_memory_bytes"]) is int and row["available_memory_bytes"] >= minimum_memory
                        and type(row["free_disk_bytes"]) is int and row["free_disk_bytes"] >= minimum_disk,
                        "invalid or unsafe host sample")

            preflight = json.loads((vm / f"{run_id}-host-preflight.json").read_text())
            check_host_row(preflight)
            require(preflight["qemu_pid"] is None and preflight["monotonic_ns"] <= samples[0]["monotonic_ns"],
                    "invalid host preflight")
            for row in samples:
                check_host_row(row)
            times = [row["monotonic_ns"] for row in samples]
            gaps = [b - a for a, b in zip(times, times[1:])]
            live_pids = [row["qemu_pid"] for row in samples[1:-1]]
            require(all(gap >= 0 for gap in gaps) and samples[0]["qemu_pid"] is None and samples[-1]["qemu_pid"] is None
                    and all(type(pid) is int and pid > 0 for pid in live_pids) and len(set(live_pids)) == 1,
                    "invalid host sample ordering or QEMU lifetime")
            expected = (times[-1] - times[0]) // 5_000_000_000 + 1
            coverage = len({min(expected - 1, (stamp - times[0]) // 5_000_000_000) for stamp in times}) / expected
            max_gap = max(gaps) / 1e9
            require(host["capture_valid"] is True and host["final_safety_ok"] is True
                    and host["run_id"] == run_id and host["guest_marker"] == marker and host["qemu_exit_code"] == 0
                    and type(host["qemu_exit_code"]) is int and host["termination_reason"] == "normal"
                    and type(host["coverage"]) in (int, float) and math.isclose(host["coverage"], coverage, abs_tol=1e-9, rel_tol=0)
                    and type(host["max_gap_seconds"]) in (int, float) and math.isclose(host["max_gap_seconds"], max_gap, abs_tol=1e-9, rel_tol=0)
                    and coverage >= 0.99 and max_gap <= 30, "host monitoring or QEMU completion failed")
            boots.append(boot)
            markers.append(marker)
        except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
            errors.append(f"{run_id}: guest evidence is unavailable or malformed ({type(error).__name__}: {error})")
    if len(boots) > 1 and (len(set(boots)) != len(boots) or len(set(markers)) != len(markers)):
        errors.append("guest boots and markers must be distinct for every run")
    if spec.get("generator_mode") == "prepared_packets" and len(run_ids) > 1:
        try:
            packet_ids = [{row["transient_id"] for row in validator.jsonl(root / "runs" / run_id / "prepared-packets.jsonl")}
                          for run_id in run_ids]
            if any(packet_ids[first] & packet_ids[second] for first in range(len(packet_ids)) for second in range(first + 1, len(packet_ids))):
                errors.append("prepared propagation packets were reused across runs")
        except (OSError, KeyError, TypeError, ValueError) as error:
            errors.append(f"prepared packet freshness evidence unavailable: {error}")
    return errors


def load_run(root, run_id, validator):
    run = root / "runs" / run_id
    spec = json.loads((run / "experiment.json").read_text())
    manifest = json.loads((run / "manifest.json").read_text())
    summary = json.loads((run / "summary.json").read_text())
    samples = validator.jsonl(run / "samples.jsonl")
    sender = validator.jsonl(run / "sender.jsonl")
    receiver = validator.jsonl(run / "receiver.jsonl")
    recovery = {"recovery_anon_change_mib": None, "recovery_file_change_mib": None}
    if spec.get("recovery_seconds"):
        if summary.get("recovery_evaluated") is not True:
            raise ValueError(f"{run_id}: recovery was not validated")
        recovery = recovery_metrics(samples, manifest["phase_times_ns"]["recovery"], spec["recovery_seconds"])
    attempts = {x["sequence"]: x for x in sender if x["event"] == "attempted"}
    finished = {x["sequence"]: x for x in sender if x["event"] == "sender_final" and x["state"] in (4, 8)}
    receipts = {x["sha256"]: x for x in receiver if x["event"] == "received"}
    accepted_latency = [(finished[n]["monotonic_ns"] - row["monotonic_ns"]) / 1e9 for n, row in attempts.items() if n in finished]
    retrieval_latency = [(receipts[row["sha256"]]["monotonic_ns"] - row["monotonic_ns"]) / 1e9 for row in attempts.values() if row["sha256"] in receipts]
    idle = spec["profile"] in ("idle", "preflight-only-idle-dry-run")
    ready = next((x["monotonic_ns"] for x in receiver if x["event"] == "node_ready"), None)
    post_reconnect_latency = [(x["monotonic_ns"] - ready) / 1e9 for x in receiver if x["event"] == "received"] if ready is not None else []
    start = manifest["phase_times_ns"]["idle"] if idle else min((x["scheduled_monotonic_ns"] for x in attempts.values()), default=samples[0]["monotonic_ns"])
    steady = [row for row in samples if start <= row["monotonic_ns"] < start + spec["duration_seconds"] * 1e9]
    if len(steady) < 2:
        raise ValueError(f"{run_id}: not enough steady-state samples")
    events_start, events_end = samples[0]["cgroup"]["memory.events"], samples[-1]["cgroup"]["memory.events"]
    p0 = pressure_total(samples[0]["cgroup"]["memory.pressure"])
    p1 = pressure_total(samples[-1]["cgroup"]["memory.pressure"])
    pss = [value for row in steady if (value := process_memory(row, "Pss")) is not None]
    swap = [value for row in steady if (value := process_memory(row, "Swap")) is not None]
    return {"id": run_id, "spec": spec, "manifest": manifest, "summary": summary, "samples": samples,
            **recovery,
            "start": start, "steady": steady, "sender": sender, "receiver": receiver,
            "window_confirmed": sum(start <= row["monotonic_ns"] < start + spec["duration_seconds"] * 1e9 for row in finished.values()),
            "window_received": sum(start <= row["monotonic_ns"] < start + spec["duration_seconds"] * 1e9 for row in receipts.values()),
            "drain_seconds": max(0, (manifest["phase_times_ns"].get("normal_end", start) - start) / 1e9 - spec["duration_seconds"]),
            "median_current_mib": statistics.median(x["cgroup"]["memory.current"] for x in steady) / MIB,
            "peak_current_mib": max(x["cgroup"]["memory.current"] for x in steady) / MIB,
            "peak_since_ready_mib": max(x["cgroup"].get("memory.peak_since_ready", 0) for x in samples) / MIB,
            "median_anon_mib": statistics.median(x["cgroup"]["memory.stat"]["anon"] for x in steady) / MIB,
            "median_file_mib": statistics.median(x["cgroup"]["memory.stat"]["file"] for x in steady) / MIB,
            "median_pss_mib": statistics.median(pss) / MIB if pss else None,
            "median_process_swap_mib": statistics.median(swap) / MIB if swap else None,
            "process_read_bytes": process_io_delta(steady, "read_bytes"),
            "process_write_bytes": process_io_delta(steady, "write_bytes"),
            "max_accepted_minus_received": max((x.get("ledger_counts", {}).get("accepted_minus_received", 0) for x in samples), default=0),
            "max_native_store_bytes": max((x.get("storage", {}).get("subject_file_bytes", 0) for x in samples), default=0),
            "anon_slope_mib_per_hour": slope(steady, "anon"),
            "file_slope_mib_per_hour": slope(steady, "file"),
            "high_events": events_end.get("high", 0) - events_start.get("high", 0),
            "oom_events": events_end.get("oom", 0),
            "memory_psi_some_seconds": (p1-p0) / 1_000_000,
            "host_memory_psi_some_seconds": (pressure_total(samples[-1]["host_memory_pressure"]) - pressure_total(samples[0]["host_memory_pressure"])) / 1_000_000,
            "host_io_psi_some_seconds": (pressure_total(samples[-1]["host_io_pressure"]) - pressure_total(samples[0]["host_io_pressure"])) / 1_000_000,
            "approx_seconds_above_high": seconds_above_high(steady, start + spec["duration_seconds"] * 1_000_000_000, spec["limits"]["memory_high_bytes"]),
            "cpu_seconds": (steady[-1]["cgroup"]["cpu.stat"]["usage_usec"]-steady[0]["cgroup"]["cpu.stat"]["usage_usec"]) / 1_000_000,
            "cpu_sampled_seconds": (steady[-1]["monotonic_ns"] - steady[0]["monotonic_ns"]) / 1e9,
            "acceptance_latency_seconds": {str(p): percentile(accepted_latency, p/100) for p in (50, 95, 99)},
            "retrieval_latency_seconds": {str(p): percentile(post_reconnect_latency, p/100) for p in (50, 95, 99)},
            "delivery_latency_seconds": {str(p): percentile(retrieval_latency, p/100) for p in (50, 95, 99)},
            "offline_wait_seconds": {str(p): percentile(retrieval_latency, p/100) for p in (50, 95, 99)}}


def failed_report(out, errors):
    for name in ("summary.csv", "memory.png", "pressure.png", "deliveries.png", "state.png"):
        (out / name).unlink(missing_ok=True)
    lines = ["# Exploratory daemon smoke", "", "The pair did not pass its validation gate. No comparative memory conclusion is supported.", ""]
    lines.extend(f"- {error}" for error in errors)
    (out / "report.md").write_text("\n".join(lines) + "\n")
    print(out / "report.md")
    return 1


def run_report():
    root = Path(sys.argv[1]).resolve()
    run_ids = sys.argv[2:]
    if len(run_ids) != 2:
        raise SystemExit("provide one Python and one Rust run ID")
    out = root / "report"
    out.mkdir(exist_ok=True)
    summaries = []
    pair_errors = []
    master = json.loads((root / "experiment.json").read_text())
    if master.get("guest_cold_cache"):
        pair_errors.extend(guest_pair_errors(root, run_ids, master))
        if pair_errors:
            return failed_report(out, pair_errors)
    validator = None
    try:
        validator = load_frozen_validator(root, master)
    except (OSError, ValueError, KeyError) as error:
        pair_errors.append(f"frozen validator unavailable: {error}")
    for run_id in run_ids:
        path = root / "runs" / run_id / "summary.json"
        summaries.append(json.loads(path.read_text()) if path.exists() else None)
        frozen = root / "runs" / run_id / "experiment.json"
        if not frozen.exists() or frozen.read_bytes() != (root / "experiment.json").read_bytes():
            pair_errors.append(f"{run_id}: frozen inputs differ")
        run = root / "runs" / run_id
        if validator is not None and summaries[-1] is not None and (run / "manifest.json").exists():
            manifest = json.loads((run / "manifest.json").read_text())
            if hasattr(validator, "load_inputs"):
                rows = validator.load_inputs(run, manifest)
                fresh = validator.validate(master, manifest, *rows)
            elif "input_errors" in inspect.signature(validator.validate).parameters:
                errors = {name: [] for name in ("samples", "sender", "receiver")}
                rows = [validator.jsonl(run / f"{name}.jsonl", errors[name]) for name in errors]
                fresh = validator.validate(master, manifest, *rows, errors)
            else:
                fresh = validator.validate(master, manifest,
                                           validator.jsonl(run / "samples.jsonl"),
                                           validator.jsonl(run / "sender.jsonl"), validator.jsonl(run / "receiver.jsonl"))
            if json.dumps(fresh, sort_keys=True, allow_nan=False) != json.dumps(summaries[-1], sort_keys=True, allow_nan=False):
                pair_errors.append(f"{run_id}: saved summary differs from fresh frozen-validator output")
    manifests = [root / "runs" / run_id / "manifest.json" for run_id in run_ids]
    if all(path.exists() for path in manifests):
        metadata = [json.loads(path.read_text()) for path in manifests]
        if {item.get("subject") for item in metadata} != {"python", "rust"}:
            pair_errors.append("the pair does not contain one Python and one Rust subject")
        for item in metadata:
            group = item.get("cgroup")
            settings = item.get("effective_limits", {}).get(group, {})
            expected = {"memory.high": "805306368", "memory.max": "1073741824", "memory.swap.max": "0", "cpu.max": "200000 100000"}
            if any(settings.get(key) != value for key, value in expected.items()):
                pair_errors.append(f"{item.get('run_id')}: effective resource limits differ")
    else:
        pair_errors.append("one or both run manifests are unavailable")
    if pair_errors or any(not result or not result["capture_valid"] or not result["functional_pass"] for result in summaries):
        lines = [f"Pair check: {error}" for error in pair_errors]
        for run_id, result in zip(run_ids, summaries):
            lines.append(f"{run_id}: {json.dumps(result, sort_keys=True) if result else 'summary unavailable'}")
        return failed_report(out, lines)
    runs = [load_run(root, run_id, validator) for run_id in run_ids]
    idle = master["profile"] in ("idle", "preflight-only-idle-dry-run")
    offline = master["profile"] in ("offline", "preflight-only-offline-recovery-dry-run")
    normal = master["profile"] in ("normal", "high-load", "preflight-only-normal-dry-run")
    axis_label = "Minutes from idle observation start" if idle else "Minutes from first scheduled message"
    fields = ["id", "median_current_mib", "peak_current_mib", "peak_since_ready_mib", "median_anon_mib", "median_file_mib", "median_pss_mib", "median_process_swap_mib", "process_read_bytes", "process_write_bytes", "max_accepted_minus_received", "max_native_store_bytes", "anon_slope_mib_per_hour", "file_slope_mib_per_hour", "recovery_anon_change_mib", "recovery_file_change_mib", "cpu_seconds", "cpu_sampled_seconds", "high_events", "oom_events", "memory_psi_some_seconds", "host_memory_psi_some_seconds", "host_io_psi_some_seconds", "approx_seconds_above_high"]
    with (out / "summary.csv").open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows({key: run[key] for key in fields} for run in runs)

    fig, axes = plt.subplots(3, 1, figsize=(10, 9), sharex=True, constrained_layout=True)
    for run in runs:
        xs = [(sample["monotonic_ns"] - run["start"]) / 60e9 for sample in run["samples"]]
        axes[0].plot(xs, [x["cgroup"]["memory.current"] / MIB for x in run["samples"]], label=run["id"])
        axes[1].plot(xs, [x["cgroup"]["memory.stat"]["anon"] / MIB for x in run["samples"]], label=run["id"])
        axes[2].plot(xs, [x["cgroup"]["memory.stat"]["file"] / MIB for x in run["samples"]], label=run["id"])
    for axis, title in zip(axes, ("Cgroup current", "Anonymous memory", "File memory")):
        axis.set_ylabel("MiB")
        axis.set_title(title)
        axis.grid(alpha=0.2)
        axis.legend()
    axes[-1].set_xlabel(axis_label)
    label = "idle" if idle else "offline delivery" if offline else "normal polled propagation" if normal else "smoke"
    fig.suptitle(f"Exploratory synthetic daemon {label}")
    fig.savefig(out / "memory.png", dpi=150)
    plt.close(fig)

    fig, axes = plt.subplots(2, 1, figsize=(10, 6), sharex=True, constrained_layout=True)
    for run in runs:
        xs = [(sample["monotonic_ns"] - run["start"]) / 60e9 for sample in run["samples"]]
        events = [x["cgroup"]["memory.events"].get("high", 0) for x in run["samples"]]
        psi = [pressure_total(x["cgroup"]["memory.pressure"]) for x in run["samples"]]
        axes[0].plot(xs[1:], [b - a for a, b in zip(events, events[1:])], label=run["id"])
        axes[1].plot(xs[1:], [(b - a) / 1e6 for a, b in zip(psi, psi[1:])], label=run["id"])
    for axis, title in zip(axes, ("memory.high events per sample interval", "Cgroup memory PSI some stall time per sample interval")):
        axis.set_title(title)
        axis.grid(alpha=0.2)
        axis.legend()
    axes[1].set_ylabel("seconds")
    axes[-1].set_xlabel(axis_label)
    fig.savefig(out / "pressure.png", dpi=150)
    plt.close(fig)

    if not idle:
        fig, axis = plt.subplots(figsize=(10, 4), constrained_layout=True)
        for run in runs:
            for events, event, style in ((run["sender"], "attempted", "-"), (run["sender"], "sender_final", "--"), (run["receiver"], "received", ":")):
                xs = sorted((x["monotonic_ns"] - run["start"]) / 60e9 for x in events if x["event"] == event)
                axis.step(xs, range(1, len(xs) + 1), where="post", linestyle=style, label=f"{run['id']} {event}")
        axis.set_xlabel(axis_label)
        axis.set_ylabel("Cumulative messages")
        axis.grid(alpha=0.2)
        axis.legend(fontsize=7)
        fig.savefig(out / "deliveries.png", dpi=150)
        plt.close(fig)

    fig, axes = plt.subplots(2, 1, figsize=(10, 6), sharex=True, constrained_layout=True)
    for run in runs:
        counts = [x for x in run["samples"] if "ledger_counts" in x]
        stores = [x for x in run["samples"] if "storage" in x]
        axes[0].plot([(x["monotonic_ns"] - run["start"]) / 60e9 for x in counts],
                     [x["ledger_counts"]["accepted_minus_received_signed" if normal else "accepted_minus_received"] for x in counts], label=run["id"])
        axes[1].plot([(x["monotonic_ns"] - run["start"]) / 60e9 for x in stores],
                     [x["storage"]["subject_file_bytes"] / 1024 for x in stores], label=run["id"])
    backlog_title = ("Accepted minus received (zero expected during idle)" if idle else
                     "Accepted minus received (signed generator-side approximation)" if normal else
                     "Accepted minus received (offline backlog approximation)")
    for axis, title in zip(axes, (backlog_title, "Native subject files (KiB)")):
        axis.set_title(title)
        axis.grid(alpha=0.2)
        axis.legend()
    axes[-1].set_xlabel(axis_label)
    fig.savefig(out / "state.png", dpi=150)
    plt.close(fig)

    valid = all(run["summary"]["capture_valid"] and run["summary"]["functional_pass"] for run in runs)
    lines = [f"# Exploratory Python and Rust daemon {label}", "",
             f"This is one synthetic empty-state paired {label}, not the full comparison plan or a production-equivalent workload.",
             f"Both run validators passed: **{valid}**.", "",
             "| Run | Attempted / received | Cgroup median / observed peak MiB | Anonymous median MiB | File median MiB | Full-run high / OOM events |",
             "| --- | ---: | ---: | ---: | ---: | ---: |"]
    if master["profile"].startswith("preflight-only-"):
        lines.insert(2, "**Preflight only:** this short run verifies harness behavior and is not a measured comparison.")
    for run in runs:
        s = run["summary"]
        lines.append(f"| {run['id']} | {s['attempted']} / {s['received']} | {run['median_current_mib']:.2f} / {run['peak_current_mib']:.2f} | {run['median_anon_mib']:.2f} | {run['median_file_mib']:.2f} | {run['high_events']} / {run['oom_events']} |")
    window = "idle observation" if idle else "offered-load"
    lines.extend(["", f"The median and observed peak are from five-second samples during the {window} window. A scope-wide high-water mark since daemon readiness is in the CSV. File cache and kernel memory are included in cgroup current, but previously cached executable and shared-library pages may be charged outside each scope; the cgroup totals therefore do not establish the complete operational memory budget.", ""])
    if idle:
        lines.extend([f"Each subject was given a 2-CPU quota, 768 MiB memory.high, 1 GiB memory.max and no swap. The idle window lasted {master['duration_seconds']} seconds, with no test peers or offered messages; loopback counters were checked for unexpected traffic.", ""])
    elif normal:
        lines.extend([f"Each subject had a 2-CPU quota, 768 MiB memory.high, 1 GiB memory.max and no swap; sender and receiver ran concurrently in separate scopes. Workload: {master['message_count']} messages of {master['payload_bytes']} bytes at {master['interval_seconds']}-second intervals; the online receiver polled every {master['receiver_poll_interval_seconds']} seconds. The synthetic per-message targets were {master['acceptance_latency_target_seconds']} seconds for sender-observed protocol confirmation and {master['normal_delivery_target_seconds']} seconds for delivery.", "", f"CPU was shared rather than reserved; conservative guest CPU PSI some bounds for the offered window and drain had to remain at or below {master['max_guest_cpu_psi_some_fraction']:.1%}, and peer scopes could not be throttled. Each bound divides pressure accumulated between samples bracketing the interval by its duration. Guest-wide pressure is a coarse contention screen, not proof that an individual peer was unsaturated. This measures polled propagated delivery. A protocol confirmation does not independently prove durable storage; confirmation and delivery percentiles include only completed messages, so read them alongside loss counts.", ""])
    else:
        lines.extend([f"Each subject was given a 2-CPU quota, 768 MiB memory.high, 1 GiB memory.max and no swap; the Python traffic generator was in a separate scope. Workload: {master['message_count']} messages of {master['payload_bytes']} bytes, spaced {master['interval_seconds']} seconds apart, with the receiver offline until the send window ended. The synthetic targets were acceptance within {master['acceptance_latency_target_seconds']} seconds per message and retrieval within {master['retrieval_latency_target_seconds']} seconds after the recipient discovered the node.", "", "Acceptance latency is sender-observed propagation completion; offline wait includes the intentional offline interval. Percentiles include only completed messages, so read them alongside the loss counts.", ""])
    for run in runs:
        if idle:
            lines.append(f"- {run['id']}: anonymous/file slopes = {run['anon_slope_mib_per_hour']:.2f}/{run['file_slope_mib_per_hour']:.2f} MiB/hour.")
        elif normal:
            lines.append(f"- {run['id']}: sender-observed confirmation p50/p95/p99 = {run['acceptance_latency_seconds']}; attempt-to-receipt delivery p50/p95/p99 = {run['delivery_latency_seconds']}; anonymous/file slopes = {run['anon_slope_mib_per_hour']:.2f}/{run['file_slope_mib_per_hour']:.2f} MiB/hour. All latency values are seconds.")
            lines.append(f"  During the {master['duration_seconds']}-second offered window: {run['window_confirmed']} confirmations ({run['window_confirmed'] / master['duration_seconds']:.2f}/s) and {run['window_received']} deliveries ({run['window_received'] / master['duration_seconds']:.2f}/s). By the end of the {run['drain_seconds']:.1f}-second drain: {run['summary']['sender_final']} final sender outcomes and {run['summary']['received']} deliveries. Sender outcomes include failures; local packet submissions are not measured daemon admissions.")
            drain = run['summary']['guest_cpu_psi_drain_fraction']
            drain_text = f"{drain:.2%}" if drain is not None else "not applicable"
            lines.append(f"  Conservative guest CPU PSI some bounds: offered window = {run['summary']['guest_cpu_psi_some_fraction']:.2%}, drain = {drain_text}.")
        else:
            lines.append(f"- {run['id']}: acceptance p50/p95/p99 = {run['acceptance_latency_seconds']}; retrieval after node discovery p50/p95/p99 = {run['retrieval_latency_seconds']}; offline wait p50/p95/p99 = {run['offline_wait_seconds']}; anonymous/file slopes = {run['anon_slope_mib_per_hour']:.2f}/{run['file_slope_mib_per_hour']:.2f} MiB/hour. All latency values are seconds.")
        if master.get("recovery_seconds"):
            lines.append(f"  Recovery begins after retrieval and the receiver quiet interval. Over the {master['recovery_seconds']}-second recovery, anonymous/file memory changed by {run['recovery_anon_change_mib']:.2f}/{run['recovery_file_change_mib']:.2f} MiB from the first to the last recovery sample. This excludes the preceding drain and does not establish native queue or retained-state stability.")
        lines.append(f"  Median process PSS/swap = {run['median_pss_mib']}/{run['median_process_swap_mib']} MiB; subject CPU time was {run['cpu_seconds']:.3f} seconds over {run['cpu_sampled_seconds']:.3f} sampled seconds within the {window} window. PSS must not be added to cgroup memory.")
        lines.append(f"  Full-run subject memory PSI some = {run['memory_psi_some_seconds']:.3f} seconds; host memory/I/O PSI some = {run['host_memory_psi_some_seconds']:.3f}/{run['host_io_psi_some_seconds']:.3f} seconds. Approximate sampled time above memory.high during the {window} window = {run['approx_seconds_above_high']} seconds.")
        lines.append(f"  Subject process read/write bytes during the sampled {window} window = {run['process_read_bytes']}/{run['process_write_bytes']}; maximum accepted-minus-received = {run['max_accepted_minus_received']}; maximum native subject-file size = {run['max_native_store_bytes']} bytes.")
        wal = [x["storage"]["reticulum.db-wal_bytes"] for x in run["samples"] if "reticulum.db-wal_bytes" in x.get("storage", {})]
        if wal:
            lines.append(f"  Sampled SQLite WAL size: first {wal[0]} bytes, maximum {max(wal)} bytes, final {wal[-1]} bytes. This size observation alone does not establish the cause of memory growth.")
        if run["summary"]["capture_errors"] or run["summary"]["functional_errors"]:
            lines.append(f"  Validation findings: {run['summary']['capture_errors'] + run['summary']['functional_errors']}")
    io_note = "Cgroup io.stat is also retained in raw samples." if any("io.stat" in x["cgroup"] for run in runs for x in run["samples"]) else "Cgroup io.stat was unavailable in these runs."
    backlog_note = ("No peers or messages were expected during idle." if idle else
                    "Accepted-minus-received is a signed generator-side approximation, not a native queue measurement; it can be negative when receipt precedes observation of acceptance." if normal else
                    "Accepted-minus-received is a generator-side approximation rather than a direct daemon queue measurement; its growth is expected while the receiver is offline.")
    cpu_note = "The collector records CPU, memory and I/O pressure; for guest runs the host watchdog separately records host pressure."
    lines.extend(["", f"The process I/O values are deltas from 60-second /proc samples, not device-level I/O. {io_note} {backlog_note} Native file sizes are physical, implementation-specific measurements rather than equal logical retained state. {cpu_note}", "", "These slopes are descriptive only; they do not establish a leak or a sustained trend. See the raw manifests, samples and ledgers for timing, process, storage and pressure details.", "", "Other shared-daemon scenarios and repetitions are outside this single-pair report. See ../config-equivalence.md for configuration differences and ../execution-board.md for remaining work."])
    (out / "report.md").write_text("\n".join(lines) + "\n")
    print(out / "report.md")
    return 0


def main():
    try:
        return run_report()
    except Exception as error:
        if len(sys.argv) < 2:
            raise
        out = Path(sys.argv[1]).resolve() / "report"
        out.mkdir(exist_ok=True)
        return failed_report(out, [f"Report validation failed: {type(error).__name__}: {error}"])


if __name__ == "__main__":
    raise SystemExit(main())
