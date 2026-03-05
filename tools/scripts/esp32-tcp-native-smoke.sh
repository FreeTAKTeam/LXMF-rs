#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/hil}"
mkdir -p "${LOG_DIR}"

LISTENER_LOG="${LOG_DIR}/esp32-tcp-native-listener.log"
BIND_ADDR="${BIND_ADDR:-0.0.0.0:7443}"
LISTENER_MODE="${LISTENER_MODE:-passive}"
TIMEOUT_SECS="${TIMEOUT_SECS:-20}"
RUNTIME_SEQ="${RUNTIME_SEQ:-}"

echo "[esp32-tcp-native-smoke] building rnx"
cargo build -p rns-tools --bin rnx --quiet

cmd=(
  "${REPO_ROOT}/target/debug/rnx"
  tcp-native-listener
  --bind "${BIND_ADDR}"
  --mode "${LISTENER_MODE}"
  --timeout-secs "${TIMEOUT_SECS}"
)

if [[ -n "${RUNTIME_SEQ}" ]]; then
  cmd+=(--runtime-seq "${RUNTIME_SEQ}")
fi

echo "[esp32-tcp-native-smoke] listening bind=${BIND_ADDR} mode=${LISTENER_MODE}"
echo "[esp32-tcp-native-smoke] log=${LISTENER_LOG}"
"${cmd[@]}" | tee "${LISTENER_LOG}"

if grep -q "TCP_NATIVE_LISTENER ok:" "${LISTENER_LOG}"; then
  echo "[esp32-tcp-native-smoke] pass"
else
  echo "[esp32-tcp-native-smoke] missing success marker; see ${LISTENER_LOG}" >&2
  exit 1
fi
