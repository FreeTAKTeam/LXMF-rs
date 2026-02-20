#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

if ! cargo fuzz --help >/dev/null 2>&1; then
  echo "cargo-fuzz is required. Install with: cargo install cargo-fuzz" >&2
  exit 1
fi

MAX_TOTAL_TIME="${FUZZ_MAX_TOTAL_TIME:-300}"

echo "Running rns-rpc fuzz target (rpc_frame_envelope) for ${MAX_TOTAL_TIME}s"
cargo fuzz run \
  --manifest-path crates/libs/rns-rpc/fuzz/Cargo.toml \
  rpc_frame_envelope \
  -- -max_total_time="${MAX_TOTAL_TIME}"

echo "Running lxmf-sdk fuzz target (sdk_json_envelope) for ${MAX_TOTAL_TIME}s"
cargo fuzz run \
  --manifest-path crates/libs/lxmf-sdk/fuzz/Cargo.toml \
  sdk_json_envelope \
  -- -max_total_time="${MAX_TOTAL_TIME}"

echo "Fuzz campaign completed"
