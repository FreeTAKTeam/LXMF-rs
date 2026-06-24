#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

SIDEBAND_ROOT="${SIDEBAND_ROOT:-${REPO_ROOT}/../Sideband}"
RPC_ADDR="${RPC_ADDR:-127.0.0.1:4243}"
TRANSPORT_ADDR="${TRANSPORT_ADDR:-127.0.0.1:37429}"
DAEMON_TO_SIDEBAND_CONTENT="${DAEMON_TO_SIDEBAND_CONTENT:-hello-from-reticulumd}"
SIDEBAND_TO_DAEMON_CONTENT="${SIDEBAND_TO_DAEMON_CONTENT:-reply-from-sideband}"
LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/interop/sideband-reticulumd}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
RUST_LOG="${RUST_LOG:-info}"

if [[ ! -d "${SIDEBAND_ROOT}" ]]; then
  echo "Sideband repo not found at ${SIDEBAND_ROOT}" >&2
  exit 1
fi

mkdir -p "${LOG_DIR}"
TMP_ROOT="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
RET_DB="${TMP_ROOT}/reticulum.db"
RET_LOG="${TMP_ROOT}/reticulumd.log"
SIDEBAND_CFG="${TMP_ROOT}/sideband"
SIDEBAND_RNS="${TMP_ROOT}/sideband-reticulum"
SIDEBAND_LOG="${TMP_ROOT}/sideband.log"
CONTROL_DIR="${TMP_ROOT}/control"
STATE_JSON="${CONTROL_DIR}/state.json"
mkdir -p "${SIDEBAND_CFG}" "${SIDEBAND_RNS}" "${CONTROL_DIR}/commands" "${CONTROL_DIR}/results"

cleanup() {
  local status=$?
  if [[ -n "${SIDEBAND_PID:-}" ]]; then
    kill "${SIDEBAND_PID}" >/dev/null 2>&1 || true
    wait "${SIDEBAND_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${RET_PID:-}" ]]; then
    kill "${RET_PID}" >/dev/null 2>&1 || true
    wait "${RET_PID}" >/dev/null 2>&1 || true
  fi
  if [[ ${status} -ne 0 ]]; then
    echo "[sideband-reticulumd-smoke] failed" >&2
    echo "[sideband-reticulumd-smoke] logs=${TMP_ROOT}" >&2
  fi
}
trap cleanup EXIT

write_command() {
  local target="$1"
  local payload="$2"
  local tmp="${target}.tmp"
  printf '%s\n' "${payload}" >"${tmp}"
  mv "${tmp}" "${target}"
}

cat > "${SIDEBAND_RNS}/config" <<EOF
[reticulum]
  enable_transport = no
  share_instance = no
  panic_on_interface_error = no

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
  RUST_LOG="${RUST_LOG}" \
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
  cd "${REPO_ROOT}"
  python3 tools/scripts/sideband_control.py serve \
    --sideband-root "${SIDEBAND_ROOT}" \
    --config-dir "${SIDEBAND_CFG}" \
    --rns-config-dir "${SIDEBAND_RNS}" \
    --control-dir "${CONTROL_DIR}" >"${SIDEBAND_LOG}" 2>&1
) &
SIDEBAND_PID=$!

for _ in $(seq 1 90); do
  if [[ -f "${STATE_JSON}" ]]; then
    break
  fi
  sleep 1
done

if [[ ! -f "${STATE_JSON}" ]]; then
  echo "Sideband control shim did not emit state.json" >&2
  exit 1
fi

SIDEBAND_HASH="$(
  python3 - <<'PY' "${STATE_JSON}"
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    print(json.load(handle)["lxmf_hash"])
PY
)"

if [[ -z "${SIDEBAND_HASH}" ]]; then
  echo "Failed to read Sideband LXMF hash" >&2
  exit 1
fi

sleep 3

"${REPO_ROOT}/target/debug/lxmf-cli" \
  --rpc "${RPC_ADDR}" \
  --output json \
  send \
  --source "${DAEMON_HASH}" \
  --destination "${SIDEBAND_HASH}" \
  --content "${DAEMON_TO_SIDEBAND_CONTENT}" >"${TMP_ROOT}/lxmf-send.json"

