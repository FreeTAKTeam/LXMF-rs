#!/usr/bin/env python3
"""Issue #663 resource soak: production-shaped reticulumd under cgroup limits.

Subcommands:
  fixture  build and run the Rust fixture generator (skips when fixture.json exists)
  run      start reticulumd in durable-broker mode under systemd-run limits, drive
           it with the SDK poll client, sample /proc, cgroup v2 and /metrics
  report   render a Markdown summary from a run directory

Everything is read from the local machine; nothing contacts production. The
report states what was measured and for how long; a 10-minute run is a smoke
check of the harness, not acceptance evidence.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import time
import urllib.request
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[2]
SAMPLE_SCHEMA = "lxmf-rs-resource-soak-663-sample-v1"
RUN_SCHEMA = "lxmf-rs-resource-soak-663-run-v1"
MEMORY_STAT_KEYS = ("anon", "file", "active_file", "inactive_file", "kernel", "file_dirty", "shmem")


# ---------------------------------------------------------------- parsing


def parse_kv_file(text: str) -> dict[str, int]:
    """Parse `key value` lines (memory.stat, memory.events, cpu.stat)."""
    values: dict[str, int] = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) >= 2:
            try:
                values[parts[0]] = int(parts[1])
            except ValueError:
                continue
    return values


def parse_proc_status_like(text: str) -> dict[str, int]:
    """Parse `Key:   123 kB` lines (smaps_rollup, status) into bytes."""
    values: dict[str, int] = {}
    for line in text.splitlines():
        if ":" not in line:
            continue
        key, _, rest = line.partition(":")
        parts = rest.split()
        if not parts:
            continue
        try:
            number = int(parts[0])
        except ValueError:
            continue
        if len(parts) > 1 and parts[1] == "kB":
            number *= 1024
        values[key.strip()] = number
    return values


def parse_proc_io(text: str) -> dict[str, int]:
    values: dict[str, int] = {}
    for line in text.splitlines():
        key, _, rest = line.partition(":")
        try:
            values[key.strip()] = int(rest.strip())
        except ValueError:
            continue
    return values


def parse_proc_stat_cpu_ticks(text: str) -> int:
    fields = text[text.rfind(")") + 2 :].split()
    return int(fields[11]) + int(fields[12])


def parse_pressure(text: str) -> dict[str, dict[str, float]]:
    """Parse PSI lines: `some avg10=0.00 avg60=0.00 avg300=0.00 total=0`."""
    result: dict[str, dict[str, float]] = {}
    for line in text.splitlines():
        parts = line.split()
        if not parts:
            continue
        entry: dict[str, float] = {}
        for item in parts[1:]:
            key, _, value = item.partition("=")
            try:
                entry[key] = float(value)
            except ValueError:
                continue
        result[parts[0]] = entry
    return result


def parse_cgroup_path(text: str) -> str | None:
    """Return the cgroup v2 relative path from /proc/<pid>/cgroup."""
    for line in text.splitlines():
        parts = line.split(":", 2)
        if len(parts) == 3 and parts[0] == "0" and parts[1] == "":
            return parts[2]
    return None


def parse_size(value: str) -> int:
    units = {"K": 1024, "M": 1024**2, "G": 1024**3}
    value = value.strip()
    if value[-1:].upper() in units:
        return int(float(value[:-1]) * units[value[-1:].upper()])
    return int(value)


# ---------------------------------------------------------------- sampling


def read_text(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8")
    except OSError:
        return None


def sample_process(pid: int) -> dict[str, Any]:
    base = Path(f"/proc/{pid}")
    sample: dict[str, Any] = {}
    rollup = read_text(base / "smaps_rollup")
    if rollup is not None:
        values = parse_proc_status_like(rollup)
        sample["rss_bytes"] = values.get("Rss")
        sample["anon_bytes"] = values.get("Anonymous")
        sample["swap_bytes"] = values.get("Swap")
    io_text = read_text(base / "io")
    if io_text is not None:
        sample["io"] = parse_proc_io(io_text)
    stat = read_text(base / "stat")
    if stat is not None:
        sample["cpu_ticks"] = parse_proc_stat_cpu_ticks(stat)
    return sample


def sample_cgroup(cgroup_dir: Path | None) -> dict[str, Any]:
    if cgroup_dir is None:
        return {"available": False}
    sample: dict[str, Any] = {"available": True, "path": str(cgroup_dir)}
    for name in ("memory.current", "memory.swap.current", "memory.high", "memory.max"):
        text = read_text(cgroup_dir / name)
        if text is not None:
            text = text.strip()
            sample[name.replace(".", "_")] = None if text == "max" else int(text)
    stat = read_text(cgroup_dir / "memory.stat")
    if stat is not None:
        values = parse_kv_file(stat)
        sample["memory_stat"] = {key: values.get(key) for key in MEMORY_STAT_KEYS}
    events = read_text(cgroup_dir / "memory.events")
    if events is not None:
        sample["memory_events"] = parse_kv_file(events)
    pressure = read_text(cgroup_dir / "memory.pressure")
    if pressure is not None:
        sample["memory_pressure"] = parse_pressure(pressure)
    cpu = read_text(cgroup_dir / "cpu.stat")
    if cpu is not None:
        sample["cpu_stat"] = parse_kv_file(cpu)
    return sample


def cgroup_dir_for_pid(pid: int) -> Path | None:
    text = read_text(Path(f"/proc/{pid}/cgroup"))
    if text is None:
        return None
    relative = parse_cgroup_path(text)
    if relative is None:
        return None
    candidate = Path("/sys/fs/cgroup") / relative.lstrip("/")
    return candidate if candidate.is_dir() else None


def fetch_metrics(port: int) -> dict[str, Any] | None:
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/metrics", timeout=5) as response:
            return json.loads(response.read().decode("utf-8"))
    except Exception:  # noqa: BLE001 - sampler must keep going
        return None


def file_size(path: Path) -> int:
    try:
        return path.stat().st_size
    except OSError:
        return 0


# ---------------------------------------------------------------- processes


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def wait_for_http(port: int, path: str, timeout: float, process: subprocess.Popen) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"process exited early with {process.returncode}")
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}{path}", timeout=1) as response:
                if response.status == 200:
                    return
        except Exception:  # noqa: BLE001
            time.sleep(0.25)
    raise RuntimeError(f"timed out waiting for http://127.0.0.1:{port}{path}")


def release_binary(name: str, example: bool) -> Path:
    directory = REPO / "target" / "release" / ("examples" if example else "")
    return directory / name


def build_release(targets: list[str]) -> None:
    command = ["cargo", "build", "--release", "-p", "reticulumd", *targets]
    print("[soak] " + " ".join(command), flush=True)
    subprocess.run(command, cwd=REPO, check=True)


def systemd_wrapper(memory_high: str, memory_max: str, cpus: str, unit: str) -> list[str] | None:
    if shutil.which("systemd-run") is None or shutil.which("taskset") is None:
        return None
    probe = subprocess.run(
        ["systemd-run", "--user", "--scope", "-q", "-p", "MemoryHigh=64M", "true"],
        capture_output=True,
        check=False,
    )
    if probe.returncode != 0:
        return None
    return [
        "systemd-run",
        "--user",
        "--scope",
        "-q",
        "--unit",
        unit,
        "-p",
        f"MemoryHigh={memory_high}",
        "-p",
        f"MemoryMax={memory_max}",
        "taskset",
        "-c",
        cpus,
    ]


def daemon_command(
    daemon_bin: Path,
    root: Path,
    http_port: int,
    transport_port: int,
    zmq_command_port: int | None,
    budget_bytes: int | None,
    announce_interval: int,
) -> list[str]:
    command = [
        str(daemon_bin),
        "--db",
        str(root / "reticulum.db"),
        "--identity",
        str(root / "identity"),
        "--config",
        str(root / "reticulum.toml"),
        "--rpc",
        f"127.0.0.1:{http_port}",
        "--rpc-unix",
        str(root / "rpc.sock"),
        "--transport",
        f"127.0.0.1:{transport_port}",
        "--announce-interval-secs",
        str(announce_interval),
    ]
    if zmq_command_port is not None:
        command += ["--zmq-rpc-command", f"tcp://127.0.0.1:{zmq_command_port}"]
    if budget_bytes is not None:
        command += ["--zmq-durable-broker", "--zmq-broker-budget-bytes", str(budget_bytes)]
    return command


def daemon_env() -> dict[str, str]:
    env = dict(os.environ)
    env.setdefault("TOKIO_WORKER_THREADS", "2")
    env.setdefault("LXMD_PROPAGATION_NODE", "true")
    env.setdefault("RUST_LOG", "warn")
    return env


def terminate(process: subprocess.Popen | None, name: str, log) -> None:
    if process is None or process.poll() is not None:
        return
    process.send_signal(signal.SIGINT)
    try:
        process.wait(timeout=30)
    except subprocess.TimeoutExpired:
        log(f"{name} ignored SIGINT; killing")
        process.kill()
        process.wait(timeout=10)


# ---------------------------------------------------------------- commands


def cmd_fixture(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    manifest = root / "fixture.json"
    if manifest.exists() and not args.force:
        print(f"[soak] fixture already present at {manifest}")
        return 0
    if not args.skip_build:
        build_release(["--example", "resource_soak_663_fixture"])
    command = [
        str(release_binary("resource_soak_663_fixture", example=True)),
        "--root",
        str(root),
        "--entries",
        str(args.entries),
        "--payload-bytes",
        str(args.payload_bytes),
        "--peers",
        str(args.peers),
        "--announces",
        str(args.announces),
        "--budget-bytes",
        str(args.budget_bytes),
        "--seed",
        str(args.seed),
    ]
    print("[soak] " + " ".join(command), flush=True)
    subprocess.run(command, cwd=REPO, check=True)
    return 0


def cmd_run(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    if not (root / "reticulum.db").exists():
        print(f"[soak] no fixture at {root}; run `fixture` first", file=sys.stderr)
        return 2
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    log_file = (out / "harness.log").open("a", encoding="utf-8")

    def log(message: str) -> None:
        line = f"[soak {time.strftime('%H:%M:%S')}] {message}"
        print(line, flush=True)
        log_file.write(line + "\n")
        log_file.flush()

    if not args.skip_build:
        build_release(["--bin", "reticulumd", "--example", "resource_soak_663_client"])
    daemon_bin = Path(args.daemon_bin) if args.daemon_bin else release_binary("reticulumd", example=False)
    client_bin = release_binary("resource_soak_663_client", example=True)
    (root / "reticulum.toml").touch()

    http_port, transport_port, zmq_port, reply_port = (free_port() for _ in range(4))
    unit = f"lxmf-soak-663-{int(time.time())}"
    wrapper = systemd_wrapper(args.memory_high, args.memory_max, args.cpus, unit)
    constrained = wrapper is not None
    if not constrained:
        log("WARNING: systemd-run/taskset unavailable; daemon runs WITHOUT cgroup limits")
        wrapper = []
    command = wrapper + daemon_command(
        daemon_bin, root, http_port, transport_port, zmq_port, args.budget_bytes, 0
    )
    log("daemon: " + " ".join(command))
    daemon_log = (out / "daemon.log").open("a", encoding="utf-8")
    daemon = subprocess.Popen(command, cwd=REPO, env=daemon_env(), stdout=daemon_log, stderr=daemon_log)
    peer: subprocess.Popen | None = None
    client: subprocess.Popen | None = None
    run_meta: dict[str, Any] = {
        "schema": RUN_SCHEMA,
        "label": args.label,
        "profile": args.profile,
        "git_revision": git_revision(),
        "started_unix": time.time(),
        "duration_secs": args.duration_secs,
        "sample_secs": args.sample_secs,
        "constrained": constrained,
        "memory_high": args.memory_high if constrained else None,
        "memory_max": args.memory_max if constrained else None,
        "cpus": args.cpus if constrained else None,
        "fixture": json.loads(read_text(root / "fixture.json") or "{}"),
        "daemon_command": command,
    }
    try:
        wait_for_http(http_port, "/readyz", 120, daemon)
        daemon_pid = resolve_daemon_pid(daemon, daemon_bin)
        cgroup_dir = cgroup_dir_for_pid(daemon_pid)
        run_meta["daemon_pid"] = daemon_pid
        run_meta["cgroup_dir"] = str(cgroup_dir) if cgroup_dir else None
        log(f"daemon ready pid={daemon_pid} cgroup={cgroup_dir}")

        if args.profile == "ingress":
            peer = start_peer(daemon_bin, out, transport_port, log)
        client_cmd = [
            str(client_bin),
            "--endpoint",
            f"tcp://127.0.0.1:{zmq_port}",
            "--response-endpoint",
            f"tcp://127.0.0.1:{reply_port}",
            "--poll-interval-ms",
            str(args.poll_interval_ms),
            "--duration-secs",
            str(args.duration_secs + 5),
            "--report-secs",
            str(args.sample_secs),
        ]
        log("client: " + " ".join(client_cmd))
        client = subprocess.Popen(
            client_cmd,
            cwd=REPO,
            stdout=(out / "client.jsonl").open("a", encoding="utf-8"),
            stderr=(out / "client.log").open("a", encoding="utf-8"),
        )
        sample_loop(out, daemon, daemon_pid, cgroup_dir, http_port, root, args, log)
    finally:
        run_meta["ended_unix"] = time.time()
        terminate(client, "client", log)
        terminate(peer, "peer daemon", log)
        terminate(daemon, "daemon", log)
        (out / "run.json").write_text(json.dumps(run_meta, indent=2, sort_keys=True) + "\n")
    report = render_report(out)
    (out / "report.md").write_text(report)
    print(report)
    return 0


def resolve_daemon_pid(wrapper_process: subprocess.Popen, daemon_bin: Path) -> int:
    """systemd-run --scope execs the payload in place; fall back to a child search."""
    pid = wrapper_process.pid
    comm = read_text(Path(f"/proc/{pid}/comm")) or ""
    if comm.strip() == daemon_bin.name[:15]:
        return pid
    children = read_text(Path(f"/proc/{pid}/task/{pid}/children")) or ""
    for child in children.split():
        child_comm = read_text(Path(f"/proc/{child}/comm")) or ""
        if child_comm.strip() == daemon_bin.name[:15]:
            return int(child)
    return pid


def start_peer(daemon_bin: Path, out: Path, target_transport_port: int, log) -> subprocess.Popen:
    peer_root = out / "peer"
    peer_root.mkdir(exist_ok=True)
    (peer_root / "reticulum.toml").write_text(
        "[[interfaces]]\n"
        'type = "tcp_client"\n'
        "enabled = true\n"
        'host = "127.0.0.1"\n'
        f"port = {target_transport_port}\n"
    )
    command = daemon_command(
        daemon_bin, peer_root, free_port(), free_port(), None, None, announce_interval=1
    )
    log("peer: " + " ".join(command))
    peer_log = (out / "peer.log").open("a", encoding="utf-8")
    return subprocess.Popen(command, cwd=REPO, env=daemon_env(), stdout=peer_log, stderr=peer_log)


def sample_loop(out, daemon, daemon_pid, cgroup_dir, http_port, root, args, log) -> None:
    deadline = time.monotonic() + args.duration_secs
    wal = Path(str(root / "reticulum.db") + "-wal")
    with (out / "samples.jsonl").open("a", encoding="utf-8") as samples:
        index = 0
        while time.monotonic() < deadline:
            if daemon.poll() is not None:
                log(f"daemon exited with {daemon.returncode}; stopping sampler")
                return
            sample = {
                "schema": SAMPLE_SCHEMA,
                "index": index,
                "unix_time": time.time(),
                "monotonic": time.monotonic(),
                "process": sample_process(daemon_pid),
                "cgroup": sample_cgroup(cgroup_dir),
                "wal_bytes": file_size(wal),
                "db_bytes": file_size(root / "reticulum.db"),
                "metrics": fetch_metrics(http_port),
            }
            samples.write(json.dumps(sample, sort_keys=True) + "\n")
            samples.flush()
            if index % 12 == 0:
                cg = sample["cgroup"]
                log(
                    f"sample {index}: rss={fmt_bytes(sample['process'].get('rss_bytes'))} "
                    f"cg={fmt_bytes(cg.get('memory_current'))} "
                    f"high_events={(cg.get('memory_events') or {}).get('high')} wal={fmt_bytes(sample['wal_bytes'])}"
                )
            index += 1
            time.sleep(args.sample_secs)


# ---------------------------------------------------------------- report


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return rows


def fmt_bytes(value: Any) -> str:
    if value is None:
        return "n/a"
    return f"{value / (1024 * 1024):.1f} MiB"


def median_of(rows: list[dict[str, Any]], getter) -> float | None:
    values = [v for v in (getter(row) for row in rows) if isinstance(v, (int, float))]
    return statistics.median(values) if values else None


def rate(first: dict[str, Any], last: dict[str, Any], getter) -> float | None:
    a, b = getter(first), getter(last)
    dt = last["monotonic"] - first["monotonic"]
    if a is None or b is None or dt <= 0:
        return None
    return (b - a) / dt


def summarize_samples(samples: list[dict[str, Any]], sample_secs: float) -> dict[str, Any]:
    if len(samples) < 2:
        return {"error": "fewer than two samples"}
    window = max(2, int(60 / max(sample_secs, 1)))
    first, last = samples[:window], samples[-window:]
    ticks = os.sysconf("SC_CLK_TCK")
    getters = {
        "rss_bytes": lambda r: r["process"].get("rss_bytes"),
        "anon_bytes": lambda r: r["process"].get("anon_bytes"),
        "swap_bytes": lambda r: r["process"].get("swap_bytes"),
        "cgroup_memory_current": lambda r: r["cgroup"].get("memory_current"),
        "cgroup_swap_current": lambda r: r["cgroup"].get("memory_swap_current"),
        "cgroup_file": lambda r: (r["cgroup"].get("memory_stat") or {}).get("file"),
        "cgroup_anon": lambda r: (r["cgroup"].get("memory_stat") or {}).get("anon"),
        "wal_bytes": lambda r: r.get("wal_bytes"),
    }
    summary: dict[str, Any] = {
        "samples": len(samples),
        "elapsed_secs": samples[-1]["monotonic"] - samples[0]["monotonic"],
        "first_minute_median": {k: median_of(first, g) for k, g in getters.items()},
        "last_minute_median": {k: median_of(last, g) for k, g in getters.items()},
    }
    f, l = samples[0], samples[-1]
    cpu_rate = rate(f, l, lambda r: r["process"].get("cpu_ticks"))
    summary["cpu_percent_one_core"] = None if cpu_rate is None else cpu_rate / ticks * 100
    summary["rchar_per_sec"] = rate(f, l, lambda r: (r["process"].get("io") or {}).get("rchar"))
    summary["syscr_per_sec"] = rate(f, l, lambda r: (r["process"].get("io") or {}).get("syscr"))
    summary["read_bytes_per_sec"] = rate(f, l, lambda r: (r["process"].get("io") or {}).get("read_bytes"))
    events = lambda r, k: (r["cgroup"].get("memory_events") or {}).get(k)  # noqa: E731
    summary["memory_events_delta"] = {
        k: (None if events(f, k) is None or events(l, k) is None else events(l, k) - events(f, k))
        for k in ("high", "max", "oom", "oom_kill")
    }
    storage = lambda r: ((r.get("metrics") or {}).get("storage") or {})  # noqa: E731
    summary["write_lock_wait_ms_delta"] = _delta(
        storage(f).get("write_lock_wait_ns_total"), storage(l).get("write_lock_wait_ns_total"), 1e6
    )
    summary["write_ops_delta"] = _delta(storage(f).get("write_ops_total"), storage(l).get("write_ops_total"), 1)
    summary["metrics_available"] = any(r.get("metrics") is not None for r in samples)
    return summary


def _delta(a, b, divisor):
    if a is None or b is None:
        return None
    return (b - a) / divisor


def summarize_client(rows: list[dict[str, Any]]) -> dict[str, Any]:
    totals = [r["total"] for r in rows if "total" in r]
    if not totals:
        return {"polls": 0, "errors": 0, "events": 0, "errors_by_code": {}}
    last = totals[-1]
    p95 = [r["window"]["latency_us"]["p95"] for r in rows if "window" in r and r["window"]["polls"]]
    return {
        "polls": last["polls"],
        "errors": last["errors"],
        "events": last["events"],
        "errors_by_code": last.get("errors_by_code", {}),
        "error_ratio": (last["errors"] / last["polls"]) if last["polls"] else None,
        "latency_p95_us_median": statistics.median(p95) if p95 else None,
        "latency_p95_us_max": max(p95) if p95 else None,
    }


def render_report(out: Path) -> str:
    run = json.loads(read_text(out / "run.json") or "{}")
    samples = load_jsonl(out / "samples.jsonl")
    client = summarize_client(load_jsonl(out / "client.jsonl"))
    summary = summarize_samples(samples, run.get("sample_secs", 5))
    (out / "summary.json").write_text(
        json.dumps({"run": run, "samples": summary, "client": client}, indent=2, sort_keys=True) + "\n"
    )
    lines = [
        f"# Resource soak 663: {run.get('label') or out.name}",
        "",
        f"- Profile `{run.get('profile')}`, duration {run.get('duration_secs')} s, "
        f"sample interval {run.get('sample_secs')} s, git `{run.get('git_revision')}`.",
        f"- Constrained: {run.get('constrained')} (MemoryHigh={run.get('memory_high')}, "
        f"MemoryMax={run.get('memory_max')}, cpus={run.get('cpus')}).",
        f"- Fixture: {describe_fixture(run.get('fixture') or {})}",
        f"- Daemon metrics endpoint sampled: {summary.get('metrics_available')}.",
        "",
    ]
    if "error" in summary:
        lines.append(f"**No usable samples:** {summary['error']}")
        return "\n".join(lines) + "\n"
    lines += [
        "| Metric | First minute median | Last minute median |",
        "|---|---:|---:|",
    ]
    for key in summary["first_minute_median"]:
        lines.append(
            f"| {key} | {fmt_bytes(summary['first_minute_median'][key])} | {fmt_bytes(summary['last_minute_median'][key])} |"
        )
    lines += [
        "",
        "| Rate / delta over the run | Value |",
        "|---|---:|",
        f"| daemon CPU (% of one core) | {fmt_num(summary['cpu_percent_one_core'])} |",
        f"| logical reads (rchar) MiB/s | {fmt_num(None if summary['rchar_per_sec'] is None else summary['rchar_per_sec'] / 2**20)} |",
        f"| read syscalls/s | {fmt_num(summary['syscr_per_sec'])} |",
        f"| physical reads MiB/s | {fmt_num(None if summary['read_bytes_per_sec'] is None else summary['read_bytes_per_sec'] / 2**20)} |",
        f"| memory.events high / max / oom delta | {summary['memory_events_delta']['high']} / {summary['memory_events_delta']['max']} / {summary['memory_events_delta']['oom']} |",
        f"| write lock wait ms delta | {fmt_num(summary['write_lock_wait_ms_delta'])} |",
        f"| write ops delta | {summary['write_ops_delta']} |",
        f"| SDK polls / errors / ratio | {client['polls']} / {client['errors']} / {fmt_num(client.get('error_ratio'))} |",
        f"| SDK poll p95 latency us (median / max of windows) | {fmt_num(client.get('latency_p95_us_median'))} / {client.get('latency_p95_us_max')} |",
        "",
        "## Acceptance checks (issue #663)",
        "",
        "| Check | Result |",
        "|---|---|",
    ]
    lines += acceptance_rows(summary, client, run)
    lines += [
        "",
        f"This run lasted {summary['elapsed_secs']:.0f} s. Anything under several hours is a "
        "harness smoke check, not acceptance evidence for the production deployment.",
    ]
    return "\n".join(lines) + "\n"


def acceptance_rows(summary, client, run) -> list[str]:
    def row(name: str, result: str) -> str:
        return f"| {name} | {result} |"

    rows = []
    high = summary["memory_events_delta"].get("high")
    rows.append(row("no continuing reclaim (memory.events high delta)", "not measured" if high is None else ("pass (0)" if high == 0 else f"fail (+{high})")))
    swap_a = summary["first_minute_median"].get("swap_bytes")
    swap_b = summary["last_minute_median"].get("swap_bytes")
    rows.append(row("no continuing swap growth", "not measured" if None in (swap_a, swap_b) else ("pass" if swap_b <= swap_a + 8 * 2**20 else f"fail (+{fmt_bytes(swap_b - swap_a)})")))
    wal_a = summary["first_minute_median"].get("wal_bytes")
    wal_b = summary["last_minute_median"].get("wal_bytes")
    rows.append(row("bounded WAL", "not measured" if None in (wal_a, wal_b) else f"{fmt_bytes(wal_a)} -> {fmt_bytes(wal_b)}"))
    ratio = client.get("error_ratio")
    rows.append(row("SDK poll errors", "not measured" if ratio is None else ("pass (0)" if client["errors"] == 0 else f"{client['errors']} ({ratio:.4%})")))
    rows.append(row("fresh inbound/outbound/fanout delivery", "not driven by this harness (item 6)"))
    rows.append(row("multi-hour duration", "pass" if run.get("duration_secs", 0) >= 4 * 3600 else f"not met ({run.get('duration_secs')} s)"))
    return rows


def describe_fixture(fixture: dict[str, Any]) -> str:
    db = fixture.get("database") or {}
    ann = fixture.get("announce_root") or {}
    if not db:
        return "unknown"
    return (
        f"{db.get('entries')} entries, db {fmt_bytes(db.get('db_bytes'))}, wal {fmt_bytes(db.get('wal_bytes'))}, "
        f"{db.get('peers')} peers, {ann.get('cache_files')} announce cache files"
    )


def fmt_num(value: Any) -> str:
    if value is None:
        return "n/a"
    return f"{value:.3f}" if isinstance(value, float) else str(value)


def git_revision() -> str | None:
    try:
        return subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True, check=True
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def cmd_report(args: argparse.Namespace) -> int:
    out = Path(args.out).resolve()
    report = render_report(out)
    (out / "report.md").write_text(report)
    print(report)
    return 0


# ---------------------------------------------------------------- cli


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    fixture = sub.add_parser("fixture", help="generate the production-shaped fixture")
    fixture.add_argument("--root", default="target/soak-663/fixture")
    fixture.add_argument("--entries", type=int, default=300_000)
    fixture.add_argument("--payload-bytes", type=int, default=2_048)
    fixture.add_argument("--peers", type=int, default=300)
    fixture.add_argument("--announces", type=int, default=100_000)
    fixture.add_argument("--budget-bytes", type=int, default=6 * 1024**3)
    fixture.add_argument("--seed", type=int, default=663)
    fixture.add_argument("--force", action="store_true")
    fixture.add_argument("--skip-build", action="store_true")
    fixture.set_defaults(func=cmd_fixture)

    run = sub.add_parser("run", help="run the daemon under cgroup limits and sample it")
    run.add_argument("--root", default="target/soak-663/fixture")
    run.add_argument("--out", required=True)
    run.add_argument("--label", default="")
    run.add_argument("--profile", choices=("idle", "ingress"), default="idle")
    run.add_argument("--duration-secs", type=int, default=600)
    run.add_argument("--sample-secs", type=int, default=5)
    run.add_argument("--poll-interval-ms", type=int, default=100)
    run.add_argument("--memory-high", default="768M")
    run.add_argument("--memory-max", default="1G")
    run.add_argument("--cpus", default="0,1")
    run.add_argument("--budget-bytes", type=int, default=6 * 1024**3)
    run.add_argument("--daemon-bin", default=None)
    run.add_argument("--skip-build", action="store_true")
    run.set_defaults(func=cmd_run)

    report = sub.add_parser("report", help="render report.md from a run directory")
    report.add_argument("--out", required=True)
    report.set_defaults(func=cmd_report)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
