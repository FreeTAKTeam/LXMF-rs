"""Opt-in Linux resident-page attribution for the existing local soak harness."""
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import time

HEADER = re.compile(r"^([0-9a-f]+)-([0-9a-f]+) ([r-][w-][x-][ps]) [0-9a-f]+ ([0-9a-f]+):([0-9a-f]+) (\d+)(?:\s+(.*))?$")
FIELD = re.compile(r"^(Anonymous|Rss|Swap):\s+(\d+) kB$")


def analyze_smaps(smaps, rollup_before, rollup_after):
    """Account whole VMAs in the resident Anonymous domain, including file COW."""
    rows = []
    for line in smaps.splitlines():
        header = HEADER.fullmatch(line)
        if header:
            start, end, flags, major, minor, inode, name = header.groups()
            start, end = int(start, 16), int(end, 16)
            if end <= start or (rows and start < rows[-1]["end"]):
                raise ValueError("overlapping or invalid VMA range")
            rows.append({"start": start, "end": end, "named": (
                flags.endswith("p") and int(major, 16) == int(minor, 16) == int(inode) == 0
                and name == "[anon:mimalloc]"), "values": {}})
        elif re.match(r"^[0-9a-fA-F]+-[0-9a-fA-F]+\s", line):
            raise ValueError("malformed VMA header")
        elif line.startswith(("Anonymous:", "Rss:", "Swap:")):
            field = FIELD.fullmatch(line)
            if not rows or not field or field[1] in rows[-1]["values"]:
                raise ValueError("malformed or duplicate resident field")
            rows[-1]["values"][field[1]] = int(field[2]) * 1024
    if not rows:
        raise ValueError("empty smaps capture")
    for row in rows:
        values = row["values"]
        if not {"Anonymous", "Rss", "Swap"} <= values.keys():
            raise ValueError("missing resident field")
        if not 0 <= values["Anonymous"] <= values["Rss"] <= row["end"] - row["start"]:
            raise ValueError("inconsistent resident bytes")
    total = sum(row["values"]["Anonymous"] for row in rows)
    for rollup in (rollup_before, rollup_after):
        matches = re.findall(r"^Anonymous:\s+(\d+) kB$", rollup, re.MULTILINE)
        if len(matches) != 1 or total <= 0 or int(matches[0]) * 1024 != total:
            raise ValueError("smaps/rollup anonymous residency does not reconcile")
    owned = sum(row["values"]["Anonymous"] for row in rows if row["named"])
    return {"total_anon_bytes": total, "mimalloc_anon_bytes": owned,
            "unclassified_anon_bytes": total - owned, "attributed_fraction": owned / total,
            "vmas": len(rows), "mimalloc_vmas": sum(row["named"] for row in rows),
            "swap_bytes": sum(row["values"]["Swap"] for row in rows)}


def thread_states(pid):
    states = {}
    for task in Path(f"/proc/{pid}/task").iterdir():
        match = re.search(r"^State:\s+(\S)", (task / "status").read_text(), re.MULTILINE)
        if not match:
            raise RuntimeError("missing task state")
        states[task.name] = match[1]
    if not states:
        raise RuntimeError("no daemon threads")
    return states


def capture_resident(pid, out, index, descriptor):
    """Freeze the owned daemon; defer ordinary cancellation until after resume."""
    if pid <= 0 or pid == os.getpid():
        raise ValueError("capture requires the separate harness-owned daemon")
    stopped = False
    previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
    started = time.monotonic()
    try:
        if any(state in ("T", "t") for state in thread_states(pid).values()):
            raise RuntimeError("daemon was already stopped before capture")
        stopped = True
        signal.pidfd_send_signal(descriptor, signal.SIGSTOP)
        deadline, previous = time.monotonic() + 2, None
        while True:
            states = thread_states(pid)
            if states == previous and all(state == "T" for state in states.values()):
                break
            if time.monotonic() >= deadline:
                raise RuntimeError("daemon group stop did not complete")
            previous = states
            time.sleep(.001)
        base = Path(f"/proc/{pid}")
        before = (base / "smaps_rollup").read_text()
        smaps = (base / "smaps").read_text()
        after = (base / "smaps_rollup").read_text()
        after_states = thread_states(pid)
        if after_states != states:
            raise RuntimeError("daemon threads changed or resumed during capture")
    finally:
        try:
            if stopped:
                signal.pidfd_send_signal(descriptor, signal.SIGCONT)
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
    pause = time.monotonic() - started
    summary = analyze_smaps(smaps, before, after)
    raw = {"schema": "lxmf-rs-resident-capture-v1", "index": index, "pid": pid,
           "started_monotonic": started, "resumed_monotonic": started + pause,
           "pause_secs": pause, "threads_before": states, "threads_after": after_states,
           "smaps": smaps, "rollup_before": before, "rollup_after": after}
    data = (json.dumps(raw, sort_keys=True) + "\n").encode()
    directory = out / "resident"
    directory.mkdir(exist_ok=True)
    path = directory / f"{index:06}.json"
    path.write_bytes(data)
    return {**summary, "raw_capture_path": str(path.relative_to(out)),
            "raw_capture_sha256": hashlib.sha256(data).hexdigest(), "pause_secs": pause}


