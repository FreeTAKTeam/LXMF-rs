#!/usr/bin/env python3
"""Run one isolated daemon smoke profile; invoke through run.sh."""
import datetime
import errno
import fcntl
import hashlib
import json
import math
import os
import re
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent


class SubjectFailure(Exception):
    pass


def subject_log_is_error(message):
    return bool(re.search(r"panicked at", message, re.IGNORECASE)
                or re.search(r"^\s*(?:\[[^\]]+\]\s*)?\[(?:error|critical)\](?:\s|$)", message, re.IGNORECASE)
                or re.search(r"^\s*(?:\d{4}-\d\d-\d\dT\S+\s+)?(?:ERROR|CRITICAL)(?:\s|:|$)", message))


def check_spec(spec):
    if spec["state"] != "frozen" or spec["profile"] not in ("smoke", "preflight-only-short-dry-run", "idle", "preflight-only-idle-dry-run", "offline", "preflight-only-offline-recovery-dry-run", "normal", "high-load", "preflight-only-normal-dry-run"):
        raise ValueError("unknown or unfrozen profile")
    if spec.get("generator_mode") not in (None, "prepared_packets"):
        raise ValueError("unknown generator mode")
    idle = spec["profile"] in ("idle", "preflight-only-idle-dry-run")
    if (idle and spec["message_count"] != 0) or (not idle and spec["message_count"] <= 0):
        raise ValueError("idle requires zero messages; other profiles require at least one")
    for key in ("payload_bytes", "interval_seconds", "duration_seconds", "warmup_seconds", "route_deadline_seconds", "sender_drain_seconds", "receiver_drain_seconds", "receiver_quiet_seconds", "sample_interval_seconds"):
        if spec[key] <= 0:
            raise ValueError(f"{key} must be positive")
    if (spec["message_count"] - 1) * spec["interval_seconds"] >= spec["duration_seconds"]:
        raise ValueError("message schedule does not fit the profile duration")
    if spec["receiver_quiet_seconds"] >= spec["receiver_drain_seconds"]:
        raise ValueError("receiver quiet interval does not fit the drain deadline")
    if spec["payload_bytes"] < len(f"{spec['seed']}:{spec['message_count'] - 1:06d}:".encode()):
        raise ValueError("payload is too short for its logical identifier")
    if spec["profile"] == "smoke" and spec["duration_seconds"] != 600:
        raise ValueError("the planned smoke profile must run for 600 seconds")
    if spec["profile"] == "smoke" and spec["sample_interval_seconds"] != 5:
        raise ValueError("the planned smoke must sample every 5 seconds")
    if spec["profile"] == "idle" and (spec["duration_seconds"] != 1800 or spec["sample_interval_seconds"] != 5):
        raise ValueError("the planned idle profile requires 1800 seconds and five-second sampling")
    if spec["profile"] == "offline" and (spec["duration_seconds"] != 7200 or spec["recovery_seconds"] != 3600 or spec["sample_interval_seconds"] != 5):
        raise ValueError("the planned offline profile requires 7200 seconds, 3600 seconds of recovery, and five-second sampling")
    if spec["profile"] == "normal" and (spec["duration_seconds"] != 7200 or spec["recovery_seconds"] != 3600 or spec["sample_interval_seconds"] != 5):
        raise ValueError("the planned normal profile requires 7200 seconds, 3600 seconds of recovery, and five-second sampling")
    if spec["profile"] == "high-load" and (spec["duration_seconds"] < 300 or spec["recovery_seconds"] < 60
                                             or spec["sample_interval_seconds"] != 5 or spec.get("generator_mode") != "prepared_packets"
                                             or not spec["receiver_poll_interval_seconds"] <= spec.get("receiver_poll_timeout_seconds", 0) < spec["receiver_drain_seconds"]):
        raise ValueError("the high-load profile requires a five-minute window, recovery, and prepared packets")
    if spec["profile"] == "preflight-only-offline-recovery-dry-run" and spec["recovery_seconds"] <= 0:
        raise ValueError("the recovery preflight must declare a positive recovery interval")
    if spec["profile"] in ("smoke", "offline") and (spec["acceptance_latency_target_seconds"] <= 0 or spec["retrieval_latency_target_seconds"] <= 0):
        raise ValueError("the profile must declare positive latency targets")
    if spec["profile"] in ("normal", "high-load", "preflight-only-normal-dry-run") and any(
            spec[key] <= 0 for key in ("receiver_poll_interval_seconds", "normal_delivery_target_seconds", "acceptance_latency_target_seconds", "recovery_seconds")):
        raise ValueError("normal traffic requires positive polling, latency, and recovery values")
    if spec["profile"] in ("normal", "high-load", "preflight-only-normal-dry-run"):
        threshold = spec["max_guest_cpu_psi_some_fraction"]
        if type(threshold) not in (int, float) or not math.isfinite(threshold) or not 0 <= threshold <= 1:
            raise ValueError("normal traffic requires a finite CPU pressure threshold between zero and one")
    if spec.get("generator_mode") == "prepared_packets":
        if (spec["profile"] not in ("normal", "high-load", "preflight-only-normal-dry-run")
                or spec["propagation_stamp_target"] != 16 or spec["propagation_stamp_flexibility"] != 3
                or spec["propagation_transfer_kb"] != 256 or spec["propagation_sync_kb"] != 10240
                or not 0 < spec["maximum_schedule_lateness_seconds"] < spec["interval_seconds"]
                or not 0 < spec["maximum_poll_lateness_seconds"] <= 2
                or not 0 < spec.get("max_generator_receipts", 900) < 1024
                or spec["preparation_deadline_seconds"] <= 0):
            raise ValueError("prepared packet profile has invalid frozen targets")


