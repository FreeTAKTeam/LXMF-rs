#!/usr/bin/env python3
import os
import shutil
import signal
import subprocess
import sys
from pathlib import Path


SUPPORTED_CASES = {
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
}

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
}


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

    env = os.environ.copy()
    env["COMPAT_CASE"] = case_id
    env.setdefault("LXMF_PYTHON_BIN", sys.executable)
    env.setdefault("PYTHON_BIN", env["LXMF_PYTHON_BIN"])
    env.setdefault("BASH_BIN", bash)

    creationflags = 0
    if os.name == "nt":
        creationflags = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)

    process = subprocess.Popen(
        [bash, str(smoke_script)],
        cwd=repo_root,
        env=env,
        start_new_session=os.name != "nt",
        creationflags=creationflags,
    )
    timeout = case_timeout_seconds()
    try:
        return process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        print(
            f"compatibility case {case_id!r} timed out after {timeout:g} seconds",
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


if __name__ == "__main__":
    raise SystemExit(main())
