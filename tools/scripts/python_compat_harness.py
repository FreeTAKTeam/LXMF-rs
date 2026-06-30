#!/usr/bin/env python3
import os
import shutil
import signal
import subprocess
import sys
from pathlib import Path


SMOKE_SCRIPT_CASES = {
    "direct_rust_to_python",
    "direct_python_to_rust",
    "opportunistic_python_to_rust",
    "opportunistic_rust_to_python",
    "propagated_rust_to_python",
    "propagated_python_to_rust",
    "propagation_remote_status_bidir",
    "propagation_remote_fetch_rust_to_python",
    "propagation_remote_download_rust_to_python",
    "propagation_remote_sync_rust_to_python",
    "propagation_get_haves_python_to_rust",
    "propagation_offer_python_to_rust",
    "propagation_offer_queue_python_to_rust",
    "propagation_offer_duplicate_wanted_source_completed_python_to_rust",
    "link_liveness_rust_to_python",
    "link_liveness_python_to_rust",
    "link_teardown_rust_to_python",
    "link_teardown_python_to_rust",
    "resource_transfer",
    "lxm_interchange",
    "rns_path_request_rust_to_python",
    "rns_path_request_rust_to_python_scoped_refresh",
    "rns_path_request_python_to_rust",
}

LOCAL_CARGO_TEST_CASES = {
    "rns_path_request_transport_policy": [
        "cargo",
        "test",
        "-p",
        "reticulumd",
        "--test",
        "transport_policy_evidence",
    ],
    "rns_path_request_roaming_transport_policy": [
        "cargo",
        "test",
        "-p",
        "reticulumd",
        "--test",
        "transport_policy_evidence",
        "roaming_same_iface_known_path_request_is_suppressed_at_transport_boundary",
    ],
    "rns_path_request_roaming_grace_transport_policy": [
        "cargo",
        "test",
        "-p",
        "reticulumd",
        "--test",
        "transport_policy_evidence",
        "roaming_diff_iface_known_path_response_waits_extra_grace_at_transport_boundary",
    ],
    "rns_announce_rebroadcast_transport_policy": [
        "cargo",
        "test",
        "-p",
        "reticulumd",
        "--test",
        "transport_policy_evidence",
        "announce_rebroadcast_policy_uses_learned_next_hop_mode_at_transport_boundary",
    ],
    "rns_unknown_announce_ingress_policy": [
        "cargo",
        "test",
        "-p",
        "reticulum-rs-transport",
        "held_announces_release_one_lowest_hop_entry_per_interface",
    ],
    "rns_link_request_mtu_transport_policy": [
        "cargo",
        "test",
        "-p",
        "reticulum-rs-transport",
        "--lib",
        "mtu_signalling",
    ],
}

SUPPORTED_CASES = SMOKE_SCRIPT_CASES | set(LOCAL_CARGO_TEST_CASES)


def resolve_bash() -> str | None:
    configured = os.environ.get("BASH_BIN")
    if configured:
        return configured

    candidates: list[str] = []
    found = shutil.which("bash")
    if found:
        candidates.append(found)

    if os.name == "nt":
        candidates.extend(
            [
                r"C:\Program Files\Git\bin\bash.exe",
                r"C:\Program Files\Git\usr\bin\bash.exe",
            ]
        )

    for candidate in candidates:
        candidate_path = Path(candidate)
        if candidate_path.name.lower() == "bash.exe" and "windows\\system32" in str(candidate_path).lower():
            continue
        if candidate_path.is_file() or shutil.which(candidate):
            return str(candidate_path)

    return None


def case_timeout_seconds() -> float:
    raw = os.environ.get("LXMF_PY_COMPAT_CASE_TIMEOUT_SECS", "420")
    try:
        timeout = float(raw)
    except ValueError:
        print(
            f"invalid LXMF_PY_COMPAT_CASE_TIMEOUT_SECS={raw!r}; expected seconds",
            file=sys.stderr,
        )
        return 420.0
    if timeout <= 0:
        print(
            f"invalid LXMF_PY_COMPAT_CASE_TIMEOUT_SECS={raw!r}; using 420 seconds",
            file=sys.stderr,
        )
        return 420.0
    return timeout


def terminate_process_tree(process: subprocess.Popen[bytes]) -> None:
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/PID", str(process.pid), "/T", "/F"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return

    try:
        os.killpg(os.getpgid(process.pid), signal.SIGTERM)
    except ProcessLookupError:
        return


