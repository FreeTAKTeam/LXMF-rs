#!/usr/bin/env python3
"""Enforce the companion file/class ceiling and absence of native binaries."""
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[2] / "android/rns-bluetooth/src"
errors = []
for path in sorted(root.rglob("*")):
    if not path.is_file():
        continue
    if path.suffix in {".java", ".kt"}:
        lines = len(path.read_text().splitlines())
        # Every class is contained in its file, so a strict file cap also bounds classes.
        if lines > 500:
            errors.append(f"{path}: {lines} lines exceeds 500 (no exemptions)")
    if path.suffix in {".so", ".aar", ".jar", ".apk", ".aab"}:
        errors.append(f"{path}: binary packaging forbidden in companion sources")
if errors:
    sys.exit("\n".join(errors))
print("Bluetooth companion source checks: ok")