DAEMON_TO_SIDEBAND_OK=0
for _ in $(seq 1 60); do
  write_command \
    "${CONTROL_DIR}/commands/find-inbound.json" \
    "{\"command\":\"find_message\",\"context_hash\":\"${DAEMON_HASH}\",\"content\":\"${DAEMON_TO_SIDEBAND_CONTENT}\",\"direction\":\"inbound\"}"
  for _ in $(seq 1 30); do
    if [[ -f "${CONTROL_DIR}/results/find-inbound.json" ]]; then
      break
    fi
    sleep 0.1
  done
  if [[ -f "${CONTROL_DIR}/results/find-inbound.json" ]]; then
    found="$(
      python3 - <<'PY' "${CONTROL_DIR}/results/find-inbound.json"
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    payload = json.load(handle)
print("yes" if payload.get("ok") else "no")
PY
    )"
    rm -f "${CONTROL_DIR}/results/find-inbound.json"
    if [[ "${found}" == "yes" ]]; then
      DAEMON_TO_SIDEBAND_OK=1
      break
    fi
  fi
  sleep 1
done

if [[ "${DAEMON_TO_SIDEBAND_OK}" -ne 1 ]]; then
  echo "Sideband did not expose the daemon-originated message" >&2
  exit 1
fi

write_command \
  "${CONTROL_DIR}/commands/send-outbound.json" \
  "{\"command\":\"send\",\"destination_hash\":\"${DAEMON_HASH}\",\"content\":\"${SIDEBAND_TO_DAEMON_CONTENT}\",\"propagation\":false}"

for _ in $(seq 1 60); do
  if [[ -f "${CONTROL_DIR}/results/send-outbound.json" ]]; then
    break
  fi
  sleep 0.1
done

if [[ ! -f "${CONTROL_DIR}/results/send-outbound.json" ]]; then
  echo "Sideband send command did not complete" >&2
  exit 1
fi

SEND_OK="$(
  python3 - <<'PY' "${CONTROL_DIR}/results/send-outbound.json"
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    payload = json.load(handle)
print("yes" if payload.get("ok") else "no")
PY
)"
rm -f "${CONTROL_DIR}/results/send-outbound.json"
if [[ "${SEND_OK}" != "yes" ]]; then
  echo "Sideband did not accept the outbound send command" >&2
  exit 1
fi

SIDEBAND_TO_DAEMON_OK=0
for _ in $(seq 1 60); do
  found="$(
    python3 - <<'PY' "${RET_DB}" "${SIDEBAND_TO_DAEMON_CONTENT}" "${SIDEBAND_HASH}" "${DAEMON_HASH}"
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
    SIDEBAND_TO_DAEMON_OK=1
    break
  fi
  sleep 1
done

if [[ "${SIDEBAND_TO_DAEMON_OK}" -ne 1 ]]; then
  echo "reticulumd did not persist the Sideband-originated message" >&2
  exit 1
fi

python3 - <<'PY' "${REPORT_PATH}" "${TMP_ROOT}" "${RET_LOG}" "${SIDEBAND_LOG}" "${DAEMON_HASH}" "${SIDEBAND_HASH}" "${DAEMON_TO_SIDEBAND_CONTENT}" "${SIDEBAND_TO_DAEMON_CONTENT}"
import json
import os
import sys

report_path, tmp_root, ret_log, sideband_log, daemon_hash, sideband_hash, daemon_msg, sideband_msg = sys.argv[1:9]
payload = {
    "peer": "Sideband",
    "tmp_root": tmp_root,
    "reticulumd_log": ret_log,
    "sideband_log": sideband_log,
    "daemon_hash": daemon_hash,
    "external_client_hash": sideband_hash,
    "daemon_to_external_content": daemon_msg,
    "external_to_daemon_content": sideband_msg,
}
os.makedirs(os.path.dirname(report_path), exist_ok=True)
with open(report_path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle, indent=2, sort_keys=True)
PY

echo "[sideband-reticulumd-smoke] pass"
echo "[sideband-reticulumd-smoke] report=${REPORT_PATH}"
echo "[sideband-reticulumd-smoke] logs=${TMP_ROOT}"
