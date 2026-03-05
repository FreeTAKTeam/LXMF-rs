#!/usr/bin/env bash
set -euo pipefail

# ESP32-CAM BLE capture smoke:
# 1) starts local reticulumd RPC
# 2) runs rnx camera-capture-upload against ESP32 BLE peripheral
# 3) verifies attachment persisted via sdk_attachment_list_v2

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

RPC_ADDR="${RPC_ADDR:-127.0.0.1:4243}"
DB_PATH="${DB_PATH:-${REPO_ROOT}/.tmp/esp32-camera-smoke/reticulum.db}"
LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/hil}"
TIMEOUT_SECS="${TIMEOUT_SECS:-25}"
CHUNK_SIZE="${CHUNK_SIZE:-8192}"
CONTENT_TYPE="${CONTENT_TYPE:-image/jpeg}"
KEEP_DAEMON="${KEEP_DAEMON:-0}"
VERBOSE="${VERBOSE:-1}"
AUTO_DISCOVER_PERIPHERAL="${AUTO_DISCOVER_PERIPHERAL:-1}"

PERIPHERAL_ID="${PERIPHERAL_ID:-}"
SERVICE_UUID="${SERVICE_UUID:-}"
WRITE_CHAR_UUID="${WRITE_CHAR_UUID:-}"
NOTIFY_CHAR_UUID="${NOTIFY_CHAR_UUID:-}"

if [[ -z "${PERIPHERAL_ID}" || -z "${SERVICE_UUID}" || -z "${WRITE_CHAR_UUID}" || -z "${NOTIFY_CHAR_UUID}" ]]; then
  cat <<USAGE >&2
Missing required BLE settings.
Set these environment variables before running:
  PERIPHERAL_ID
  SERVICE_UUID
  WRITE_CHAR_UUID
  NOTIFY_CHAR_UUID
Optional:
  RPC_ADDR=${RPC_ADDR}
  TIMEOUT_SECS=${TIMEOUT_SECS}
  CHUNK_SIZE=${CHUNK_SIZE}
  CONTENT_TYPE=${CONTENT_TYPE}
  KEEP_DAEMON=${KEEP_DAEMON}
USAGE
  exit 2
fi

mkdir -p "$(dirname "${DB_PATH}")" "${LOG_DIR}"
DAEMON_LOG="${LOG_DIR}/esp32-camera-smoke-reticulumd.log"
RUN_LOG="${LOG_DIR}/esp32-camera-smoke-rnx.log"
DISCOVERY_LOG="${LOG_DIR}/esp32-camera-smoke-discovery.log"
ACTIVE_PERIPHERAL_ID="${PERIPHERAL_ID}"

log() {
  if [[ "${VERBOSE}" == "1" ]]; then
    printf '[smoke %s] %s\n' "$(date '+%H:%M:%S')" "$*" >&2
  fi
}

cleanup() {
  if [[ -n "${DAEMON_PID:-}" ]] && kill -0 "${DAEMON_PID}" 2>/dev/null; then
    if [[ "${KEEP_DAEMON}" == "1" ]]; then
      log "keeping daemon running pid=${DAEMON_PID}"
    else
      kill "${DAEMON_PID}" >/dev/null 2>&1 || true
      wait "${DAEMON_PID}" >/dev/null 2>&1 || true
    fi
  fi
}
trap cleanup EXIT

on_error() {
  local line_no="${1:-unknown}"
  echo "[smoke] failure at line ${line_no}" >&2
  if [[ -f "${DISCOVERY_LOG}" ]]; then
    echo "[smoke] discovery log tail:" >&2
    tail -n 25 "${DISCOVERY_LOG}" >&2 || true
  fi
  if [[ -f "${RUN_LOG}" ]]; then
    echo "[smoke] rnx log tail:" >&2
    tail -n 40 "${RUN_LOG}" >&2 || true
  fi
  if [[ -f "${DAEMON_LOG}" ]]; then
    echo "[smoke] daemon log tail:" >&2
    tail -n 40 "${DAEMON_LOG}" >&2 || true
  fi
}
trap 'on_error $LINENO' ERR

