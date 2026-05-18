#!/usr/bin/env python3
"""Regenerate deterministic LXMF interop fixtures using Rust test harness."""
import os, subprocess, sys

if os.environ.get("LXMF_REGEN_FIXTURES") != "1":
    print("Set LXMF_REGEN_FIXTURES=1 to regenerate fixtures", file=sys.stderr)
    sys.exit(2)

subprocess.run([
    "cargo", "test", "-p", "lxmf-wire", "regenerate_fixtures_when_env_set", "--", "--exact", "--nocapture"
], check=True)
print("Fixtures regenerated under crates/libs/lxmf-core/tests/fixtures/lxmf_interop")