def verify_resident_run(root):
    """Replay every capture; no invalid or missing observation can be skipped."""
    root = Path(root)
    run = json.loads((root / "run.json").read_text())
    rows = [json.loads(line) for line in (root / "samples.jsonl").read_text().splitlines()]
    if not rows or [row["index"] for row in rows] != list(range(len(rows))):
        raise ValueError("missing or duplicate sample index")
    expected = {f"resident/{i:06}.json" for i in range(len(rows))}
    if {str(path.relative_to(root)) for path in (root / "resident").glob("*.json")} != expected:
        raise ValueError("missing or unexpected raw resident capture")
    for row in rows:
        counts = row["resident_memory"]
        path = f"resident/{row['index']:06}.json"
        data = (root / path).read_bytes()
        if counts["raw_capture_path"] != path or hashlib.sha256(data).hexdigest() != counts["raw_capture_sha256"]:
            raise ValueError("raw capture identity/hash mismatch")
        raw = json.loads(data)
        if (raw["schema"] != "lxmf-rs-resident-capture-v1"
                or raw["index"] != row["index"] or raw["pid"] != run["daemon_pid"]
                or raw["started_monotonic"] < row["monotonic"]
                or raw["threads_before"] != raw["threads_after"] or not raw["threads_before"]
                or set(raw["threads_before"].values()) != {"T"}):
            raise ValueError("capture identity/order/quiescence mismatch")
        if (raw["pause_secs"] <= 0 or counts["pause_secs"] != raw["pause_secs"]
                or abs(raw["resumed_monotonic"] - raw["started_monotonic"] - raw["pause_secs"]) > 1e-6):
            raise ValueError("capture pause timing mismatch")
        replay = analyze_smaps(raw["smaps"], raw["rollup_before"], raw["rollup_after"])
        if any(counts.get(key) != value for key, value in replay.items()):
            raise ValueError("resident accounting replay mismatch")
    times = [row["monotonic"] - rows[0]["monotonic"] for row in rows]
    gaps = [b - a for a, b in zip(times, times[1:])]
    if not gaps or any(gap <= 0 for gap in gaps):
        raise ValueError("invalid sample timing")
    phases = ("expiry", "quota", "payload")
    anchor = next((i for i, row in enumerate(rows)
                   if min(row["metrics"]["storage"]["maintenance"][name]["runs_total"]
                          for name in phases) >= 2), None)
    errors = sum(rows[-1]["metrics"]["storage"]["maintenance"][name]["errors_total"] for name in phases)
    duration = run.get("measured_duration_secs", 0)
    valid = (run.get("completed") is True and run.get("resident_memory") is True
             and run.get("duration_secs", 0) >= 3600 and duration >= 3600
             and max(gaps) <= 10 and 0 <= duration - times[-1] <= 10 and errors == 0)
    post = rows[anchor:] if anchor is not None else []
    minimum = min(post, key=lambda row: row["resident_memory"]["attributed_fraction"]) if post else None
    return {"schema": "lxmf-rs-resident-gate-v1", "captures_replayed": len(rows),
            "post_second_pass_captures": len(post), "anchor_index": anchor,
            "minimum_sample_index": minimum["index"] if minimum else None,
            "minimum_resident_memory": minimum["resident_memory"] if minimum else None,
            "capture_duration_coverage_valid": bool(valid),
            "resident_owner_80_percent_gate": bool(valid and post and all(
                row["resident_memory"]["mimalloc_anon_bytes"] * 5 >=
                row["resident_memory"]["total_anon_bytes"] * 4 for row in post)),
            "total_pause_secs": sum(row["resident_memory"]["pause_secs"] for row in rows),
            "max_pause_secs": max(row["resident_memory"]["pause_secs"] for row in rows)}


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_root", type=Path)
    print(json.dumps(verify_resident_run(parser.parse_args().run_root), indent=2, sort_keys=True))