log "config rpc=${RPC_ADDR} timeout=${TIMEOUT_SECS}s chunk_size=${CHUNK_SIZE} peripheral_id=${PERIPHERAL_ID}"
log "building reticulumd and rnx binaries"
cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnx --quiet

log "starting reticulumd"
"${REPO_ROOT}/target/debug/reticulumd" --rpc "${RPC_ADDR}" --db "${DB_PATH}" \
  >"${DAEMON_LOG}" 2>&1 &
DAEMON_PID=$!

# Wait for RPC socket
for _ in $(seq 1 100); do
  if nc -z "${RPC_ADDR%:*}" "${RPC_ADDR##*:}" >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

if ! nc -z "${RPC_ADDR%:*}" "${RPC_ADDR##*:}" >/dev/null 2>&1; then
  echo "[smoke] daemon RPC did not become ready at ${RPC_ADDR}" >&2
  exit 1
fi

log "daemon ready at ${RPC_ADDR}"
if [[ "${AUTO_DISCOVER_PERIPHERAL}" == "1" ]]; then
  log "preflight BLE discovery using ble-find-camera"
  if "${REPO_ROOT}/target/debug/rnx" ble-find-camera \
    --scan-secs 12 \
    --service-uuid "${SERVICE_UUID}" \
    --write-char-uuid "${WRITE_CHAR_UUID}" \
    --notify-char-uuid "${NOTIFY_CHAR_UUID}" \
    2>&1 | tee "${DISCOVERY_LOG}"; then
    DISCOVERED_ID="$(awk '/BLE_FIND_CAMERA match id=/{for(i=1;i<=NF;i++) if($i ~ /^id=/){sub("id=","",$i); print $i; exit}}' "${DISCOVERY_LOG}")"
    if [[ -n "${DISCOVERED_ID}" ]]; then
      ACTIVE_PERIPHERAL_ID="${DISCOVERED_ID}"
      log "using discovered peripheral id=${ACTIVE_PERIPHERAL_ID}"
    fi
  else
    log "ble-find-camera failed, continuing with provided peripheral id=${ACTIVE_PERIPHERAL_ID}"
  fi
else
  log "auto discovery disabled, using provided peripheral id=${ACTIVE_PERIPHERAL_ID}"
fi

log "starting camera-capture-upload"
set -x
"${REPO_ROOT}/target/debug/rnx" camera-capture-upload \
  --rpc "${RPC_ADDR}" \
  --peripheral-id "${ACTIVE_PERIPHERAL_ID}" \
  --service-uuid "${SERVICE_UUID}" \
  --write-char-uuid "${WRITE_CHAR_UUID}" \
  --notify-char-uuid "${NOTIFY_CHAR_UUID}" \
  --content-type "${CONTENT_TYPE}" \
  --chunk-size "${CHUNK_SIZE}" \
  --timeout-secs "${TIMEOUT_SECS}" 2>&1 | tee "${RUN_LOG}"
set +x

log "verifying attachment presence through sdk_attachment_list_v2"
ATTACHMENT_COUNT=$(
  python3 - <<PY
import http.client
import json
rpc="${RPC_ADDR}"
host, port = rpc.split(":")
port = int(port)
req = {
  "id": 9001,
  "method": "sdk_attachment_list_v2",
  "params": {"limit": 50}
}
conn = http.client.HTTPConnection(host, port, timeout=5)
conn.request("POST", "/rpc", body=json.dumps(req), headers={"Content-Type": "application/json"})
resp = conn.getresponse()
body = resp.read().decode("utf-8")
conn.close()
obj = json.loads(body)
items = obj.get("result", {}).get("attachments", [])
print(len(items))
PY
)

echo "[smoke] attachment_count=${ATTACHMENT_COUNT}"
echo "[smoke] active_peripheral_id=${ACTIVE_PERIPHERAL_ID}"
echo "[smoke] logs: ${DAEMON_LOG} ${RUN_LOG} ${DISCOVERY_LOG}"