def check_headroom(root, spec):
    disk = shutil.disk_usage(root).free
    memory = next(int(line.split()[1]) * 1024 for line in Path("/proc/meminfo").read_text().splitlines() if line.startswith("MemAvailable:"))
    if disk < spec["minimum_free_disk_bytes"] or memory < spec["minimum_available_memory_bytes"]:
        raise RuntimeError(f"host safety budget exhausted: disk={disk}, available_memory={memory}")
    return {"free_disk_bytes": disk, "available_memory_bytes": memory}


def command(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def sha256_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def sha256_tree(root):
    root = Path(root)
    digest = hashlib.sha256()
    for path in sorted(root.rglob("*")):
        if path.is_file():
            digest.update(str(path.relative_to(root)).encode() + b"\0")
            digest.update(bytes.fromhex(sha256_file(path)))
    return digest.hexdigest()


def validate_guest_cache(evidence, spec, guest_boot_id, guest_marker):
    residency = evidence.get("residency", {})
    expected = set(spec["subject_runtime_trees_sha256"]) | set(spec["subject_runtime_files_sha256"]) | {spec["rust_executable"]}
    roots = residency.get("roots", {})
    if (evidence.get("guest_boot_id") != guest_boot_id or guest_boot_id == spec["host_boot_id"]
            or evidence.get("host_boot_id") != spec["host_boot_id"] or evidence.get("guest_marker") != guest_marker
            or evidence.get("virtualization") not in ("kvm", "qemu") or set(roots) != expected
            or residency.get("resident_pages") != 0 or residency.get("skipped_symlinks") != []
            or any(value.get("resident_pages") != 0 or value.get("files", 0) < 1 for value in roots.values())):
        raise ValueError("guest cold-cache verification failed")


def request_guest_cache(run_id, run, spec):
    control = Path('/home/ubuntu/rch-page-audit/cache-control')
    control.mkdir(exist_ok=False)
    pending = control / 'request.tmp'
    pending.write_text(run_id + '\n')
    pending.replace(control / 'request')
    response = control / 'response.json'
    deadline = time.monotonic() + 120
    while not response.exists():
        if time.monotonic() >= deadline:
            raise TimeoutError('guest cache-control service did not respond')
        time.sleep(0.2)
    raw = response.read_text()
    (run / 'guest-cache-control.json').write_text(raw)
    evidence = json.loads(raw)
    boot_id = Path('/proc/sys/kernel/random/boot_id').read_text().strip()
    marker = Path('/etc/rch-disposable-memory-vm').read_text().strip()
    validate_guest_cache(evidence, spec, boot_id, marker)
    return evidence


def kv(path):
    return {key: int(value) for key, value in (line.split() for line in path.read_text().splitlines())}


def group_for(pid):
    relative = Path(f"/proc/{pid}/cgroup").read_text().strip().split(":", 2)[2]
    return Path("/sys/fs/cgroup") / relative.lstrip("/")


def limits(group):
    names = ("memory.high", "memory.max", "memory.swap.max", "cpu.max", "cpuset.cpus.effective")
    result = {}
    current = group
    while True:
        result[str(current)] = {name: (current / name).read_text().strip() for name in names if (current / name).exists()}
        if current == Path("/sys/fs/cgroup"):
            break
        current = current.parent
    return result


def verify_limits(group, expected):
    all_limits = limits(group)
    leaf = all_limits[str(group)]
    assert leaf["memory.high"] == str(expected["memory_high_bytes"])
    assert leaf["memory.max"] == str(expected["memory_max_bytes"])
    assert leaf["memory.swap.max"] == str(expected["swap_max_bytes"])
    quota, period = leaf["cpu.max"].split()
    assert quota != "max" and int(quota) / int(period) == expected["cpu_quota"]
    for item in all_limits.values():
        for name, limit in (("memory.high", expected["memory_high_bytes"]), ("memory.max", expected["memory_max_bytes"])):
            if name in item and item[name] != "max":
                assert int(item[name]) >= limit, (name, item[name])
        if "cpu.max" in item:
            quota, period = item["cpu.max"].split()
            if quota != "max":
                assert int(quota) / int(period) >= expected["cpu_quota"]
    return all_limits


def process_stats(pid, heavy):
    root = Path(f"/proc/{pid}")
    stat = (root / "stat").read_text()
    fields = stat[stat.rfind(")") + 2:].split()
    result = {"pid": pid, "start_ticks": int(fields[19]), "cpu_ticks": int(fields[11]) + int(fields[12])}
    if heavy:
        for name in ("status", "smaps_rollup", "io"):
            result[name] = (root / name).read_text()
    return result


def sample_generator(pid, group, heavy):
    if not Path(f"/proc/{pid}").exists():
        return None
    try:
        generator_group = group_for(pid)
        if generator_group == group:
            raise RuntimeError("generator is inside the subject cgroup")
        generator_pids = [int(value) for value in (generator_group / "cgroup.procs").read_text().split()]
        result = {"cgroup": str(generator_group),
                  "memory_current": int((generator_group / "memory.current").read_text()),
                  "memory_events": kv(generator_group / "memory.events"),
                  "cpu_stat": kv(generator_group / "cpu.stat"), "processes": [], "disappeared_pids": []}
        for member in generator_pids:
            try:
                result["processes"].append(process_stats(member, heavy))
            except OSError as error:
                if error.errno not in (errno.ENOENT, errno.ESRCH):
                    raise
                try:
                    stat = Path(f"/proc/{member}/stat").read_text()
                except FileNotFoundError:
                    result["disappeared_pids"].append(member)
                else:
                    if stat[stat.rfind(")") + 2:].split()[0] != "Z":
                        raise
                    result["disappeared_pids"].append(member)
        return result
    except FileNotFoundError:
        return None


def sample(group, generator_pid, heavy):
    result = {"monotonic_ns": time.monotonic_ns(),
              "utc": datetime.datetime.now(datetime.timezone.utc).isoformat(), "cgroup": {}}
    for name in ("memory.current", "memory.peak", "memory.swap.current"):
        if (group / name).exists():
            result["cgroup"][name] = int((group / name).read_text().strip())
    for name in ("memory.stat", "memory.events", "cpu.stat"):
        result["cgroup"][name] = kv(group / name)
    for name in ("memory.pressure", "io.pressure", "io.stat"):
        if (group / name).exists():
            result["cgroup"][name] = (group / name).read_text().strip()
    result["host_memory_pressure"] = Path("/proc/pressure/memory").read_text().strip()
    result["host_cpu_pressure_started_ns"] = time.monotonic_ns()
    result["host_cpu_pressure"] = Path("/proc/pressure/cpu").read_text().strip()
    result["host_cpu_pressure_completed_ns"] = time.monotonic_ns()
    result["host_io_pressure"] = Path("/proc/pressure/io").read_text().strip()
    result["namespace_network"] = Path("/proc/net/dev").read_text()
    pids = [int(value) for value in (group / "cgroup.procs").read_text().split()]
    if not pids:
        raise RuntimeError("subject cgroup contains no process")
    result["processes"] = [process_stats(pid, heavy) for pid in pids]
    if generator_pid is not None and Path(f"/proc/{generator_pid}").exists():
        generator = sample_generator(generator_pid, group, heavy)
        if generator is None:
            result["generator_exited_during_sample"] = True
        else:
            result["generator"] = generator
    return result


def capture_subject_sample(group, generator_pid, heavy, daemon, manifest):
    try:
        return sample(group, generator_pid, heavy)
    except Exception as error:
        if daemon.poll() is None:
            raise
        manifest["subject_unexpected_exit"] = True
        manifest["subject_sample_error"] = f"{type(error).__name__}: {error}"
        try:
            manifest["subject_final_memory_events"] = kv(group / "memory.events")
        except OSError as event_error:
            manifest["subject_final_memory_events_error"] = str(event_error)
        missing = str(error.filename) if isinstance(error, FileNotFoundError) and error.filename else ""
        expected_disappearance = (missing.startswith(str(group) + "/") or missing.startswith(f"/proc/{daemon.pid}/")
                                  or str(error) == "subject cgroup contains no process")
        if not expected_disappearance:
            manifest["collector_errors"].append(manifest["subject_sample_error"])
        raise SubjectFailure(f"subject exited during sampling: {daemon.returncode}") from error


def storage_stats(subject_dir):
    files, total = 0, 0
    for directory, _, names in os.walk(subject_dir):
        for name in names:
            path = Path(directory) / name
            try:
                if path.is_file():
                    files += 1
                    total += path.stat().st_size
            except FileNotFoundError:
                pass
    result = {"subject_files": files, "subject_file_bytes": total}
    for name in ("reticulum.db", "reticulum.db-wal"):
        path = subject_dir / name
        if path.exists():
            result[name + "_bytes"] = path.stat().st_size
    messages = subject_dir / "lxmd/storage/lxmf/messagestore"
    if messages.exists():
        result["python_message_files"] = sum(1 for path in messages.iterdir() if path.is_file())
    return result


class AppendOnlyLines:
    def __init__(self, path):
        self.path = path
        self.offset = 0
        self.fragment = b""
        self.identity = None
        self.boundary = b""

    def read(self, final=False):
        if not self.path.exists():
            if self.identity is not None:
                raise RuntimeError(f"file disappeared: {self.path}")
            return []
        with self.path.open("rb") as source:
            stat = os.fstat(source.fileno())
            identity = (stat.st_dev, stat.st_ino)
            if self.identity is not None and identity != self.identity:
                raise RuntimeError(f"file was replaced: {self.path}")
            self.identity = identity
            if stat.st_size < self.offset:
                raise RuntimeError(f"file was truncated: {self.path}")
            source.seek(self.offset - len(self.boundary))
            if source.read(len(self.boundary)) != self.boundary:
                raise RuntimeError(f"file was truncated or rewritten: {self.path}")
            source.seek(self.offset)
            new = source.read()
            self.offset = source.tell()
            self.boundary = (self.boundary + new)[-4096:]
            data = self.fragment + new
        *lines, self.fragment = data.split(b"\n")
        if len(self.fragment) > 1_048_576:
            raise RuntimeError(f"unterminated line exceeds collector limit: {self.path}")
        if final and self.fragment:
            lines.append(self.fragment)
            self.fragment = b""
        return lines


class LedgerCounts:
    def __init__(self, run):
        self.readers = {role: AppendOnlyLines(run / f"{role}.jsonl") for role in ("sender", "receiver")}
        self.counts = {"attempted": 0, "accepted": 0, "received": 0}
        self.ready = {}
        self.timeouts = {}

    def snapshot(self):
        for role, reader in self.readers.items():
            lines = reader.read()
            for line in lines:
                if not line.strip():
                    continue
                row = json.loads(line)
                if row["event"] == "node_ready":
                    if role in self.ready:
                        raise RuntimeError(f"{role} recorded node readiness more than once")
                    self.ready[role] = row["monotonic_ns"]
                elif row["event"] == "node_timeout":
                    if role in self.timeouts:
                        raise RuntimeError(f"{role} recorded node timeout more than once")
                    self.timeouts[role] = row["monotonic_ns"]
                elif role == "sender" and row["event"] == "attempted":
                    self.counts["attempted"] += 1
                elif role == "sender" and row["event"] == "sender_final" and row["state"] in (4, 8):
                    self.counts["accepted"] += 1
                elif role == "receiver" and row["event"] == "received":
                    self.counts["received"] += 1
        delta = self.counts["accepted"] - self.counts["received"]
        return {**self.counts, "accepted_minus_received": max(0, delta), "accepted_minus_received_signed": delta}


def make_config(root, subject):
    (root / "subject" / "home").mkdir(parents=True)
    for role in ("sender", "receiver"):
        config = root / role / "rns" / "config"
        config.parent.mkdir(parents=True)
        config.write_text("[reticulum]\nenable_transport = no\nshare_instance = no\ndiscover_interfaces = no\nautoconnect_discovered_interfaces = 0\n[logging]\nloglevel = 2\n[interfaces]\n  [[Local test]]\n    type = TCPClientInterface\n    enabled = yes\n    target_host = 127.0.0.1\n    target_port = 42428\n")
    if subject == "python":
        rns = root / "subject" / "rns" / "config"
        lxmd = root / "subject" / "lxmd" / "config"
        rns.parent.mkdir()
        lxmd.parent.mkdir()
        rns.write_text("[reticulum]\nenable_transport = yes\nshare_instance = no\ndiscover_interfaces = no\nautoconnect_discovered_interfaces = 0\n[logging]\nloglevel = 2\n[interfaces]\n  [[Local test]]\n    type = TCPServerInterface\n    enabled = yes\n    listen_ip = 127.0.0.1\n    listen_port = 42428\n")
        lxmd.write_text("[propagation]\nenable_node = yes\nannounce_at_start = yes\nautopeer = yes\nautopeer_maxdepth = 6\nmessage_storage_limit = 500\npropagation_transfer_max_accepted_size = 256\npropagation_sync_max_accepted_size = 10240\npropagation_stamp_cost_target = 16\npropagation_stamp_cost_flexibility = 3\n[lxmf]\ndisplay_name = Local benchmark node\nannounce_at_start = no\n[logging]\nloglevel = 2\n")
        return root / "subject" / "lxmd" / "identity"
    config = root / "subject" / "reticulum.toml"
    config.write_text("[reticulum]\nenable_transport = true\nshare_instance = false\n[propagation_node]\nenabled = true\nnode_announce_at_start = true\npeer_announce_at_start = false\ntransfer_limit_kb = 256\nsync_limit_kb = 10240\nstamp_cost = 16\nstamp_cost_flexibility = 3\nmessage_storage_limit_mb = 500\n")
    return root / "subject" / "identity"


def stop(process):
    if process is None or process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=8)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)


