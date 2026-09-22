#!/usr/bin/env python3
"""Run the required Python/Rust compatibility cases as one evidence gate.

The existing compatibility harness deliberately runs one case at a time.  This
runner supplies the missing release-acceptance layer: it verifies the dispatch
contract, freezes the reference and candidate revisions, runs every selected
case, validates the produced evidence, and writes one machine-readable report.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import platform
import re
import shlex
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Iterable

from python_compat_harness import (
    LOCAL_CARGO_TEST_CASES,
    SMOKE_SCRIPT_CASES,
    SUPPORTED_CASES,
    resolve_bash,
    terminate_process_tree,
    kill_process_tree,
)


ROOT = Path(__file__).resolve().parents[2]
SCHEMA_VERSION = 1
EXPECTED_RETICULUM_REVISION = "99de23c040d507e3fefca19e87b182302902725d"
EXPECTED_LXMF_REVISION = "727830cefda83d9c6e3982b48675425f3f988f9c"

# Keep this list explicit.  It is the acceptance contract, while the imported
# sets are the implementation dispatch table that must continue to satisfy it.
REQUIRED_LIVE_CASES = (
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
)
REQUIRED_LOCAL_CASES = (
    "rns_path_request_transport_policy",
    "rns_path_request_roaming_transport_policy",
    "rns_path_request_roaming_grace_transport_policy",
    "rns_announce_rebroadcast_transport_policy",
    "rns_announce_rate_target_transport_policy",
    "rns_unknown_announce_ingress_policy",
    "rns_link_request_mtu_transport_policy",
)
REQUIRED_CASES = REQUIRED_LIVE_CASES + REQUIRED_LOCAL_CASES

CARGO_RESULT_RE = re.compile(
    r"test result: (?P<result>ok|FAILED)\.\s+"
    r"(?P<passed>\d+) passed;\s+"
    r"(?P<failed>\d+) failed;\s+"
    r"(?P<ignored>\d+) ignored;"
)


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )


def repo_path(path: Path) -> str:
    """Return a stable path for evidence while retaining external paths."""

    try:
        return str(path.resolve().relative_to(ROOT))
    except ValueError:
        return str(path.resolve())


def command_text(command: Iterable[str]) -> str:
    return shlex.join([str(part) for part in command])


def run_capture(
    command: list[str], cwd: Path, env: dict[str, str] | None = None, timeout: float = 30
) -> tuple[int | None, str, str | None]:
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
            timeout=timeout,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return None, "", str(error)
    return completed.returncode, completed.stdout, completed.stderr


def git_metadata(path: Path) -> dict[str, Any]:
    revision_code, revision_output, revision_error = run_capture(
        ["git", "-C", str(path), "rev-parse", "HEAD"], ROOT
    )
    status_code, status_output, status_error = run_capture(
        ["git", "-C", str(path), "status", "--porcelain", "--untracked-files=all"], ROOT
    )
    revision = revision_output.strip() if revision_code == 0 else None
    status_lines = status_output.splitlines() if status_code == 0 else []
    return {
        "path": repo_path(path),
        "revision": revision,
        "clean": status_code == 0 and not status_lines,
        "status": status_lines,
        "revision_error": revision_error,
        "status_error": status_error,
    }


def resolve_reference_path(
    configured: str | None, variable: str, candidates: tuple[Path, ...]
) -> Path:
    if configured:
        return Path(configured).expanduser().resolve()
    from_environment = os.environ.get(variable)
    if from_environment:
        return Path(from_environment).expanduser().resolve()
    for candidate in candidates:
        if candidate.is_dir():
            return candidate.resolve()
    return candidates[0].resolve()


def tool_version(command: list[str]) -> str | None:
    code, stdout, _ = run_capture(command, ROOT)
    if code != 0:
        return None
    return stdout.strip()


def dispatch_errors() -> list[str]:
    errors: list[str] = []
    required_live = set(REQUIRED_LIVE_CASES)
    required_local = set(REQUIRED_LOCAL_CASES)
    missing_live = sorted(required_live - SMOKE_SCRIPT_CASES)
    missing_local = sorted(required_local - set(LOCAL_CARGO_TEST_CASES))
    missing_supported = sorted(set(REQUIRED_CASES) - SUPPORTED_CASES)
    if missing_live:
        errors.append(f"required live cases missing from smoke dispatch: {missing_live}")
    if missing_local:
        errors.append(f"required local cases missing from cargo dispatch: {missing_local}")
    if missing_supported:
        errors.append(f"required cases missing from harness support: {missing_supported}")
    return errors


def coverage_metadata(selected_cases: list[str]) -> dict[str, Any]:
    required_live = set(REQUIRED_LIVE_CASES)
    required_local = set(REQUIRED_LOCAL_CASES)
    return {
        "required_case_count": len(REQUIRED_CASES),
        "required_live_cases": list(REQUIRED_LIVE_CASES),
        "required_local_cases": list(REQUIRED_LOCAL_CASES),
        "selected_cases": selected_cases,
        "unselected_required_cases": [
            case for case in REQUIRED_CASES if case not in selected_cases
        ],
        "missing_live_dispatch": sorted(required_live - SMOKE_SCRIPT_CASES),
        "missing_local_dispatch": sorted(required_local - set(LOCAL_CARGO_TEST_CASES)),
        "missing_supported_dispatch": sorted(set(REQUIRED_CASES) - SUPPORTED_CASES),
    }


def cargo_test_summaries(output: str) -> list[dict[str, int | str]]:
    return [
        {
            "result": match.group("result"),
            "passed": int(match.group("passed")),
            "failed": int(match.group("failed")),
            "ignored": int(match.group("ignored")),
        }
        for match in CARGO_RESULT_RE.finditer(output)
    ]


def validate_cargo_evidence(output: str) -> tuple[str, str, list[dict[str, int | str]]]:
    summaries = cargo_test_summaries(output)
    if not summaries:
        return "fail", "cargo output did not contain a test-result summary", summaries
    passed = sum(int(summary["passed"]) for summary in summaries)
    failed = sum(int(summary["failed"]) for summary in summaries)
    ignored = sum(int(summary["ignored"]) for summary in summaries)
    if failed:
        return "fail", f"cargo reported {failed} failed test(s)", summaries
    if ignored:
        return "fail", f"cargo reported {ignored} ignored test(s)", summaries
    if passed == 0:
        return "fail", "cargo did not execute a passing required test", summaries
    return "pass", f"cargo executed {passed} passing test(s)", summaries


def validate_live_evidence(report_path: Path) -> tuple[str, str, dict[str, Any] | None]:
    if not report_path.is_file():
        return "fail", "live case exited successfully without report.json", None
    try:
        payload = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return "fail", f"live case produced invalid report.json: {error}", None
    if not isinstance(payload, dict):
        return "fail", "live case report.json is not an object", None
    status = payload.get("status")
    if status == "pass":
        for field in ("skipped", "ignored"):
            if payload.get(field):
                return "fail", f"live report records {field} required scenario(s)", payload
        return "pass", "live report status is pass", payload
    if status in {"blocked", "unverified"}:
        return "blocked", f"live report status is {status}", payload
    return "fail", f"live report status is {status!r}", payload


def preflight(
    python_executable: str,
    bash: str | None,
    harness: Path,
    reticulum_path: Path,
    lxmf_path: Path,
) -> tuple[list[str], dict[str, Any]]:
    errors: list[str] = []
    candidate = git_metadata(ROOT)
    reticulum = git_metadata(reticulum_path)
    lxmf = git_metadata(lxmf_path)

    if not candidate["revision"]:
        errors.append("candidate checkout is not a Git worktree")
    if not candidate["clean"]:
        errors.append("candidate checkout is dirty; exact acceptance requires a clean commit")
    if not harness.is_file():
        errors.append(f"compatibility harness is missing: {harness}")
    if not reticulum_path.is_dir() or not reticulum["revision"]:
        errors.append(f"Python Reticulum checkout is unavailable: {reticulum_path}")
    elif reticulum["revision"] != EXPECTED_RETICULUM_REVISION:
        errors.append(
            "Python Reticulum checkout does not match the frozen parity target: "
            f"{reticulum['revision']} != {EXPECTED_RETICULUM_REVISION}"
        )
    if not reticulum["clean"]:
        errors.append("Python Reticulum checkout is dirty")
    if not lxmf_path.is_dir() or not lxmf["revision"]:
        errors.append(f"Python LXMF checkout is unavailable: {lxmf_path}")
    elif lxmf["revision"] != EXPECTED_LXMF_REVISION:
        errors.append(
            "Python LXMF checkout does not match the pinned compatibility reference: "
            f"{lxmf['revision']} != {EXPECTED_LXMF_REVISION}"
        )
    if not lxmf["clean"]:
        errors.append("Python LXMF checkout is dirty")
    if not shutil.which(python_executable) and not Path(python_executable).is_file():
        errors.append(f"Python executable is unavailable: {python_executable}")
    if not bash or (Path(bash).is_absolute() and not Path(bash).is_file()):
        errors.append("no usable Bash runner was found")

    module_env = os.environ.copy()
    module_env["PYTHONPATH"] = os.pathsep.join(
        [str(reticulum_path), str(lxmf_path), module_env.get("PYTHONPATH", "")]
    ).rstrip(os.pathsep)
    module_code, _, module_error = run_capture(
        [python_executable, "-c", "import RNS, LXMF"], ROOT, env=module_env
    )
    if module_code != 0:
        errors.append(f"Python reference modules are unavailable: {module_error or 'import failed'}")

    return errors, {
        "candidate": candidate,
        "reticulum": reticulum,
        "lxmf": lxmf,
        "python": tool_version([python_executable, "--version"]),
        "rustc": tool_version(["rustc", "-Vv"]),
        "cargo": tool_version(["cargo", "-V"]),
        "bash": tool_version([bash, "--version"]) if bash else None,
        "platform": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python_implementation": platform.python_implementation(),
        },
    }


def run_process(
    command: list[str], env: dict[str, str], stdout_path: Path, stderr_path: Path, timeout: float
) -> dict[str, Any]:
    started = time.monotonic()
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        creationflags = 0
        if os.name == "nt":
            creationflags = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
        try:
            process = subprocess.Popen(
                command,
                cwd=ROOT,
                env=env,
                stdout=stdout,
                stderr=stderr,
                start_new_session=os.name != "nt",
                creationflags=creationflags,
            )
        except OSError as error:
            return {
                "returncode": None,
                "timed_out": False,
                "duration_seconds": round(time.monotonic() - started, 3),
                "error": str(error),
            }
        timed_out = False
        try:
            returncode = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            terminate_process_tree(process)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                kill_process_tree(process)
                process.wait()
            returncode = 124
    return {
        "returncode": returncode,
        "timed_out": timed_out,
        "duration_seconds": round(time.monotonic() - started, 3),
    }


def run_case(
    case_id: str,
    output_dir: Path,
    python_executable: str,
    bash: str,
    harness: Path,
    reticulum_path: Path,
    lxmf_path: Path,
    timeout: float,
) -> dict[str, Any]:
    case_dir = output_dir / "cases" / case_id
    case_dir.mkdir(parents=True, exist_ok=True)
    stdout_path = case_dir / "stdout.log"
    stderr_path = case_dir / "stderr.log"
    report_path = case_dir / "report.json"
    for stale_path in (stdout_path, stderr_path, report_path):
        try:
            stale_path.unlink()
        except FileNotFoundError:
            pass
    environment = os.environ.copy()
    environment.update(
        {
            "RETICULUM_PY_REPO": str(reticulum_path),
            "LXMF_PY_REPO": str(lxmf_path),
            "LXMF_PY_COMPAT_HARNESS": str(harness),
            "LXMF_PYTHON_BIN": python_executable,
            "PYTHON_BIN": python_executable,
            "BASH_BIN": bash,
            "LOG_DIR": str(case_dir / "smoke-logs"),
            "REPORT_PATH": str(report_path),
            "LXMF_PY_COMPAT_CASE_TIMEOUT_SECS": str(timeout),
        }
    )
    command = [python_executable, str(harness), case_id]
    execution = run_process(command, environment, stdout_path, stderr_path, timeout)
    result: dict[str, Any] = {
        "case_id": case_id,
        "kind": "live_python_rust" if case_id in SMOKE_SCRIPT_CASES else "local_cargo",
        "command": command_text(command),
        "dispatched_command": command_text(
            ["bash", "tools/scripts/python-lxmd-rust-lxmd-smoke.sh"]
            if case_id in SMOKE_SCRIPT_CASES
            else LOCAL_CARGO_TEST_CASES[case_id]
        ),
        "environment": {
            key: repo_path(Path(value))
            if key in {"RETICULUM_PY_REPO", "LXMF_PY_REPO", "LXMF_PY_COMPAT_HARNESS"}
            else value
            for key, value in environment.items()
            if key
            in {
                "RETICULUM_PY_REPO",
                "LXMF_PY_REPO",
                "LXMF_PY_COMPAT_HARNESS",
                "LXMF_PYTHON_BIN",
                "PYTHON_BIN",
                "BASH_BIN",
                "LOG_DIR",
                "REPORT_PATH",
                "LXMF_PY_COMPAT_CASE_TIMEOUT_SECS",
            }
        },
        "stdout": repo_path(stdout_path),
        "stderr": repo_path(stderr_path),
        "report": repo_path(report_path),
        **execution,
    }
    if execution["returncode"] is None:
        result.update({"status": "blocked", "reason": execution["error"]})
        return result
    if execution["timed_out"]:
        result.update({"status": "fail", "reason": f"case timed out after {timeout:g} seconds"})
        return result

    if case_id in SMOKE_SCRIPT_CASES:
        status, reason, report = validate_live_evidence(report_path)
        result.update({"status": status, "reason": reason})
        if report is not None:
            result["live_report"] = report
    else:
        output = stdout_path.read_text(encoding="utf-8", errors="replace")
        status, reason, summaries = validate_cargo_evidence(output)
        result.update({"status": status, "reason": reason, "cargo_summaries": summaries})
    if execution["returncode"] != 0 and result["status"] == "pass":
        result.update({"status": "fail", "reason": f"harness exited with {execution['returncode']}"})
    return result


def overall_status(
    selected_cases: list[str], results: list[dict[str, Any]], preflight_errors: list[str], allow_subset: bool
) -> tuple[str, int]:
    if preflight_errors:
        return "blocked", 1
    if any(result["status"] == "fail" for result in results):
        return "fail", 1
    if any(result["status"] == "blocked" for result in results):
        return "blocked", 1
    if len(selected_cases) != len(REQUIRED_CASES):
        return "partial", 0 if allow_subset else 1
    return "pass", 0


def default_output_path() -> Path:
    timestamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return ROOT / "target" / "interop" / "python-compat-matrix" / timestamp / "matrix.json"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=default_output_path())
    parser.add_argument("--case", action="append", dest="cases", help="run only this case (repeatable)")
    parser.add_argument("--all", action="store_true", help="run all required cases (the default)")
    parser.add_argument(
        "--allow-subset",
        action="store_true",
        help="return success for a passing development subset, while recording status=partial",
    )
    parser.add_argument("--timeout", type=float, default=float(os.environ.get("LXMF_PY_COMPAT_CASE_TIMEOUT_SECS", "420")))
    parser.add_argument("--python", dest="python_executable", default=os.environ.get("LXMF_PYTHON_BIN", sys.executable))
    parser.add_argument("--bash", dest="bash", default=os.environ.get("BASH_BIN"))
    parser.add_argument("--harness", type=Path)
    parser.add_argument("--python-rns-path", type=str)
    parser.add_argument("--python-lxmf-path", type=str)
    args = parser.parse_args(argv)
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    selected = list(dict.fromkeys(args.cases or REQUIRED_CASES))
    unknown = sorted(set(selected) - set(REQUIRED_CASES))
    if unknown:
        parser.error(f"unknown required case(s): {', '.join(unknown)}")
    args.selected_cases = selected
    args.harness = (args.harness or ROOT / "tools/scripts/python_compat_harness.py").resolve()
    args.reticulum_path = resolve_reference_path(
        args.python_rns_path,
        "RETICULUM_PY_REPO",
        (ROOT / ".tmp/python-refs/Reticulum", ROOT.parent / "reticulum"),
    )
    args.lxmf_path = resolve_reference_path(
        args.python_lxmf_path,
        "LXMF_PY_REPO",
        (ROOT / ".tmp/python-refs/LXMF", ROOT.parent / "lxmf"),
    )
    args.output = args.output if args.output.is_absolute() else ROOT / args.output
    args.output = args.output.resolve()
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    started_at = utc_now()
    dispatch = dispatch_errors()
    bash = args.bash or resolve_bash()
    preflight_errors, environment = preflight(
        args.python_executable,
        bash,
        args.harness,
        args.reticulum_path,
        args.lxmf_path,
    )
    preflight_errors = dispatch + preflight_errors
    args.output.parent.mkdir(parents=True, exist_ok=True)
    evidence_dir = args.output.parent
    results: list[dict[str, Any]] = []
    if not preflight_errors:
        for case_id in args.selected_cases:
            print(f"[python-compat-matrix] running {case_id}", flush=True)
            results.append(
                run_case(
                    case_id,
                    evidence_dir,
                    args.python_executable,
                    bash or "",
                    args.harness,
                    args.reticulum_path,
                    args.lxmf_path,
                    args.timeout,
                )
            )
            print(
                f"[python-compat-matrix] {case_id}: {results[-1]['status']} "
                f"({results[-1]['reason']})",
                flush=True,
            )
    status, exit_code = overall_status(args.selected_cases, results, preflight_errors, args.allow_subset)
    report = {
        "schema_version": SCHEMA_VERSION,
        "status": status,
        "started_at": started_at,
        "finished_at": utc_now(),
        "candidate": environment,
        "reference": {
            "reticulum_expected_revision": EXPECTED_RETICULUM_REVISION,
            "lxmf_expected_revision": EXPECTED_LXMF_REVISION,
            "reticulum_path": repo_path(args.reticulum_path),
            "lxmf_path": repo_path(args.lxmf_path),
        },
        "fault_injection": {"enabled": False, "seed": None},
        "timeout_seconds": args.timeout,
        "coverage": coverage_metadata(args.selected_cases),
        "preflight_errors": preflight_errors,
        "passed": sum(result["status"] == "pass" for result in results),
        "failed": sum(result["status"] == "fail" for result in results),
        "blocked": sum(result["status"] == "blocked" for result in results)
        + (1 if preflight_errors else 0),
        "skipped": len(REQUIRED_CASES) - len(args.selected_cases),
        "ignored": 0,
        "cases": results,
        "commands": {
            "runner": command_text([args.python_executable, str(args.harness), "<case_id>"]),
            "full_invocation": command_text(
                [args.python_executable, str(Path(__file__).resolve()), "--all", "--output", str(args.output)]
            ),
        },
    }
    report_path = args.output
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"[python-compat-matrix] status={status} report={repo_path(report_path)}")
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
