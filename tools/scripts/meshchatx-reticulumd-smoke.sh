#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

MESHCHATX_ROOT="${MESHCHATX_ROOT:-${REPO_ROOT}/../MeshChatX}"
RPC_ADDR="${RPC_ADDR:-127.0.0.1:4243}"
TRANSPORT_ADDR="${TRANSPORT_ADDR:-127.0.0.1:37429}"
MESHCHAT_HOST="${MESHCHAT_HOST:-127.0.0.1}"
MESHCHAT_PORT="${MESHCHAT_PORT:-18080}"
DAEMON_TO_MESH_CONTENT="${DAEMON_TO_MESH_CONTENT:-hello-from-reticulumd}"
MESH_TO_DAEMON_CONTENT="${MESH_TO_DAEMON_CONTENT:-reply-from-meshchatx}"
LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/interop/meshchatx-reticulumd}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
RETICULUMD_DIAGNOSTICS="${RETICULUMD_DIAGNOSTICS:-0}"

if [[ ! -d "${MESHCHATX_ROOT}" ]]; then
  echo "MeshChatX repo not found at ${MESHCHATX_ROOT}" >&2
  exit 1
fi

if ! command -v uv >/dev/null 2>&1; then
  echo "uv is required to run MeshChatX from source" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required for MeshChatX API checks" >&2
  exit 1
fi

mkdir -p "${LOG_DIR}"
TMP_ROOT="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
RET_DB="${TMP_ROOT}/reticulum.db"
RET_LOG="${TMP_ROOT}/reticulumd.log"
MESH_RET="${TMP_ROOT}/mesh-reticulum"
MESH_STORE="${TMP_ROOT}/mesh-storage"
MESH_LOG="${TMP_ROOT}/meshchatx.log"
MESH_CONFIG_JSON="${TMP_ROOT}/mesh-config.json"
MESH_CONVERSATION_JSON="${TMP_ROOT}/mesh-conversation.json"
MESH_SEND_JSON="${TMP_ROOT}/mesh-send.json"
LXMf_SEND_JSON="${TMP_ROOT}/lxmf-send.json"

mkdir -p "${MESH_RET}" "${MESH_STORE}"

SHARED_INSTANCE_PORT="${SHARED_INSTANCE_PORT:-$((38428 + ($$ % 200)))}"
INSTANCE_CONTROL_PORT="${INSTANCE_CONTROL_PORT:-$((SHARED_INSTANCE_PORT + 1))}"

cleanup() {
  local status=$?
  if [[ -n "${MESH_PID:-}" ]]; then
    kill "${MESH_PID}" >/dev/null 2>&1 || true
    wait "${MESH_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${RET_PID:-}" ]]; then
    kill "${RET_PID}" >/dev/null 2>&1 || true
    wait "${RET_PID}" >/dev/null 2>&1 || true
  fi
  if [[ ${status} -ne 0 ]]; then
    echo "[meshchatx-reticulumd-smoke] failed" >&2
    echo "[meshchatx-reticulumd-smoke] logs=${TMP_ROOT}" >&2
  fi
}
trap cleanup EXIT

cat > "${MESH_RET}/config" <<EOF
[reticulum]
  enable_transport = true
  share_instance = yes
  shared_instance_port = ${SHARED_INSTANCE_PORT}
  instance_control_port = ${INSTANCE_CONTROL_PORT}
  discover_interfaces = false
  autoconnect_discovered_interfaces = 0

[logging]
  loglevel = 4

[interfaces]
  [[LXMF RS]]
    type = TCPClientInterface
    enabled = yes
    target_host = ${TRANSPORT_ADDR%:*}
    target_port = ${TRANSPORT_ADDR##*:}
EOF

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p lxmf-cli --bin lxmf-cli --quiet

(
  RETICULUMD_DIAGNOSTICS="${RETICULUMD_DIAGNOSTICS}" \
    "${REPO_ROOT}/target/debug/reticulumd" \
    --rpc "${RPC_ADDR}" \
    --db "${RET_DB}" \
    --transport "${TRANSPORT_ADDR}" \
    --announce-interval-secs 2 >"${RET_LOG}" 2>&1
) &
RET_PID=$!

for _ in $(seq 1 60); do
  if grep -q "delivery destination hash=" "${RET_LOG}"; then
    break
  fi
  sleep 1
done

DAEMON_HASH="$(sed -n 's/.*delivery destination hash=\([0-9a-f]*\).*/\1/p' "${RET_LOG}" | tail -n1)"
if [[ -z "${DAEMON_HASH}" ]]; then
  echo "Failed to read reticulumd delivery hash from ${RET_LOG}" >&2
  exit 1
fi

(
  cd "${MESHCHATX_ROOT}"
  uv run meshchat \
    --headless \
    --host "${MESHCHAT_HOST}" \
    --port "${MESHCHAT_PORT}" \
    --no-https \
    --reticulum-config-dir "${MESH_RET}" \
    --storage-dir "${MESH_STORE}" >"${MESH_LOG}" 2>&1
) &
MESH_PID=$!

for _ in $(seq 1 90); do
  if curl -fsS "http://${MESHCHAT_HOST}:${MESHCHAT_PORT}/api/v1/config" >"${MESH_CONFIG_JSON}" 2>/dev/null; then
    break
  fi
  sleep 1
done

MESH_HASH="$(
  python3 - <<'PY' "${MESH_CONFIG_JSON}"
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    payload = json.load(handle)
print(payload["config"]["lxmf_address_hash"])
PY
)"

