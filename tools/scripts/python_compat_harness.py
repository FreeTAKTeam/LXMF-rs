#!/usr/bin/env python3
import os
import subprocess
import sys
from pathlib import Path


SUPPORTED_CASES = {
    "direct_rust_to_python",
    "direct_python_to_rust",
    "opportunistic_rust_to_python",
    "propagated_rust_to_python",
    "propagated_python_to_rust",
    "resource_transfer",
    "lxm_interchange",
}

SMOKE_SCRIPT_CASES = {
    "direct_python_to_rust",
    "propagated_python_to_rust",
}


def main() -> int:
    supported_cases = ", ".join(sorted(SUPPORTED_CASES))
    if len(sys.argv) != 2:
        print(
            "usage: python_compat_harness.py <case_id> "
            f"(one of: {supported_cases})",
            file=sys.stderr,
        )
        return 2

    case_id = sys.argv[1]
    if case_id not in SUPPORTED_CASES:
        print(
            f"unsupported compatibility case: {case_id}. "
            f"Supported cases: {supported_cases}",
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

    if not smoke_script.exists() or not smoke_script.is_file():
        print(f"missing smoke script: {smoke_script}", file=sys.stderr)
        return 2

    env = os.environ.copy()
    env["COMPAT_CASE"] = case_id
    if python_bin := env.get("LXMF_PYTHON_BIN"):
        env.setdefault("PYTHON_BIN", python_bin)

    result = subprocess.run(
        ["bash", str(smoke_script)],
        cwd=repo_root,
        env=env,
        check=False,
    )
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