def stop_scope(group):
    if group is None or not group.exists():
        return True
    if not group.name.startswith("run-p") or not group.name.endswith(".scope"):
        raise RuntimeError(f"refusing to stop non-test cgroup {group}")
    def members():
        try:
            return (group / "cgroup.procs").read_text().split()
        except OSError as error:
            if error.errno in (errno.ENOENT, errno.ENODEV):
                return []
            raise
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for raw in members():
            try:
                os.kill(int(raw), sig)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if not members():
                return True
            time.sleep(0.1)
    return not members()


def interrupt(signum, _frame):
    raise InterruptedError(f"received signal {signum}")


def main():
    spec_path, subject, run_id = Path(sys.argv[1]).resolve(), sys.argv[2], sys.argv[3]
    if subject not in ("python", "rust") or not re.fullmatch(r"[a-zA-Z0-9_-]+", run_id):
        raise SystemExit("usage: run.sh EXPERIMENT.json python|rust RUN_ID")
    spec = json.loads(spec_path.read_text())
    check_spec(spec)
    normal = spec["profile"] in ("normal", "high-load", "preflight-only-normal-dry-run")
    if spec["profile"] in ("smoke", "idle", "offline", "normal", "high-load", "preflight-only-normal-dry-run") or spec.get("guest_cold_cache"):
        required = {"run.sh", "run.py", "launch.py", "peer.py", "watch_scope.py", "validate.py"}
        if normal:
            required.add("peer_guard.py")
        if spec.get("generator_mode") == "prepared_packets":
            required.update(("prepare_packet_load.py", "packet_sender.py"))
        if set(spec["harness_sha256"]) != required:
            raise SystemExit("frozen harness inventory is incomplete")
        for name, expected in spec["harness_sha256"].items():
            if sha256_file(HERE / name) != expected:
                raise SystemExit(f"frozen harness changed: {name}")
        for name in ("python", "rust"):
            if sha256_file(spec[name + "_executable"]) != spec[name + "_sha256"]:
                raise SystemExit(f"frozen {name} executable changed")
        if sha256_tree(spec["python_site_packages"]) != spec["python_site_packages_sha256"]:
            raise SystemExit("frozen Python dependencies changed")
        if spec.get("guest_cold_cache"):
            if sha256_file(spec["subject_python_executable"]) != spec["subject_python_sha256"]:
                raise SystemExit("frozen private subject Python changed")
            runtime = spec["subject_runtime_trees_sha256"]
            roots = [Path(root).resolve() for root in runtime]
            required = [Path(spec[key]).resolve() for key in ("subject_python_executable", "subject_lib_path", "subject_locale_path", "subject_site_packages")]
            if not runtime or any(not any(path == root or root in path.parents for root in roots) for path in required):
                raise SystemExit("private runtime hash inventory is incomplete")
            for root, expected in runtime.items():
                if sha256_tree(root) != expected:
                    raise SystemExit(f"frozen private subject runtime changed: {root}")
            required_config = str(Path(spec["subject_python_executable"]).parent.parent / "pyvenv.cfg")
            if set(spec["subject_runtime_files_sha256"]) != {required_config}:
                raise SystemExit("private runtime file hash inventory is incomplete")
            for path, expected in spec["subject_runtime_files_sha256"].items():
                if sha256_file(path) != expected:
                    raise SystemExit(f"frozen private subject runtime file changed: {path}")
            for path, key in (("/usr/local/sbin/rch-daemon-memory-cold-cache", "guest_cache_helper_sha256"),
                              ("/usr/local/lib/rch-daemon-memory/cache-residency-audit.py", "guest_cache_auditor_sha256")):
                if sha256_file(path) != spec[key]:
                    raise SystemExit(f"frozen guest cache control changed: {path}")
    lock = (Path(os.environ["XDG_RUNTIME_DIR"]) / "rch-daemon-memory-bench.lock").open("a")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise SystemExit("another daemon comparison run holds the host lock")
    signal.signal(signal.SIGTERM, interrupt)
    signal.signal(signal.SIGINT, interrupt)
    experiment_root = spec_path.parent
    run = experiment_root / "runs" / run_id
    run.mkdir(parents=True, exist_ok=False)
    counts = LedgerCounts(run)
    log_lines = AppendOnlyLines(run / "subject.log")
    shutil.copyfile(spec_path, run / "experiment.json")
    shutil.copytree(experiment_root / "fixture", run / "fixture")
    for name, expected in spec["fixture_sha256"].items():
        assert hashlib.sha256((run / "fixture" / name).read_bytes()).hexdigest() == expected
    identity = make_config(run, subject)
    manifest = {"subject": subject, "run_id": run_id, "collector_errors": [], "finalized": False,
                "isolation_verified": False, "limits_verified": False,
                "generator_telemetry_contract": 2 if normal else 1,
                "sender_started": False, "receiver_started": False,
                "subject_unexpected_exit": False, "subject_log_error_count": 0,
                "subject_log_error_examples": [], "phase_times_ns": {}}

    def collect_subject_log(final=False):
        for line in log_lines.read(final=final):
            message = re.sub(r"\x1b\[[0-9;]*m", "", line.decode(errors="replace"))
            if subject_log_is_error(message):
                manifest["subject_log_error_count"] += 1
                if len(manifest["subject_log_error_examples"]) < 20:
                    manifest["subject_log_error_examples"].append(message[:512])

    def save_manifest():
        pending = run / "manifest.json.tmp"
        pending.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        pending.replace(run / "manifest.json")

    save_manifest()
    processes = []
    peer_groups = []
    watchers = []
    normal_peers = {}
    peak_file = None
    daemon = peer = group = None

    def collect_normal_exit(role, process):
        if role + "_exit" in manifest:
            return True
        result_path = run / f"{role}-peer-result.json"
        if not result_path.exists():
            if process.poll() is not None:
                raise RuntimeError(f"{role} guard exited without its result")
            return False
        if process.poll() is not None:
            raise RuntimeError(f"{role} guard did not retain its scope")
        result = json.loads(result_path.read_text())
        start = json.loads((run / f"{role}-peer-start.json").read_text())
        if (type(result.get("pid")) is not int or type(result.get("start_ticks")) is not int
                or type(result.get("exit_code")) is not int or type(result.get("cancelled")) is not bool
                or result.get("remaining_pids") != []
                or start != {"pid": result["pid"], "start_ticks": result["start_ticks"]}):
            raise RuntimeError(f"{role} guard result was invalid or left processes running")
        scope = Path(manifest[role + "_cgroup"])
        events, cpu = kv(scope / "memory.events"), kv(scope / "cpu.stat")
        if not all(key in events for key in ("oom", "oom_kill")) or not all(key in cpu for key in ("usage_usec", "nr_throttled")):
            raise RuntimeError(f"{role} final scope counters were incomplete")
        manifest.update({role + "_exit": result["exit_code"], role + "_peer_pid": result["pid"],
                         role + "_peer_start_ticks": result["start_ticks"], role + "_cancelled": result["cancelled"],
                         role + "_final_memory_events": events, role + "_final_cpu_stat": cpu,
                         role + "_exit_observed_ns": time.monotonic_ns()})
        if manifest.get("normal_readiness_failed") and result["cancelled"]:
            manifest[role + "_stopped_after_readiness_failure"] = True
        save_manifest()
        (run / f"{role}-ack").touch()
        return True

    try:
        command("ip", "link", "set", "lo", "up")
        links = json.loads(command("ip", "-j", "link"))
        routes4 = json.loads(command("ip", "-j", "-4", "route"))
        routes6 = json.loads(command("ip", "-j", "-6", "route"))
        assert [link["ifname"] for link in links] == ["lo"] and not routes4 and not routes6
        manifest["initial_headroom"] = check_headroom(run, spec)
        manifest["network"] = {"links": links, "ipv4_routes": routes4, "ipv6_routes": routes6}
        manifest["isolation_verified"] = True
        py = spec["python_executable"]
        subject_dir = run / "subject"
        if spec.get("generator_mode") == "prepared_packets":
            with (run / "preparation.log").open("w") as preparation_log:
                subprocess.run([py, "-u", str(HERE / "prepare_packet_load.py"), str(run)],
                               stdout=preparation_log, stderr=subprocess.STDOUT, check=True,
                               timeout=spec["preparation_deadline_seconds"])
            preparation = json.loads((run / "prepared-packets-metadata.json").read_text())
            if (preparation.get("message_count") != spec["message_count"]
                    or preparation.get("stamp_target") != spec["propagation_stamp_target"]
                    or preparation.get("packets_sha256") != sha256_file(run / "prepared-packets.jsonl")):
                raise RuntimeError("prepared packet metadata differs from the frozen workload")
            manifest["packet_preparation"] = preparation
            save_manifest()
        private_python = spec.get("subject_python_executable") if spec.get("guest_cold_cache") else None
        if subject == "python":
            target = [private_python or py, "-u", "-m", "LXMF.Utilities.lxmd", "--config", str(subject_dir / "lxmd"), "--rnsconfig", str(subject_dir / "rns"), "--propagation-node"]
            if private_python:
                target = ["/usr/bin/env", f"LD_LIBRARY_PATH={spec['subject_lib_path']}", f"LOCPATH={spec['subject_locale_path']}", *target]
        else:
            target = [spec["rust_executable"], "--config", str(subject_dir / "reticulum.toml"), "--db", str(subject_dir / "reticulum.db"), "--identity", str(identity), "--rpc", "127.0.0.1:42429", "--rpc-unix", "rpc.sock", "--transport", "127.0.0.1:42428"]
        launch = ["systemd-run", "--user", "--scope", "--quiet", "-p", "MemoryAccounting=yes", "-p", "MemoryHigh=768M", "-p", "MemoryMax=1G", "-p", "MemorySwapMax=0", "-p", "CPUQuota=200%", "env", "-i", "PATH=/usr/bin:/bin", "LANG=C.UTF-8", f"HOME={subject_dir / 'home'}", "PYTHONNOUSERSITE=1", "PYTHONDONTWRITEBYTECODE=1", "RUST_LOG=warn", "TOKIO_WORKER_THREADS=2", sys.executable, str(HERE / "launch.py"), str(os.getpid()), str(run / "fixture" / "node.key"), str(identity), *target]
        if spec.get("guest_cold_cache"):
            cache_evidence = request_guest_cache(run_id, run, spec)
            manifest["guest_cache_control"] = cache_evidence
            save_manifest()
        manifest["subject_command"] = target
        manifest["phase_times_ns"]["launch_requested"] = time.monotonic_ns()
        daemon = subprocess.Popen(launch, cwd=subject_dir, stdout=(run / "subject.log").open("w"), stderr=subprocess.STDOUT, start_new_session=True)
        processes.append(daemon)
        for _ in range(100):
            if daemon.poll() is not None:
                raise SubjectFailure(f"subject exited during startup: {daemon.returncode}")
            candidate = group_for(daemon.pid)
            if candidate != group_for(os.getpid()) and candidate.name.startswith("run-p") and candidate.name.endswith(".scope"):
                group = candidate
                break
            time.sleep(0.1)
        else:
            raise RuntimeError("subject scope was not created")
        watchers.append(subprocess.Popen([sys.executable, str(HERE / "watch_scope.py"), str(os.getpid()), str(group)], start_new_session=True, pass_fds=(lock.fileno(),)))
        manifest["cgroup"] = str(group)
        manifest["effective_limits"] = verify_limits(group, spec["limits"])
        manifest["limits_verified"] = True
        manifest["subject_pid"] = daemon.pid
        manifest["subject_start_ticks"] = process_stats(daemon.pid, False)["start_ticks"]
        manifest["sampling_start_ns"] = time.monotonic_ns()
        manifest["phase_times_ns"]["startup"] = manifest["sampling_start_ns"]
        phase = "startup"
        phase_start = time.monotonic()
        next_sample = phase_start
        sample_number = 0
        peer_started = None
        with (run / "samples.jsonl").open("w", buffering=1) as output:
            def start_peer(role):
                peer_program = "peer_guard.py" if normal else "peer.py"
                launch_peer = ["systemd-run", "--user", "--scope", "--quiet", "-p", "MemoryAccounting=yes", "-p", "MemoryHigh=768M", "-p", "MemoryMax=1G", "-p", "MemorySwapMax=0", "-p", "CPUQuota=1200%", "-p", "TasksMax=128", "env", "-i", "PATH=/usr/bin:/bin", "LANG=C.UTF-8", f"HOME={run / role}", "PYTHONNOUSERSITE=1", "PYTHONDONTWRITEBYTECODE=1", py, "-u", str(HERE / peer_program), str(run), role, str(os.getpid())]
                process = subprocess.Popen(launch_peer, cwd=run, stdout=(run / (role + ".log")).open("w"), stderr=subprocess.STDOUT, start_new_session=True)
                processes.append(process)
                manifest[role + "_started"] = True
                manifest[role + "_pid"] = process.pid
                save_manifest()
                for _ in range(50):
                    if process.poll() is not None:
                        raise RuntimeError(f"{role} exited before its scope was verified")
                    candidate = group_for(process.pid)
                    if candidate != group and candidate != group_for(os.getpid()) and candidate.name.startswith("run-p") and candidate.name.endswith(".scope"):
                        if candidate in peer_groups:
                            raise RuntimeError("peer scopes are not distinct")
                        peer_groups.append(candidate)
                        watchers.append(subprocess.Popen([sys.executable, str(HERE / "watch_scope.py"), str(os.getpid()), str(candidate)], start_new_session=True, pass_fds=(lock.fileno(),)))
                        expected = {**spec["limits"], "cpu_quota": 12}
                        manifest[role + "_effective_limits"] = verify_limits(candidate, expected)
                        manifest[role + "_cgroup"] = str(candidate)
                        manifest[role + "_start_ticks"] = process_stats(process.pid, False)["start_ticks"]
                        manifest[role + "_scope_verified_ns"] = time.monotonic_ns()
                        save_manifest()
                        return process
                    time.sleep(0.1)
                raise RuntimeError(f"{role} cgroup was not created")

            def boundary_sample(phase_name):
                row = capture_subject_sample(group, None, True, daemon, manifest)
                row["phase"] = phase_name
                row["sample_kind"] = "boundary"
                row["headroom"] = check_headroom(run, spec)
                row["ledger_counts"] = counts.snapshot()
                row["host_swap_counters"] = {key: value for key, value in kv(Path("/proc/vmstat")).items() if key in ("pswpin", "pswpout")}
                row["storage"] = storage_stats(subject_dir)
                if peak_file is not None:
                    peak_file.seek(0)
                    row["cgroup"]["memory.peak_since_ready"] = int(peak_file.read().strip())
                output.write(json.dumps(row, sort_keys=True) + "\n")
                if daemon.poll() is not None:
                    raise SubjectFailure(f"subject exited at a phase boundary: {daemon.returncode}")
                return row["monotonic_ns"]

            while True:
                now = time.monotonic()
                if daemon.poll() is not None:
                    manifest["subject_unexpected_exit"] = True
                    raise SubjectFailure(f"subject exited unexpectedly: {daemon.returncode}")
                collect_subject_log()
                if now >= next_sample:
                    try:
                        heavy = sample_number % 12 == 0
                        if normal:
                            row = capture_subject_sample(group, None, heavy, daemon, manifest)
                            row["generators"] = {}
                            for role, process in normal_peers.items():
                                active_at_start = process.poll() is None
                                generator = sample_generator(process.pid, group, heavy) if active_at_start else None
                                row["generators"][role] = {"active_at_start": active_at_start,
                                                           "active_at_end": process.poll() is None,
                                                           "scope": generator}
                        else:
                            active_at_start = bool(peer and peer.poll() is None)
                            row = capture_subject_sample(group, peer.pid if active_at_start else None, heavy, daemon, manifest)
                            row["generator_active_at_start"] = active_at_start
                            row["generator_active_at_end"] = bool(peer and peer.poll() is None)
                        row["sample_kind"] = "periodic"
                        row["headroom"] = check_headroom(run, spec)
                        row["ledger_counts"] = counts.snapshot()
                        row["host_swap_counters"] = {key: value for key, value in kv(Path("/proc/vmstat")).items() if key in ("pswpin", "pswpout")}
                        if heavy:
                            row["storage"] = storage_stats(subject_dir)
                        if peak_file is not None:
                            peak_file.seek(0)
                            row["cgroup"]["memory.peak_since_ready"] = int(peak_file.read().strip())
                    except SubjectFailure:
                        raise
                    except Exception as error:
                        manifest["collector_errors"].append(f"{type(error).__name__}: {error}")
                        raise
                    row["phase"] = phase
                    output.write(json.dumps(row, sort_keys=True) + "\n")
                    sample_number += 1
                    next_sample += spec["sample_interval_seconds"]
                    save_manifest()
                if phase == "startup":
                    listeners = command("ss", "-lnt")
                    endpoints = [line.split()[3] for line in listeners.splitlines()[1:]]
                    required = {"127.0.0.1:42428"} | ({"127.0.0.1:42429"} if subject == "rust" else set())
                    if set(endpoints) == required and identity.exists() and (subject != "rust" or (subject_dir / "rpc.sock").is_socket()):
                        udp_listeners = command("ss", "-lun")
                        if len(udp_listeners.splitlines()) != 1:
                            raise RuntimeError("unexpected UDP listener in the test namespace")
                        manifest["listeners"] = listeners
                        manifest["udp_listeners"] = udp_listeners
                        manifest["unix_listeners"] = command("ss", "-lx")
                        peak_file = (group / "memory.peak").open("r+")
                        peak_file.write("1")
                        peak_file.flush()
                        manifest["phase_times_ns"]["memory_peak_reset"] = time.monotonic_ns()
                        phase = "warmup"
                        manifest["phase_times_ns"][phase] = time.monotonic_ns()
                        phase_start = manifest["phase_times_ns"][phase] / 1e9
                    elif now - phase_start > 30:
                        raise RuntimeError(f"subject startup readiness timed out; listeners={endpoints}")
                elif phase == "warmup" and now - phase_start >= spec["warmup_seconds"]:
                    if spec["profile"] in ("idle", "preflight-only-idle-dry-run"):
                        phase = "idle"
                        idle_start_ns = boundary_sample(phase)
                        phase_start = idle_start_ns / 1e9
                        manifest["phase_times_ns"][phase] = idle_start_ns
                    elif normal:
                        phase = "normal_ready"
                        phase_start = time.monotonic()
                        manifest["phase_times_ns"][phase] = time.monotonic_ns()
                        normal_peers["receiver"] = start_peer("receiver")
                        normal_peers["sender"] = start_peer("sender")
                    else:
                        phase = "sender"
                        manifest["phase_times_ns"][phase] = time.monotonic_ns()
                        peer = start_peer("sender")
                        peer_started = now
                elif phase == "idle" and now - phase_start >= spec["duration_seconds"]:
                    boundary_sample(phase)
                    break
                elif phase == "normal_ready":
                    counts.snapshot()
                    failed = [role for role, process in normal_peers.items() if collect_normal_exit(role, process)]
                    if not failed and now - phase_start > spec["route_deadline_seconds"] * 2 + 20:
                        raise RuntimeError("normal peer readiness controller timed out")
                    if failed:
                        counts.snapshot()
                        if any(manifest.get(role + "_exit") != 71 or role not in counts.timeouts or role in counts.ready for role in failed):
                            raise RuntimeError("normal peer exited unexpectedly before readiness")
                        manifest["normal_readiness_failed"] = True
                        manifest["normal_readiness_failed_roles"] = failed
                        for role in normal_peers:
                            if role + "_exit" not in manifest:
                                (run / f"{role}-cancel").touch()
                        phase = "normal_cancel"
                        phase_start = time.monotonic()
                        save_manifest()
                    elif set(counts.ready) == {"sender", "receiver"}:
                        start_ns = time.monotonic_ns() + 5_000_000_000
                        pending = run / "normal-start.json.tmp"
                        pending.write_text(json.dumps({"start_monotonic_ns": start_ns}) + "\n")
                        pending.replace(run / "normal-start.json")
                        manifest["phase_times_ns"]["normal"] = start_ns
                        phase = "normal"
                        save_manifest()
                elif phase == "normal_cancel":
                    for role, process in normal_peers.items():
                        collect_normal_exit(role, process)
                    if all(role + "_exit" in manifest and process.poll() is not None for role, process in normal_peers.items()):
                        if any(manifest[role + "_exit"] == 70 for role in normal_peers):
                            raise RuntimeError("normal traffic peer reported a ledger capture failure during cancellation")
                        break
                    if now - phase_start > 30:
                        raise RuntimeError("normal traffic peer cancellation did not finish")
                elif phase == "normal":
                    for role, process in normal_peers.items():
                        if collect_normal_exit(role, process):
                            if manifest[role + "_exit"] != 0:
                                raise RuntimeError(f"{role} exited unexpectedly during normal traffic")
                    if all(role + "_exit" in manifest and process.poll() is not None for role, process in normal_peers.items()):
                        for peer_group in peer_groups:
                            try:
                                members = (peer_group / "cgroup.procs").read_text().split()
                            except FileNotFoundError:
                                members = []
                            if members:
                                raise RuntimeError("normal traffic peer scope remained populated before recovery")
                        manifest["phase_times_ns"]["normal_end"] = boundary_sample(phase)
                        phase = "recovery"
                        recovery_start_ns = boundary_sample(phase)
                        phase_start = recovery_start_ns / 1e9
                        manifest["phase_times_ns"][phase] = recovery_start_ns
                    elif now > (manifest["phase_times_ns"]["normal"] / 1e9 + spec["duration_seconds"]
                                + max(spec["sender_drain_seconds"], spec["receiver_drain_seconds"]) + 45):
                        raise RuntimeError("normal traffic exceeded its deadline")
                elif phase == "sender" and peer.poll() is not None:
                    manifest["sender_exit_observed_ns"] = time.monotonic_ns()
                    manifest["sender_exit"] = peer.returncode
                    if peer.returncode == 70:
                        raise RuntimeError("sender reported a ledger capture failure")
                    manifest["phase_times_ns"]["sender_end"] = boundary_sample(phase)
                    phase = "receiver"
                    manifest["phase_times_ns"][phase] = time.monotonic_ns()
                    peer = start_peer("receiver")
                    peer_started = now
                elif phase == "receiver" and peer.poll() is not None:
                    manifest["receiver_exit_observed_ns"] = time.monotonic_ns()
                    manifest["receiver_exit"] = peer.returncode
                    if peer.returncode == 70:
                        raise RuntimeError("receiver reported a ledger capture failure")
                    if spec.get("recovery_seconds", 0):
                        peer = None
                        peer_started = None
                        phase = "recovery"
                        recovery_start_ns = boundary_sample(phase)
                        phase_start = recovery_start_ns / 1e9
                        manifest["phase_times_ns"][phase] = recovery_start_ns
                    else:
                        boundary_sample(phase)
                        break
                elif phase == "recovery" and now - phase_start >= spec["recovery_seconds"]:
                    boundary_sample(phase)
                    break
                if peer_started is not None:
                    maximum = spec["route_deadline_seconds"] + (spec["duration_seconds"] + spec["sender_drain_seconds"] if phase == "sender" else spec["receiver_drain_seconds"]) + 20
                    if now - peer_started > maximum:
                        raise RuntimeError(f"{phase} exceeded its deadline")
                time.sleep(0.2)
        manifest["sampling_end_ns"] = time.monotonic_ns()
    except SubjectFailure as error:
        manifest["subject_unexpected_exit"] = True
        manifest["subject_failure"] = str(error)
        if group is not None and "subject_final_memory_events" not in manifest:
            try:
                manifest["subject_final_memory_events"] = kv(group / "memory.events")
            except OSError as event_error:
                manifest["subject_final_memory_events_error"] = str(event_error)
        manifest.setdefault("sampling_start_ns", time.monotonic_ns())
        manifest["sampling_end_ns"] = time.monotonic_ns()
        if normal and normal_peers:
            try:
                for role in normal_peers:
                    if role + "_exit" not in manifest:
                        (run / f"{role}-cancel").touch()
                deadline = time.monotonic() + 30
                while not all(collect_normal_exit(role, process) for role, process in normal_peers.items()):
                    if time.monotonic() >= deadline:
                        raise TimeoutError("normal peer cancellation after subject failure timed out")
                    time.sleep(0.1)
            except Exception as cleanup_error:
                manifest["collector_errors"].append(f"normal peer evidence after subject failure: {type(cleanup_error).__name__}: {cleanup_error}")
    except Exception as error:
        manifest["collector_errors"].append(f"{type(error).__name__}: {error}")
        manifest.setdefault("sampling_start_ns", time.monotonic_ns())
        manifest["sampling_end_ns"] = time.monotonic_ns()
    finally:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        if peak_file is not None:
            try:
                peak_file.close()
            except OSError as error:
                manifest["collector_errors"].append(f"peak counter close failed: {error}")
        for process in reversed(processes):
            stop(process)
        manifest["peer_scopes_empty"] = all([stop_scope(peer_group) for peer_group in peer_groups])
        manifest["subject_scope_empty"] = stop_scope(group)
        try:
            collect_subject_log(final=True)
        except Exception as error:
            manifest["collector_errors"].append(f"subject log collection failed: {type(error).__name__}: {error}")
        for watcher in watchers:
            stop(watcher)
        try:
            manifest["listeners_after_stop"] = command("ss", "-lnt")
        except subprocess.CalledProcessError as error:
            manifest["collector_errors"].append(f"listener cleanup verification failed: {error}")
            manifest["listeners_after_stop"] = "unavailable"
        if not manifest["subject_scope_empty"] or not manifest["peer_scopes_empty"] or len(manifest["listeners_after_stop"].splitlines()) != 1 or manifest["listeners_after_stop"] == "unavailable":
            manifest["collector_errors"].append("subject scope or namespace listeners remained after cleanup")
        manifest["phase_times_ns"]["stopped"] = time.monotonic_ns()
        manifest["finalized"] = True
        save_manifest()
    result = subprocess.run([sys.executable, str(HERE / "validate.py"), str(run)])
    print(f"run artifacts: {run}", flush=True)
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
