#!/usr/bin/env python3
"""Reconcile a smoke-run ledger and validate its sampling coverage."""
import hashlib
import base64
import json
import math
import sys
from collections import Counter
from pathlib import Path


def jsonl(path, errors=None):
    if not path.exists():
        if errors is not None:
            errors.append(f"missing input: {path.name}")
        return []
    rows = []
    with path.open("rb") as source:
        for number, line in enumerate(source, 1):
            if not line.strip():
                continue
            try:
                row = json.loads(line)
                if not isinstance(row, dict):
                    raise ValueError("record is not an object")
                rows.append(row)
            except (ValueError, UnicodeDecodeError) as error:
                if errors is None:
                    raise
                errors.append(f"{path.name}:{number}: malformed record ({type(error).__name__})")
    return rows


def expected_payload_hash(sequence, spec):
    prefix = f"{spec['seed']}:{sequence:06d}:".encode()
    padding = hashlib.sha256(prefix).hexdigest().encode()
    content = (prefix + padding * (spec["payload_bytes"] // len(padding) + 1))[:spec["payload_bytes"]]
    return hashlib.sha256(content).hexdigest()


def peer_started(manifest, role):
    if role + "_started" in manifest:
        return manifest[role + "_started"]
    return manifest.get(role + "_exit") is not None or role in manifest.get("phase_times_ns", {})


def loopback_bytes(sample):
    for line in sample["namespace_network"].splitlines():
        if ":" in line and line.split(":", 1)[0].strip() == "lo":
            fields = line.split(":", 1)[1].split()
            return int(fields[0]), int(fields[8])
    raise ValueError("loopback counters are unavailable")


def cpu_pressure_total(sample):
    for line in sample["host_cpu_pressure"].splitlines():
        fields = line.split()
        if fields and fields[0] == "some":
            return int(next(field[6:] for field in fields[1:] if field.startswith("total=")))
    raise ValueError("CPU pressure total is unavailable")


def cpu_pressure_bound(samples, start, end, period):
    before = [row for row in samples if row.get("host_cpu_pressure_completed_ns", start + 1) <= start]
    after = [row for row in samples if row.get("host_cpu_pressure_started_ns", end - 1) >= end]
    first, last = before[-1], after[0]
    pressure = cpu_pressure_total(last) - cpu_pressure_total(first)
    if (end <= start or pressure < 0 or first["host_cpu_pressure_started_ns"] > first["host_cpu_pressure_completed_ns"]
            or last["host_cpu_pressure_started_ns"] > last["host_cpu_pressure_completed_ns"]
            or start - first["host_cpu_pressure_completed_ns"] > 2 * period
            or last["host_cpu_pressure_started_ns"] - end > 2 * period):
        raise ValueError("CPU pressure window is incomplete")
    return pressure * 1000 / (end - start)


def inbound_queue_errors(role, rows, completed):
    snapshots = [row for row in rows if row["event"] == "inbound_queue"]
    errors = []
    stages = [row.get("stage") for row in snapshots]
    if completed and (not stages or stages[0] != "baseline" or stages[-1] != "final"):
        errors.append(f"{role} inbound queue snapshots are incomplete")
    if snapshots and (stages[0] != "baseline" or stages.count("baseline") != 1
                      or stages.count("final") > 1 or ("final" in stages and stages[-1] != "final")
                      or any(stage not in ("baseline", "periodic", "final") for stage in stages)):
        errors.append(f"{role} inbound queue snapshot order is invalid")
    previous = None
    for row in snapshots:
        heights, dropped, total = row.get("heights"), row.get("dropped"), row.get("total")
        if (not isinstance(heights, list) or not isinstance(dropped, list)
                or len(heights) != 4 or len(dropped) != 4 or type(total) is not int
                or any(type(value) is not int or value < 0 for value in heights + dropped)
                or total != sum(heights)):
            errors.append(f"{role} inbound queue snapshot is malformed")
            break
        if previous is not None and any(current < earlier for current, earlier in zip(dropped, previous)):
            errors.append(f"{role} inbound queue drop counters decreased")
            break
        if any(dropped):
            errors.append(f"{role} inbound queue dropped packets")
            break
        previous = dropped
    return errors


def proof_timeout_errors(finished, sends, target):
    for row in finished:
        sent = sends.get(row["sequence"])
        if sent and sent.get("submitted") and row["state"] not in (4, 8):
            elapsed = row.get("proof_elapsed_seconds")
            if (row.get("receipt_status") != 0 or type(elapsed) not in (int, float)
                    or not math.isfinite(elapsed) or elapsed < target):
                return ["submitted packet proof timed out early or its receipt was culled"]
    return []


def _validate(spec, manifest, samples, sender, receiver, input_errors=None):
    capture_errors, functional_errors = [], []
    input_errors = input_errors or {}
    for errors in input_errors.values():
        capture_errors.extend(errors)
    count = spec["message_count"]
    attempted = [row for row in sender if row["event"] == "attempted"]
    finished = [row for row in sender if row["event"] == "sender_final"]
    delivered = [row for row in receiver if row["event"] == "received"]
    complete = [row for row in receiver if row["event"] == "receiver_complete"]
    for name, rows in (("sample", samples), ("sender", sender), ("receiver", receiver)):
        for row in rows:
            if type(row["monotonic_ns"]) is not int or row["monotonic_ns"] < 0:
                raise ValueError(f"{name} has an invalid monotonic timestamp")
        if any(b["monotonic_ns"] < a["monotonic_ns"] for a, b in zip(rows, rows[1:])):
            capture_errors.append(f"{name} timestamps are out of order")
    for row in attempted:
        if type(row["scheduled_monotonic_ns"]) is not int or row["scheduled_monotonic_ns"] < 0:
            raise ValueError("attempt has an invalid scheduled timestamp")
    for row in attempted + finished:
        if type(row["sequence"]) is not int or row["sequence"] < 0:
            raise ValueError("sender has an invalid sequence number")
    for row in complete:
        if type(row["count"]) is not int or row["count"] < 0 or type(row["quiet_interval_completed"]) is not bool or type(row["sync_state"]) is not int:
            raise ValueError("receiver completion has invalid fields")
    sender_capture_failure = manifest.get("sender_exit") == 70 or bool(input_errors.get("sender"))
    receiver_capture_failure = manifest.get("receiver_exit") == 70 or bool(input_errors.get("receiver"))
    sender_started = peer_started(manifest, "sender")
    receiver_started = peer_started(manifest, "receiver")
    subject_failed = manifest.get("subject_unexpected_exit", False)
    sender_interrupted = subject_failed and (manifest.get("sender_exit") is None or manifest.get("sender_cancelled") is True)
    receiver_interrupted = subject_failed and (manifest.get("receiver_exit") is None or manifest.get("receiver_cancelled") is True)
    idle = spec.get("profile") in ("idle", "preflight-only-idle-dry-run")
    normal = spec.get("profile") in ("normal", "high-load", "preflight-only-normal-dry-run")
    normal_readiness_failed = normal and manifest.get("normal_readiness_failed") is True
    sender_completions = [row for row in sender if row["event"] == "sender_complete"]
    if spec.get("generator_mode") == "prepared_packets":
        capture_errors.extend(inbound_queue_errors("sender", sender, bool(sender_completions)))
        capture_errors.extend(inbound_queue_errors("receiver", receiver, bool(complete)))
    if any(type(row["attempted"]) is not int or type(row["final"]) is not int or row["attempted"] < 0 or row["final"] < 0 for row in sender_completions):
        raise ValueError("sender completion has invalid counters")
    if sender_completions and (len(sender_completions) != 1 or sender_completions[-1]["attempted"] != len(attempted)
                               or sender_completions[-1]["final"] != len(finished)):
        capture_errors.append("sender completion counters disagree with the ledger")
        sender_capture_failure = True
    if len(complete) > 1:
        capture_errors.append("receiver recorded multiple completion events")
        receiver_capture_failure = True
    for index, row in enumerate(receiver):
        if row["event"] == "receiver_complete" and row["count"] != sum(item["event"] == "received" for item in receiver[:index]):
            capture_errors.append("receiver completion count disagrees with the preceding ledger")
            receiver_capture_failure = True
    delivery_evaluated = sender_started and receiver_started and not (sender_capture_failure or receiver_capture_failure or sender_interrupted or receiver_interrupted or normal_readiness_failed)
    if normal_readiness_failed:
        functional_errors.append("normal traffic peer failed readiness before the offered schedule")
        failed_roles = manifest.get("normal_readiness_failed_roles")
        if (not isinstance(failed_roles, list) or not failed_roles or any(role not in ("sender", "receiver") for role in failed_roles)
                or "normal" in manifest.get("phase_times_ns", {}) or attempted
                or any(row["event"] == "poll_attempt" for row in receiver)):
            capture_errors.append("normal readiness failure evidence was inconsistent")
        else:
            for role in failed_roles:
                rows = sender if role == "sender" else receiver
                starts = [row for row in rows if row["event"] == "node_discovery_start"]
                timeouts = [row for row in rows if row["event"] == "node_timeout"]
                if (manifest.get(role + "_exit") != 71 or len(starts) != 1 or len(timeouts) != 1
                        or starts[0].get("node") != timeouts[0].get("node")
                        or timeouts[0]["monotonic_ns"] - starts[0]["monotonic_ns"] < (spec["route_deadline_seconds"] - 0.5) * 1e9
                        or any(row["event"] == "node_ready" for row in rows)):
                    capture_errors.append(f"{role} readiness timeout evidence was inconsistent")
    if sender_capture_failure:
        capture_errors.append("sender reported a ledger capture failure")
    if receiver_capture_failure:
        capture_errors.append("receiver reported a ledger capture failure")

    if not manifest.get("isolation_verified") or not manifest.get("limits_verified"):
        capture_errors.append("isolation or resource limits were not verified")
    if not manifest.get("finalized"):
        capture_errors.append("runner did not finalize the run")
    if manifest.get("collector_errors"):
        capture_errors.extend(manifest["collector_errors"])
    if not samples:
        capture_errors.append("no samples")
        coverage = 0.0
    else:
        times = [sample["monotonic_ns"] for sample in samples]
        period = spec["sample_interval_seconds"] * 1_000_000_000
        start = manifest.get("sampling_start_ns", times[0])
        end = manifest.get("sampling_end_ns", times[-1])
        expected = max(1, math.floor((end - start) / period) + 1)
        occupied = {min(expected - 1, (timestamp - start) // period) for timestamp in times if start <= timestamp <= end}
        coverage = len(occupied) / expected
        gaps = [b - a for a, b in zip(times, times[1:])]
        if coverage < 0.99:
            capture_errors.append(f"sample coverage {coverage:.3%} below 99%")
        if times[0] - start > period * 2 or end - times[-1] > period * 2 or any(gap > 30_000_000_000 for gap in gaps):
            capture_errors.append("unexplained sample gap or missing boundary sample")
        if any(sample.get("error") for sample in samples):
            capture_errors.append("sample contains collector error")
        if any(sample["cgroup"]["memory.events"].get(key, 0) for sample in samples for key in ("oom", "oom_kill")):
            functional_errors.append("subject cgroup recorded an OOM event")
        if any(sample.get("generator", {}).get("memory_events", {}).get(key, 0) for sample in samples for key in ("oom", "oom_kill")):
            capture_errors.append("generator cgroup recorded an OOM event")

    if manifest.get("generator_telemetry_contract") == 1:
        for role in ("sender", "receiver"):
            if not peer_started(manifest, role) or manifest.get(role + "_exit") == 70:
                continue
            start = manifest.get(role + "_scope_verified_ns")
            end = manifest.get(role + "_exit_observed_ns")
            if end is None and subject_failed:
                end = manifest.get("sampling_end_ns")
            if start is None or end is None or end < start:
                capture_errors.append(f"{role} scope timing was unavailable")
                continue
            periodic = [row for row in samples if row.get("sample_kind") == "periodic" and row.get("phase") == role
                        and start <= row["monotonic_ns"] <= end]
            if any("generator_active_at_start" not in row or "generator_active_at_end" not in row for row in periodic):
                capture_errors.append(f"{role} generator liveness evidence was unavailable")
            if any(type(row.get("generator_active_at_start")) is not bool or type(row.get("generator_active_at_end")) is not bool for row in periodic):
                raise ValueError(f"{role} generator liveness evidence is not boolean")
            eligible = [row for row in periodic if row.get("generator_active_at_start") and row.get("generator_active_at_end")]
            generator_samples = [row for row in eligible if all(key in row.get("generator", {}) for key in ("memory_current", "memory_events", "cpu_stat"))
                                 and all(key in row["generator"]["memory_events"] for key in ("oom", "oom_kill"))
                                 and all(key in row["generator"]["cpu_stat"] for key in ("usage_usec", "nr_throttled"))]
            if (end - start > 2 * spec["sample_interval_seconds"] * 1e9 and not eligible) or (eligible and len(generator_samples) / len(eligible) < 0.99):
                capture_errors.append(f"{role} generator scope sampling was incomplete")
            times = [row["monotonic_ns"] for row in generator_samples]
            if end - start > 30_000_000_000 and (not times or times[0] - start > 30_000_000_000
                    or end - times[-1] > 30_000_000_000
                    or any(b - a > 30_000_000_000 for a, b in zip(times, times[1:]))):
                capture_errors.append(f"{role} generator scope sampling had a gap over 30 seconds")
            if any(row["generator"]["cpu_stat"].get("nr_throttled", 0) for row in generator_samples):
                capture_errors.append(f"{role} generator cgroup experienced CPU throttling")

    if manifest.get("generator_telemetry_contract") == 2:
        roles = [role for role in ("sender", "receiver") if peer_started(manifest, role)]
        groups = [manifest.get(role + "_cgroup") for role in roles]
        if (any(not isinstance(group, str) or not group for group in groups)
                or len(set(groups + [manifest.get("cgroup")])) != len(groups) + 1
                or ("normal" in manifest.get("phase_times_ns", {}) and set(roles) != {"sender", "receiver"})):
            capture_errors.append("normal traffic subject and peer scopes were not distinct")
        for role in ("sender", "receiver"):
            exit_code = manifest.get(role + "_exit")
            expected_timeout = normal_readiness_failed and role in manifest.get("normal_readiness_failed_roles", []) and exit_code == 71
            expected_cancel = manifest.get(role + "_cancelled") is True and (subject_failed or normal_readiness_failed)
            if exit_code is not None and exit_code != 0 and not (expected_timeout or expected_cancel):
                capture_errors.append(f"{role} peer exited unexpectedly")
            if not peer_started(manifest, role) or manifest.get(role + "_exit") == 70:
                continue
            start = manifest.get(role + "_scope_verified_ns")
            end = manifest.get(role + "_exit_observed_ns")
            if end is None and subject_failed:
                end = manifest.get("sampling_end_ns")
            if start is None or end is None or end < start:
                capture_errors.append(f"{role} scope timing was unavailable")
                continue
            periodic = [row for row in samples if row.get("sample_kind") == "periodic" and start <= row["monotonic_ns"] <= end]
            recorded = []
            for row in periodic:
                evidence = row.get("generators", {}).get(role)
                if not isinstance(evidence, dict) or type(evidence.get("active_at_start")) is not bool or type(evidence.get("active_at_end")) is not bool:
                    capture_errors.append(f"{role} generator liveness evidence was unavailable")
                    continue
                if not evidence["active_at_start"] or not evidence["active_at_end"]:
                    continue
                scope = evidence.get("scope")
                if (not isinstance(scope, dict) or scope.get("cgroup") != manifest.get(role + "_cgroup")
                        or not all(key in scope for key in ("memory_current", "memory_events", "cpu_stat", "processes"))
                        or not all(key in scope["memory_events"] for key in ("oom", "oom_kill"))
                        or not all(key in scope["cpu_stat"] for key in ("usage_usec", "nr_throttled"))
                        or not any(process.get("pid") == manifest.get(role + "_pid")
                                   and process.get("start_ticks") == manifest.get(role + "_start_ticks") for process in scope["processes"])):
                    capture_errors.append(f"{role} generator scope sampling was incomplete")
                    continue
                if any(scope["memory_events"][key] for key in ("oom", "oom_kill")):
                    capture_errors.append(f"{role} generator cgroup recorded an OOM event")
                if scope["cpu_stat"]["nr_throttled"]:
                    capture_errors.append(f"{role} generator cgroup experienced CPU throttling")
                recorded.append(row["monotonic_ns"])
            if end - start > 30_000_000_000 and (not recorded or recorded[0] - start > 30_000_000_000
                    or end - recorded[-1] > 30_000_000_000 or any(b - a > 30_000_000_000 for a, b in zip(recorded, recorded[1:]))):
                capture_errors.append(f"{role} generator scope sampling had a gap over 30 seconds")
            final_events = manifest.get(role + "_final_memory_events")
            final_cpu = manifest.get(role + "_final_cpu_stat")
            if (not isinstance(final_events, dict) or not all(key in final_events for key in ("oom", "oom_kill"))
                    or not isinstance(final_cpu, dict) or not all(key in final_cpu for key in ("usage_usec", "nr_throttled"))):
                capture_errors.append(f"{role} lacked final generator scope counters")
            elif any(final_events[key] for key in ("oom", "oom_kill")):
                capture_errors.append(f"{role} generator cgroup recorded an OOM event")
            if isinstance(final_cpu, dict) and final_cpu.get("nr_throttled", 0):
                capture_errors.append(f"{role} generator cgroup experienced CPU throttling")

    if sender_started and not (sender_capture_failure or sender_interrupted or normal_readiness_failed) and (len(attempted) != count or {row["sequence"] for row in attempted} != set(range(count))):
        functional_errors.append("offered message schedule was incomplete")
    if attempted:
        start = attempted[0]["scheduled_monotonic_ns"]
        if any(abs(row["scheduled_monotonic_ns"] - start - row["sequence"] * spec["interval_seconds"] * 1e9) > 1_000_000 for row in attempted):
            capture_errors.append("offered schedule spacing differs from the frozen profile")
        if any(row["sha256"] != expected_payload_hash(row["sequence"], spec) for row in attempted):
            functional_errors.append("offered payload differs from the frozen fixture")
        if (spec.get("profile") in ("offline", "preflight-only-offline-recovery-dry-run")
                and not sender_interrupted and not sender_capture_failure):
            completions = [row for row in sender if row["event"] == "sender_complete"]
            sender_end = manifest.get("phase_times_ns", {}).get("sender_end")
            if (not completions or completions[-1].get("monotonic_ns", 0) < start + spec["duration_seconds"] * 1e9
                    or sender_end is None or sender_end < completions[-1].get("monotonic_ns", 0)
                    or not any(row.get("phase") == "sender" and row["monotonic_ns"] == sender_end for row in samples)):
                capture_errors.append("offered-load observation window was incomplete")
    guest_cpu_psi_some_fraction = None
    guest_cpu_psi_drain_fraction = None
    if normal and not (subject_failed or normal_readiness_failed):
        start = manifest.get("phase_times_ns", {}).get("normal")
        end = manifest.get("phase_times_ns", {}).get("normal_end")
        sender_ready = [row for row in sender if row["event"] == "node_ready"]
        receiver_ready = [row for row in receiver if row["event"] == "node_ready"]
        if type(start) is not int or type(end) is not int or len(sender_ready) != 1 or len(receiver_ready) != 1:
            capture_errors.append("normal traffic readiness or phase boundaries were unavailable")
        else:
            if (sender_ready[0]["monotonic_ns"] >= start or receiver_ready[0]["monotonic_ns"] >= start
                    or any(abs(row["scheduled_monotonic_ns"] - start - row["sequence"] * spec["interval_seconds"] * 1e9) > 1_000_000 for row in attempted)
                    or (manifest.get("sender_exit") == 0 and (not sender_completions or sender_completions[-1]["monotonic_ns"] < start + spec["duration_seconds"] * 1e9))
                    or (manifest.get("receiver_exit") == 0 and (not complete or complete[-1]["monotonic_ns"] < start + spec["duration_seconds"] * 1e9))
                    or (complete and end < complete[-1]["monotonic_ns"])):
                capture_errors.append("normal traffic did not cover the frozen shared schedule")
            window_end = start + spec["duration_seconds"] * 1_000_000_000
            try:
                period = spec["sample_interval_seconds"] * 1_000_000_000
                guest_cpu_psi_some_fraction = cpu_pressure_bound(samples, start, window_end, period)
                if end > window_end:
                    guest_cpu_psi_drain_fraction = cpu_pressure_bound(samples, window_end, end, period)
                if any(value is not None and value > spec["max_guest_cpu_psi_some_fraction"]
                       for value in (guest_cpu_psi_some_fraction, guest_cpu_psi_drain_fraction)):
                    capture_errors.append("guest CPU pressure exceeded the frozen limit")
            except (IndexError, KeyError, StopIteration, ValueError):
                capture_errors.append("guest CPU pressure evidence was unavailable or incomplete")
            polls = [row for row in receiver if row["event"] == "poll_attempt"]
            finished_polls = [row for row in receiver if row["event"] == "poll_complete"]
            failed_polls = [row for row in receiver if row["event"] == "poll_failed"]
            missed_polls = [row for row in receiver if row["event"] == "poll_missed"]
            if any(type(row["slot"]) is not int or type(row["scheduled_monotonic_ns"]) is not int for row in polls + missed_polls):
                raise ValueError("normal poll schedule has invalid fields")
            if any(type(row["slot"]) is not int for row in finished_polls + failed_polls):
                raise ValueError("normal poll completion has invalid fields")
            slots = [row["slot"] for row in polls]
            expected = spec["duration_seconds"] // spec["receiver_poll_interval_seconds"] + 1
            scheduled = sorted(polls + missed_polls, key=lambda row: row["slot"])
            if ([row["slot"] for row in scheduled] != list(range(len(scheduled)))
                    or (manifest.get("receiver_exit") == 0 and len(scheduled) < expected)):
                capture_errors.append("normal receiver did not account for every fixed poll slot")
            if len(slots) < expected or (spec.get("receiver_poll_timeout_seconds") is None and [row["slot"] for row in finished_polls] != slots):
                functional_errors.append("normal propagation poll schedule was incomplete")
            poll_by_slot = {row["slot"]: row for row in polls}
            if spec.get("receiver_poll_timeout_seconds") is not None:
                outcomes = sorted(finished_polls + failed_polls, key=lambda row: row["monotonic_ns"])
                if [row["slot"] for row in outcomes] != slots:
                    capture_errors.append("normal poll outcomes were missing, duplicated, or out of order")
                if any(type(row.get("poll_started_monotonic_ns")) is not int
                       or not 0 <= row["monotonic_ns"] - row["poll_started_monotonic_ns"] <= 1_000_000_000 for row in polls):
                    capture_errors.append("normal poll start timestamps were invalid")
                if failed_polls:
                    functional_errors.append("normal propagation poll failed or timed out")
                if any(row.get("reason") not in ("terminal", "link_closed", "request_failed", "submission", "route", "timeout", "observation_late", "drain_deadline")
                       or type(row.get("sync_state")) is not int
                       or (row.get("link_status") is not None and type(row.get("link_status")) is not int)
                       or type(row.get("pending_requests")) is not int or row["pending_requests"] < 0
                       or not isinstance(row.get("request_statuses"), list)
                       or any(type(status) is not int or status not in (0, 1, 2, 3, 4) for status in row["request_statuses"])
                       or type(row.get("submission_failures")) is not int or row["submission_failures"] < 0
                       or row["slot"] not in poll_by_slot
                       or row["monotonic_ns"] < poll_by_slot[row["slot"]]["monotonic_ns"]
                       or (row.get("reason") == "terminal" and row["sync_state"] < 0xf0)
                       or (row.get("reason") == "link_closed" and row["link_status"] != 4)
                       or (row.get("reason") == "request_failed" and 0 not in row["request_statuses"])
                       or (row.get("reason") == "submission" and row["submission_failures"] == 0)
                       or (row.get("reason") == "route" and row["link_status"] is not None)
                       or (row.get("reason") == "timeout" and row["monotonic_ns"] - poll_by_slot[row["slot"]].get("poll_started_monotonic_ns", row["monotonic_ns"]) < spec["receiver_poll_timeout_seconds"] * 1e9)
                       or (row.get("reason") == "drain_deadline" and row["monotonic_ns"] < start + (spec["duration_seconds"] + spec["receiver_drain_seconds"]) * 1e9)
                       for row in failed_polls):
                    capture_errors.append("normal failed poll evidence was malformed")
                if any(row.get("reason") == "observation_late" for row in failed_polls):
                    capture_errors.append("poll completion was first observed after its deadline")
            if any(row.get("sync_state") != 7 or row.get("link_status") != 2 or row.get("pending_requests") != 0
                   or (spec.get("receiver_poll_timeout_seconds") is not None and
                       (not isinstance(row.get("request_statuses"), list) or not row["request_statuses"] or any(status != 4 for status in row["request_statuses"])))
                   or row["slot"] not in poll_by_slot or row["monotonic_ns"] < poll_by_slot[row["slot"]]["monotonic_ns"]
                   or (spec.get("receiver_poll_timeout_seconds") is not None and row["monotonic_ns"] - poll_by_slot[row["slot"]].get("poll_started_monotonic_ns", 0) > spec["receiver_poll_timeout_seconds"] * 1e9)
                   for row in finished_polls):
                capture_errors.append("normal poll completion lacked an active, drained sync")
            outcomes_by_slot = {row["slot"]: row for row in finished_polls + failed_polls}
            if any(a["slot"] in outcomes_by_slot and b["monotonic_ns"] < outcomes_by_slot[a["slot"]]["monotonic_ns"]
                   for a, b in zip(polls, polls[1:])):
                functional_errors.append("normal propagation polls overlapped")
            if any(abs(row["scheduled_monotonic_ns"] - start - row["slot"] * spec["receiver_poll_interval_seconds"] * 1e9) > 1_000_000
                   or row["monotonic_ns"] < row["scheduled_monotonic_ns"]
                   or row["monotonic_ns"] - row["scheduled_monotonic_ns"] > spec.get("maximum_poll_lateness_seconds", spec["maximum_schedule_lateness_seconds"]) * 1e9 for row in scheduled):
                capture_errors.append("normal receiver missed its fixed poll schedule")
            if missed_polls:
                if any(type(row.get("previous_pending")) is not bool or type(row.get("schedule_late")) is not bool for row in missed_polls):
                    raise ValueError("normal missed poll lacks its reason")
                if any(row.get("previous_pending") is True for row in missed_polls):
                    functional_errors.append("normal propagation polls overlapped")
                if any(row.get("schedule_late") is True or row.get("previous_pending") is False for row in missed_polls):
                    capture_errors.append("normal receiver missed its fixed poll schedule")
            if spec.get("generator_mode") == "prepared_packets":
                sends = [row for row in sender if row["event"] == "protocol_send"]
                if len(sends) != count or {row.get("sequence") for row in sends} != set(range(count)):
                    capture_errors.append("prepared protocol send evidence was incomplete")
                sent_by_sequence = {row["sequence"]: row for row in sends}
                for row in attempted:
                    sent = sent_by_sequence.get(row["sequence"])
                    if (sent is None or type(sent.get("submitted")) is not bool
                            or sent["monotonic_ns"] < row["monotonic_ns"]
                            or sent["monotonic_ns"] - row["scheduled_monotonic_ns"] > spec["maximum_schedule_lateness_seconds"] * 1e9):
                        capture_errors.append("prepared packet missed its protocol-send schedule")
                        break
                if any(row.get("submitted") is False for row in sends):
                    functional_errors.append("a prepared propagation packet was not submitted")
                if spec.get("receiver_poll_timeout_seconds") is not None:
                    capture_errors.extend(proof_timeout_errors(finished, sent_by_sequence, spec["acceptance_latency_target_seconds"]))
                done = [row for row in receiver if row["event"] == "sender_done_observed"]
                if complete and complete[-1]["quiet_interval_completed"]:
                    latest = max([row["monotonic_ns"] for row in delivered] + [0])
                    if (len(done) != 1 or not sender_completions
                            or type(done[0].get("sender_done_ns")) is not int
                            or done[0]["sender_done_ns"] < sender_completions[-1]["monotonic_ns"]
                            or not any(poll["monotonic_ns"] >= max(latest, done[0]["monotonic_ns"])
                                       and poll["slot"] in {item["slot"] for item in finished_polls} for poll in polls)):
                        capture_errors.append("receiver quiet completion lacked a fresh post-sender poll")
    if any(row["monotonic_ns"] < row["scheduled_monotonic_ns"] or row["monotonic_ns"] - row["scheduled_monotonic_ns"] > spec["maximum_schedule_lateness_seconds"] * 1e9 for row in attempted):
        capture_errors.append("generator missed its offered-load schedule")
    if sender_started and not (sender_capture_failure or sender_interrupted or normal_readiness_failed) and (len(finished) != count or {row["sequence"] for row in finished} != set(range(count))):
        functional_errors.append("sender did not reach a final state for every message")
    if sender_started and not (sender_capture_failure or sender_interrupted or normal_readiness_failed) and (manifest.get("sender_exit") != 0 or not sender_completions):
        functional_errors.append("sender process or completion record failed")
    if delivery_evaluated and manifest.get("receiver_exit") != 0:
        functional_errors.append("receiver process failed")
    if any(row["state"] not in (4, 8) for row in finished):
        functional_errors.append("sender reported failed or rejected messages")
    attempts_by_sequence = {row["sequence"]: row for row in attempted}
    if any(row["sequence"] in attempts_by_sequence and row["monotonic_ns"] < attempts_by_sequence[row["sequence"]]["monotonic_ns"] for row in finished):
        capture_errors.append("sender completion preceded its attempt")
    if "acceptance_latency_target_seconds" in spec and any(
        row["sequence"] in attempts_by_sequence and row["monotonic_ns"] - attempts_by_sequence[row["sequence"]]["monotonic_ns"] > spec["acceptance_latency_target_seconds"] * 1e9
        for row in finished if row["state"] in (4, 8)
    ):
        functional_errors.append("sender-observed protocol confirmation exceeded its latency target")
    expected_hashes = Counter((expected_payload_hash(sequence, spec), spec["sender_delivery_hash"], spec["receiver_delivery_hash"])
                              for sequence in range(count))
    received_hashes = Counter((row["sha256"], row["source"], row["destination"]) for row in delivered)
    missing = sum((expected_hashes - received_hashes).values())
    unexpected = sum((received_hashes - expected_hashes).values())
    if delivery_evaluated and (missing or unexpected):
        functional_errors.append(f"receiver reconciliation: {missing} missing, {unexpected} duplicate or unexpected")
    sender_message_hashes = Counter(row["message_hash"] for row in finished if row["state"] in (4, 8))
    receiver_message_hashes = Counter(row["message_hash"] for row in delivered)
    if delivery_evaluated and sender_message_hashes != receiver_message_hashes:
        functional_errors.append("sender/receiver protocol message hashes differ")
    expected_protocol_hash = {attempts_by_sequence[row["sequence"]]["sha256"]: row["message_hash"]
                              for row in finished if row["state"] in (4, 8) and row["sequence"] in attempts_by_sequence}
    attempt_times = {row["sha256"]: row["monotonic_ns"] for row in attempted}
    if any(row["sha256"] in attempt_times and row["monotonic_ns"] < attempt_times[row["sha256"]] for row in delivered):
        capture_errors.append("receiver delivery preceded its sender attempt")
    if delivery_evaluated and any(row["sha256"] in expected_protocol_hash and row["message_hash"] != expected_protocol_hash[row["sha256"]] for row in delivered):
        functional_errors.append("receiver payload and protocol message hash pairing differs from the sender")
    if any(row["source"] != spec["sender_delivery_hash"] or row["destination"] != spec["receiver_delivery_hash"] for row in delivered):
        functional_errors.append("receiver observed an unexpected source or destination")
    if delivery_evaluated and (not complete or not complete[-1]["quiet_interval_completed"] or complete[-1]["sync_state"] != 7):
        functional_errors.append("receiver did not complete propagation sync and the quiet/drain interval")
    if complete and any(row["event"] == "received" for row in receiver[receiver.index(complete[-1]) + 1:]):
        functional_errors.append("receiver recorded a delivery after its quiet completion")
    ready = [row for row in receiver if row["event"] == "node_ready"]
    if ready and any(row["monotonic_ns"] < ready[0]["monotonic_ns"] for row in delivered):
        capture_errors.append("receiver delivery preceded node readiness")
    if delivery_evaluated and not normal and "retrieval_latency_target_seconds" in spec and (not ready or any(
        row["monotonic_ns"] - ready[0]["monotonic_ns"] > spec["retrieval_latency_target_seconds"] * 1e9 for row in delivered
    )):
        functional_errors.append("offline retrieval exceeded its latency target")
    if delivery_evaluated and normal and any(
        row["sha256"] in attempt_times and row["monotonic_ns"] - attempt_times[row["sha256"]] > spec["normal_delivery_target_seconds"] * 1e9
        for row in delivered
    ):
        functional_errors.append("normal delivery exceeded its latency target")
    if manifest.get("subject_unexpected_exit"):
        functional_errors.append("subject exited unexpectedly")
    final_events = manifest.get("subject_final_memory_events")
    if final_events and any(final_events.get(key, 0) for key in ("oom", "oom_kill")) and "subject cgroup recorded an OOM event" not in functional_errors:
        functional_errors.append("subject cgroup recorded an OOM event")
    oom_at_exit = ("recorded" if final_events and any(final_events.get(key, 0) for key in ("oom", "oom_kill"))
                   else "not_recorded" if final_events is not None else "unknown") if subject_failed else "not_applicable"
    if manifest.get("subject_log_error_count", 0):
        functional_errors.append("subject log contains error or panic indicators; inspect subject_log_error_examples")
    idle_evaluated = False
    if idle:
        if sender_started or receiver_started or sender or receiver:
            functional_errors.append("idle profile unexpectedly started peers or recorded peer events")
        idle_start = manifest.get("phase_times_ns", {}).get("idle")
        idle_samples = [row for row in samples if row.get("phase") == "idle" and idle_start is not None and row["monotonic_ns"] >= idle_start]
        complete_window = (len(idle_samples) >= 2 and idle_samples[0]["monotonic_ns"] == idle_start
                           and idle_samples[-1]["monotonic_ns"] - idle_start >= spec["duration_seconds"] * 1e9)
        if not subject_failed and not complete_window:
            capture_errors.append("idle observation window was incomplete")
        if complete_window:
            try:
                before, after = loopback_bytes(idle_samples[0]), loopback_bytes(idle_samples[-1])
                idle_evaluated = True
                if after != before:
                    functional_errors.append("unexpected loopback traffic during the idle window")
            except (KeyError, ValueError, IndexError):
                capture_errors.append("idle loopback counters were unavailable")
    recovery_evaluated = not spec.get("recovery_seconds", 0) or normal_readiness_failed
    if not recovery_evaluated:
        recovery_start = manifest.get("phase_times_ns", {}).get("recovery")
        recovery_samples = [row for row in samples if row.get("phase") == "recovery" and recovery_start is not None and row["monotonic_ns"] >= recovery_start]
        recovery_evaluated = (len(recovery_samples) >= 2 and recovery_samples[0]["monotonic_ns"] == recovery_start
                              and recovery_samples[-1]["monotonic_ns"] - recovery_start >= spec["recovery_seconds"] * 1e9)
        if not subject_failed and not recovery_evaluated:
            capture_errors.append("recovery observation window was incomplete")
        if recovery_evaluated:
            try:
                boundaries = (recovery_samples[0], recovery_samples[-1])
                backlog = [row["ledger_counts"]["accepted_minus_received"] for row in boundaries]
                for row in boundaries:
                    row["storage"]["subject_file_bytes"]
            except KeyError:
                recovery_evaluated = False
                capture_errors.append("recovery backlog or storage boundary evidence was unavailable")
            else:
                if any(backlog):
                    functional_errors.append("application-side backlog remained during recovery")
    sender_evidence_complete = sender_started and manifest.get("sender_exit") == 0 and not sender_capture_failure and bool(sender_completions)
    never_attempted = len(set(range(count)) - {row["sequence"] for row in attempted}) if sender_evidence_complete or idle else None
    return {"capture_valid": not capture_errors, "functional_pass": not functional_errors and recovery_evaluated and (delivery_evaluated or (idle_evaluated and not capture_errors)),
            "delivery_evaluated": delivery_evaluated,
            "recovery_evaluated": recovery_evaluated if spec.get("recovery_seconds", 0) and not normal_readiness_failed else False,
            "guest_cpu_psi_some_fraction": guest_cpu_psi_some_fraction,
            "guest_cpu_psi_drain_fraction": guest_cpu_psi_drain_fraction,
            "oom_at_exit": oom_at_exit,
            "capture_errors": capture_errors, "functional_errors": functional_errors,
            "coverage": coverage, "planned": count, "never_attempted": never_attempted,
            "attempted": len(attempted), "sender_final": len(finished),
            "received": len(delivered), "missing": missing if delivery_evaluated or idle else None,
            "duplicate_or_unexpected": unexpected if delivery_evaluated or idle else None}


def validate(spec, manifest, samples, sender, receiver, input_errors=None):
    try:
        return _validate(spec, manifest, samples, sender, receiver, input_errors)
    except Exception as error:
        errors = [item for values in (input_errors or {}).values() for item in values]
        errors.append(f"validator could not interpret evidence: {type(error).__name__}: {error}")
        errors.extend(manifest.get("collector_errors", []))
        functional = []
        subject_failed = bool(manifest.get("subject_unexpected_exit"))
        final_events = manifest.get("subject_final_memory_events")
        if subject_failed:
            functional.append("subject exited unexpectedly")
        if isinstance(final_events, dict) and any(final_events.get(key, 0) for key in ("oom", "oom_kill")):
            functional.append("subject cgroup recorded an OOM event")
        oom_at_exit = ("recorded" if "subject cgroup recorded an OOM event" in functional
                       else "not_recorded" if isinstance(final_events, dict) else "unknown") if subject_failed else "not_applicable"
        count_event = lambda rows, event: sum(isinstance(row, dict) and row.get("event") == event for row in rows)
        return {"capture_valid": False, "functional_pass": False, "delivery_evaluated": False,
                "recovery_evaluated": False, "oom_at_exit": oom_at_exit, "capture_errors": errors,
                "functional_errors": functional, "coverage": None, "planned": spec.get("message_count"),
                "never_attempted": None, "attempted": count_event(sender, "attempted"),
                "sender_final": count_event(sender, "sender_final"), "received": count_event(receiver, "received"),
                "missing": None, "duplicate_or_unexpected": None}


def load_inputs(root, manifest):
    errors = {name: [] for name in ("samples", "sender", "receiver")}
    samples = jsonl(root / "samples.jsonl", errors["samples"])
    sender_path = root / "sender.jsonl"
    receiver_path = root / "receiver.jsonl"
    sender_optional = not peer_started(manifest, "sender") or (manifest.get("subject_unexpected_exit") and (manifest.get("sender_exit") is None or manifest.get("sender_cancelled") is True))
    receiver_optional = not peer_started(manifest, "receiver") or (manifest.get("subject_unexpected_exit") and (manifest.get("receiver_exit") is None or manifest.get("receiver_cancelled") is True))
    if manifest.get("normal_readiness_failed"):
        sender_optional |= bool(manifest.get("sender_stopped_after_readiness_failure"))
        receiver_optional |= bool(manifest.get("receiver_stopped_after_readiness_failure"))
    sender = jsonl(sender_path, None if sender_optional and not sender_path.exists() else errors["sender"])
    receiver = jsonl(receiver_path, None if receiver_optional and not receiver_path.exists() else errors["receiver"])
    spec_path = root / "experiment.json"
    try:
        spec = json.loads(spec_path.read_text())
        if spec.get("generator_mode") == "prepared_packets":
            metadata = json.loads((root / "prepared-packets-metadata.json").read_text())
            prepared_path = root / "prepared-packets.jsonl"
            prepared = jsonl(prepared_path, errors["sender"])
            if (metadata != manifest.get("packet_preparation") or metadata.get("message_count") != spec["message_count"]
                    or metadata.get("stamp_target") != spec["propagation_stamp_target"]
                    or metadata.get("packets_sha256") != hashlib.sha256(prepared_path.read_bytes()).hexdigest()
                    or metadata["finished_ns"] < metadata["started_ns"]
                    or metadata["finished_ns"] >= manifest["phase_times_ns"]["launch_requested"]
                    or [row["sequence"] for row in prepared] != list(range(spec["message_count"]))
                    or len({row["transient_id"] for row in prepared}) != spec["message_count"]
                    or any(row["sha256"] != expected_payload_hash(row["sequence"], spec)
                           or len(bytes.fromhex(row["message_hash"])) != 32
                           or len(bytes.fromhex(row["transient_id"])) != 32
                           or not base64.b64decode(row["packet_base64"], validate=True) for row in prepared)):
                errors["sender"].append("prepared packet evidence differs from the frozen workload")
    except (OSError, ValueError, TypeError, KeyError) as error:
        errors["sender"].append(f"prepared input evidence unavailable: {type(error).__name__}: {error}")
    return samples, sender, receiver, errors


def guest_status_matches(guest_status, runner_status, manifest, summary):
    if manifest.get("finalized") is not True or summary.get("capture_valid") is not True:
        return False
    return ((guest_status, runner_status) == ("guest_script_exit=0", "runner_process_exit=0") and summary.get("functional_pass") is True
            or (guest_status, runner_status) == ("guest_script_exit=1", "runner_process_exit=1") and summary.get("functional_pass") is False)


def same_json(left, right):
    options = {"sort_keys": True, "separators": (",", ":"), "allow_nan": False}
    return json.dumps(left, **options) == json.dumps(right, **options)


def verify_guest_outcome(root, spec, guest_status, runner_status, experiment_bytes):
    manifest = json.loads((root / "manifest.json").read_text())
    summary = json.loads((root / "summary.json").read_text())
    if ((root / "experiment.json").read_bytes() != experiment_bytes or not same_json(json.loads(experiment_bytes), spec)
            or not guest_status_matches(guest_status, runner_status, manifest, summary)
            or not same_json(validate(spec, manifest, *load_inputs(root, manifest)), summary)):
        raise ValueError("guest status or saved summary does not match freshly validated evidence")
    return summary


def main():
    root = Path(sys.argv[1])
    spec = json.loads((root / "experiment.json").read_text())
    manifest = json.loads((root / "manifest.json").read_text())
    samples, sender, receiver, errors = load_inputs(root, manifest)
    result = validate(spec, manifest, samples, sender, receiver, errors)
    (root / "summary.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["capture_valid"] and result["functional_pass"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
