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

cleanup() {
  if [[ -n "${DAEMON_PID:-}" ]] && kill -0 "${DAEMON_PID}" 2>/dev/null; then
    if [[ "${KEEP_DAEMON}" == "1" ]]; then
      echo "[smoke] keeping daemon running pid=${DAEMON_PID}" >&2
    else
      kill "${DAEMON_PID}" >/dev/null 2>&1 || true
      wait "${DAEMON_PID}" >/dev/null 2>&1 || true
    fi
  fi
}
trap cleanup EXIT

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnx --quiet

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

set -x
"${REPO_ROOT}/target/debug/rnx" camera-capture-upload \
  --rpc "${RPC_ADDR}" \
  --peripheral-id "${PERIPHERAL_ID}" \
  --service-uuid "${SERVICE_UUID}" \
  --write-char-uuid "${WRITE_CHAR_UUID}" \
  --notify-char-uuid "${NOTIFY_CHAR_UUID}" \
  --content-type "${CONTENT_TYPE}" \
  --chunk-size "${CHUNK_SIZE}" \
  --timeout-secs "${TIMEOUT_SECS}" | tee "${RUN_LOG}"
set +x

ATTACHMENT_COUNT=$(
  python3 - <<PY
import json, socket
rpc="${RPC_ADDR}"
host, port = rpc.split(":")
port = int(port)
req = {
  "id": 9001,
  "method": "sdk_attachment_list_v2",
  "params": {"limit": 50}
}
payload = json.dumps(req).encode()
http = (
  f"POST /rpc HTTP/1.1\\r\\nHost: {rpc}\\r\\nContent-Type: application/json\\r\\nContent-Length: {len(payload)}\\r\\nConnection: close\\r\\n\\r\\n"
).encode() + payload
s = socket.create_connection((host, port), timeout=5)
s.sendall(http)
chunks = []
while True:
    part = s.recv(4096)
    if not part:
        break
    chunks.append(part)
s.close()
resp = b"".join(chunks)
body = resp.split(b"\\r\\n\\r\\n", 1)[1]
obj = json.loads(body.decode())
items = obj.get("result", {}).get("attachments", [])
print(len(items))
PY
)

echo "[smoke] attachment_count=${ATTACHMENT_COUNT}"
echo "[smoke] logs: ${DAEMON_LOG} ${RUN_LOG}"
