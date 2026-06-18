#!/usr/bin/env python3
import os
import shutil
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

    result = subprocess.run(
        [bash, str(smoke_script)],
        cwd=repo_root,
        env=env,
        check=False,
    )
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
