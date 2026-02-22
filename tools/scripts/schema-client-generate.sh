#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

MODE="write"
if [[ $# -gt 1 ]]; then
  echo "usage: $0 [--check|--write]" >&2
  exit 1
fi

if [[ $# -eq 1 ]]; then
  case "$1" in
    --check)
      MODE="check"
      ;;
    --write)
      MODE="write"
      ;;
    *)
      echo "unknown mode '$1' (expected --check or --write)" >&2
      exit 1
      ;;
  esac
fi

if [[ "$MODE" == "check" ]]; then
  cargo run -p xtask -- schema-client-generate --check
else
  cargo run -p xtask -- schema-client-generate
fi