def kill_process_tree(process: subprocess.Popen[bytes]) -> None:
    if os.name == "nt":
        terminate_process_tree(process)
        return

    try:
        os.killpg(os.getpgid(process.pid), signal.SIGKILL)
    except ProcessLookupError:
        return


def run_with_timeout(command: list[str], env: dict[str, str], cwd: Path, timeout: float) -> int:
    creationflags = 0
    if os.name == "nt":
        creationflags = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)

    process = subprocess.Popen(
        command,
        cwd=cwd,
        env=env,
        start_new_session=os.name != "nt",
        creationflags=creationflags,
    )
    try:
        return process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        print(
            f"compatibility command {' '.join(command)!r} timed out after {timeout:g} seconds",
            file=sys.stderr,
            flush=True,
        )
        terminate_process_tree(process)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            kill_process_tree(process)
            process.wait()
        return 124


def cargo_test_list_command(command: list[str]) -> list[str]:
    if "--" in command:
        separator = command.index("--")
        return command[:separator] + ["--", "--list"] + command[separator + 1 :]
    return command + ["--", "--list"]


def matching_test_count(list_output: bytes) -> int:
    return sum(
        1
        for line in list_output.decode(errors="replace").splitlines()
        if line.endswith(": test")
    )


def run_local_cargo_test_case(
    command: list[str], env: dict[str, str], cwd: Path, timeout: float
) -> int:
    list_command = cargo_test_list_command(command)
    try:
        completed = subprocess.run(
            list_command,
            cwd=cwd,
            env=env,
            timeout=timeout,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
    except subprocess.TimeoutExpired:
        print(
            f"compatibility command {' '.join(list_command)!r} timed out after {timeout:g} seconds",
            file=sys.stderr,
        )
        return 124

    if completed.returncode != 0:
        sys.stdout.buffer.write(completed.stdout)
        sys.stdout.buffer.flush()
        return completed.returncode

    count = matching_test_count(completed.stdout)
    if count == 0:
        print(
            f"local cargo compatibility case matched zero tests: {' '.join(command)!r}",
            file=sys.stderr,
        )
        sys.stdout.buffer.write(completed.stdout)
        sys.stdout.buffer.flush()
        return 3

    print(
        f"local cargo compatibility case matched {count} test(s): {' '.join(command)!r}",
        flush=True,
    )
    return run_with_timeout(command, env, cwd, timeout)


def main() -> int:
    supported_cases = ", ".join(sorted(SUPPORTED_CASES))
    if len(sys.argv) != 2:
        print(
            f"usage: python_compat_harness.py <case_id> (one of: {supported_cases})",
            file=sys.stderr,
        )
        return 2

    case_id = sys.argv[1]
    if case_id not in SUPPORTED_CASES:
        print(
            f"unsupported compatibility case: {case_id}. Supported cases: {supported_cases}",
            file=sys.stderr,
        )
        return 2

    repo_root = Path(__file__).resolve().parents[2]
    env = os.environ.copy()
    timeout = case_timeout_seconds()

    if case_id in LOCAL_CARGO_TEST_CASES:
        return run_local_cargo_test_case(
            LOCAL_CARGO_TEST_CASES[case_id], env, repo_root, timeout
        )

    smoke_script = repo_root / "tools" / "scripts" / "python-lxmd-rust-lxmd-smoke.sh"
    if case_id not in SMOKE_SCRIPT_CASES:
        print(
            f"compatibility case {case_id!r} is recognized but is not yet wired to a local dispatcher",
            file=sys.stderr,
        )
        return 3

    if not smoke_script.is_file():
        print(f"missing smoke script: {smoke_script}", file=sys.stderr)
        return 2
    bash = resolve_bash()
    if not bash:
        print(
            "missing usable bash. Set BASH_BIN or install Git Bash before running this harness.",
            file=sys.stderr,
        )
        return 2

    env["COMPAT_CASE"] = case_id
    env.setdefault("LXMF_PYTHON_BIN", sys.executable)
    env.setdefault("PYTHON_BIN", env["LXMF_PYTHON_BIN"])
    env.setdefault("BASH_BIN", bash)

    return run_with_timeout([bash, str(smoke_script)], env, repo_root, timeout)


if __name__ == "__main__":
    raise SystemExit(main())
