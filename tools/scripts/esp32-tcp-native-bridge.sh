#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

BIND_ADDR="${BIND_ADDR:-0.0.0.0:7443}"
BRIDGE_MODE="${BRIDGE_MODE:-capture}"
TIMEOUT_SECS="${TIMEOUT_SECS:-30}"
RPC_ADDR="${RPC_ADDR:-127.0.0.1:4243}"
RUNTIME_SEQ="${RUNTIME_SEQ:-}"
PAYLOAD="${PAYLOAD:-bridge-ping}"
SOURCE_HEX="${SOURCE_HEX:-99999999999999999999999999999999}"
DESTINATION_HEX="${DESTINATION_HEX:-22222222222222222222222222222222}"
CONTENT_TYPE="${CONTENT_TYPE:-image/jpeg}"
CHUNK_SIZE="${CHUNK_SIZE:-8192}"
CAPTURE_OUT="${CAPTURE_OUT:-}"
CAPTURE_PROFILE="${CAPTURE_PROFILE:-default}"
LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/hil}"
mkdir -p "${LOG_DIR}"
CAPTURE_DIR_DEFAULT="${LOG_DIR}/captures"
if [[ "${BRIDGE_MODE}" == "capture" && -z "${CAPTURE_OUT}" ]]; then
  mkdir -p "${CAPTURE_DIR_DEFAULT}"
  CAPTURE_OUT="${CAPTURE_DIR_DEFAULT}/bridge-capture-$(date +%s).jpg"
fi
LOG_PATH="${LOG_DIR}/esp32-tcp-native-bridge.log"

echo "[esp32-tcp-native-bridge] building rnx"
cargo build -p rns-tools --bin rnx --quiet

cmd=(
  "${REPO_ROOT}/target/debug/rnx"
  tcp-native-bridge
  --bind "${BIND_ADDR}"
  --mode "${BRIDGE_MODE}"
  --rpc "${RPC_ADDR}"
  --content-type "${CONTENT_TYPE}"
  --chunk-size "${CHUNK_SIZE}"
  --timeout-secs "${TIMEOUT_SECS}"
  --payload "${PAYLOAD}"
  --source-hex "${SOURCE_HEX}"
  --destination-hex "${DESTINATION_HEX}"
)
if [[ -n "${RUNTIME_SEQ}" ]]; then
  cmd+=(--runtime-seq "${RUNTIME_SEQ}")
fi
if [[ -n "${CAPTURE_OUT}" ]]; then
  cmd+=(--capture-out "${CAPTURE_OUT}")
fi
if [[ "${BRIDGE_MODE}" == "capture" ]]; then
  cmd+=(--capture-profile "${CAPTURE_PROFILE}")
fi

echo "[esp32-tcp-native-bridge] listening bind=${BIND_ADDR} mode=${BRIDGE_MODE} rpc=${RPC_ADDR}"
echo "[esp32-tcp-native-bridge] log=${LOG_PATH}"
"${cmd[@]}" | tee "${LOG_PATH}"