if [[ -z "${MESH_HASH}" ]]; then
  echo "Failed to read MeshChatX LXMF address hash" >&2
  exit 1
fi

"${REPO_ROOT}/target/debug/lxmf-cli" \
  --rpc "${RPC_ADDR}" \
  --output json \
  send \
  --source "${DAEMON_HASH}" \
  --destination "${MESH_HASH}" \
  --content "${DAEMON_TO_MESH_CONTENT}" >"${LXMf_SEND_JSON}"

DAEMON_TO_MESH_OK=0
for _ in $(seq 1 60); do
  if curl -fsS "http://${MESHCHAT_HOST}:${MESHCHAT_PORT}/api/v1/lxmf-messages/conversation/${DAEMON_HASH}" >"${MESH_CONVERSATION_JSON}" 2>/dev/null; then
    found="$(
      python3 - <<'PY' "${MESH_CONVERSATION_JSON}" "${DAEMON_TO_MESH_CONTENT}"
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    payload = json.load(handle)
content = sys.argv[2]
ok = any(message.get("content") == content for message in payload.get("lxmf_messages", []))
print("yes" if ok else "no")
PY
    )"
    if [[ "${found}" == "yes" ]]; then
      DAEMON_TO_MESH_OK=1
      break
    fi
  fi
  sleep 1
done

if [[ "${DAEMON_TO_MESH_OK}" -ne 1 ]]; then
  echo "MeshChatX did not expose the daemon-originated message" >&2
  exit 1
fi

curl -fsS -X POST "http://${MESHCHAT_HOST}:${MESHCHAT_PORT}/api/v1/lxmf-messages/send" \
  -H "Content-Type: application/json" \
  -d "{\"delivery_method\":\"direct\",\"lxmf_message\":{\"destination_hash\":\"${DAEMON_HASH}\",\"content\":\"${MESH_TO_DAEMON_CONTENT}\"}}" >"${MESH_SEND_JSON}"

MESH_TO_DAEMON_OK=0
for _ in $(seq 1 60); do
  found="$(
    python3 - <<'PY' "${RET_DB}" "${MESH_TO_DAEMON_CONTENT}" "${MESH_HASH}" "${DAEMON_HASH}"
import sqlite3
import sys

db_path, content, source_hash, destination_hash = sys.argv[1:5]
conn = sqlite3.connect(db_path)
try:
    row = conn.execute(
        """
        SELECT id
        FROM messages
        WHERE direction = 'in'
          AND content = ?
          AND source = ?
          AND destination = ?
        ORDER BY timestamp DESC
        LIMIT 1
        """,
        (content, source_hash, destination_hash),
    ).fetchone()
    print("yes" if row else "no")
finally:
    conn.close()
PY
  )"
  if [[ "${found}" == "yes" ]]; then
    MESH_TO_DAEMON_OK=1
    break
  fi
  sleep 1
done

if [[ "${MESH_TO_DAEMON_OK}" -ne 1 ]]; then
  echo "reticulumd did not persist the MeshChatX reply" >&2
  exit 1
fi

python3 - <<'PY' \
  "${REPORT_PATH}" \
  "${TMP_ROOT}" \
  "${RET_LOG}" \
  "${MESH_LOG}" \
  "${DAEMON_HASH}" \
  "${MESH_HASH}" \
  "${DAEMON_TO_MESH_CONTENT}" \
  "${MESH_TO_DAEMON_CONTENT}"
import json
import os
import sys

(
    report_path,
    tmp_root,
    ret_log,
    mesh_log,
    daemon_hash,
    mesh_hash,
    daemon_to_mesh_content,
    mesh_to_daemon_content,
) = sys.argv[1:9]

report = {
    "status": "pass",
    "peer": "meshchatx",
    "daemon_hash": daemon_hash,
    "meshchatx_hash": mesh_hash,
    "proof": {
        "daemon_to_meshchatx": daemon_to_mesh_content,
        "meshchatx_to_daemon": mesh_to_daemon_content,
    },
    "artifacts": {
        "tmp_root": tmp_root,
        "reticulumd_log": ret_log,
        "meshchatx_log": mesh_log,
    },
}

os.makedirs(os.path.dirname(report_path), exist_ok=True)
with open(report_path, "w", encoding="utf-8") as handle:
    json.dump(report, handle, indent=2)
PY

echo "[meshchatx-reticulumd-smoke] pass"
echo "[meshchatx-reticulumd-smoke] report=${REPORT_PATH}"
echo "[meshchatx-reticulumd-smoke] logs=${TMP_ROOT}"
