#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

CHECK_ONLY=0
SKIP_TOOLS=0
SKIP_SMOKE=0

for arg in "$@"; do
  case "$arg" in
    --check)
      CHECK_ONLY=1
      ;;
    --skip-tools)
      SKIP_TOOLS=1
      ;;
    --skip-smoke)
      SKIP_SMOKE=1
      ;;
    *)
      echo "unknown argument: $arg" >&2
      echo "usage: tools/scripts/bootstrap-dev.sh [--check] [--skip-tools] [--skip-smoke]" >&2
      exit 2
      ;;
  esac
done

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing required command: $cmd" >&2
    exit 1
  fi
}

ensure_toolchain() {
  local toolchain="$1"
  if rustup toolchain list | awk '{print $1}' | grep -Eq "^${toolchain}(-|$)"; then
    return
  fi
  if [[ "$CHECK_ONLY" -eq 1 ]]; then
    echo "missing rustup toolchain: $toolchain" >&2
    exit 1
  fi
  rustup toolchain install "$toolchain" --profile minimal
}

ensure_component() {
  local component="$1"
  if rustup component list --installed | grep -Eq "^${component}(-|$)"; then
    return
  fi
  if [[ "$CHECK_ONLY" -eq 1 ]]; then
    echo "missing rustup component: $component" >&2
    exit 1
  fi
  rustup component add "$component"
}

ensure_cargo_tool() {
  local binary="$1"
  shift
  if command -v "$binary" >/dev/null 2>&1; then
    return
  fi
  if [[ "$CHECK_ONLY" -eq 1 ]]; then
    echo "missing cargo tool binary: $binary" >&2
    exit 1
  fi
  cargo install --locked "$@"
}

require_cmd rustup
require_cmd cargo

ensure_toolchain stable
ensure_toolchain 1.75.0
ensure_component rustfmt
ensure_component clippy

if [[ "$SKIP_TOOLS" -eq 0 ]]; then
  ensure_cargo_tool cargo-nextest cargo-nextest
  ensure_cargo_tool cargo-deny cargo-deny
  ensure_cargo_tool cargo-audit cargo-audit
  ensure_cargo_tool cargo-udeps cargo-udeps
  ensure_cargo_tool cargo-public-api cargo-public-api --version 0.50.2
fi

if [[ "$CHECK_ONLY" -eq 0 ]]; then
  cargo fetch --locked
fi

if [[ "$SKIP_SMOKE" -eq 0 ]]; then
  cargo run -p xtask -- ci --stage lint-format
fi

echo "bootstrap completed"
