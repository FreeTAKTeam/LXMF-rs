#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

REPORT_PATH="target/embedded/footprint-report.txt"
TARGET_DIR="target/embedded/target"
EXAMPLE_BIN="${TARGET_DIR}/release/examples/embedded_alloc_tick"
FEATURE_MATRIX_PATH="docs/contracts/sdk-v2-feature-matrix.md"

EMBEDDED_MAX_BINARY_BYTES="${EMBEDDED_MAX_BINARY_BYTES:-15728640}"
EMBEDDED_HEAP_BUDGET_BYTES=8388608
EMBEDDED_EVENT_QUEUE_BUDGET_BYTES=2097152
EMBEDDED_ATTACHMENT_SPOOL_BUDGET_BYTES=16777216

file_size_bytes() {
  local path="$1"
  if stat -c%s "$path" >/dev/null 2>&1; then
    stat -c%s "$path"
  else
    stat -f%z "$path"
  fi
}

if ! grep -Fq "| \`embedded-alloc\` | 8,388,608 | 2,097,152 | 16,777,216 |" "$FEATURE_MATRIX_PATH"; then
  echo "error: embedded memory budget row drift in ${FEATURE_MATRIX_PATH}" >&2
  exit 1
fi

mkdir -p "$(dirname "$REPORT_PATH")" "$TARGET_DIR"

CARGO_TARGET_DIR="$TARGET_DIR" \
cargo build \
  --locked \
  --release \
  -p lxmf-sdk \
  --example embedded_alloc_tick \
  --no-default-features \
  --features std,rpc-backend,embedded-alloc

if [[ ! -f "$EXAMPLE_BIN" ]]; then
  echo "error: missing embedded example binary at ${EXAMPLE_BIN}" >&2
  exit 1
fi

BINARY_BYTES="$(file_size_bytes "$EXAMPLE_BIN")"

{
  echo "# Embedded Footprint Report"
  echo
  echo "example_binary=${EXAMPLE_BIN}"
  echo "example_binary_bytes=${BINARY_BYTES}"
  echo "example_binary_budget_bytes=${EMBEDDED_MAX_BINARY_BYTES}"
  echo "embedded_heap_budget_bytes=${EMBEDDED_HEAP_BUDGET_BYTES}"
  echo "embedded_event_queue_budget_bytes=${EMBEDDED_EVENT_QUEUE_BUDGET_BYTES}"
  echo "embedded_attachment_spool_budget_bytes=${EMBEDDED_ATTACHMENT_SPOOL_BUDGET_BYTES}"
  echo "feature_matrix_source=${FEATURE_MATRIX_PATH}"
} >"$REPORT_PATH"

if (( BINARY_BYTES > EMBEDDED_MAX_BINARY_BYTES )); then
  echo "error: embedded example binary exceeds budget (${BINARY_BYTES} > ${EMBEDDED_MAX_BINARY_BYTES})" >&2
  exit 1
fi

echo "embedded footprint check passed; report written to ${REPORT_PATH}"
