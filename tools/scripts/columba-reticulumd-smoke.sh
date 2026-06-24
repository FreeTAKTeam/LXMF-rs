#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

COLUMBA_ROOT="${COLUMBA_ROOT:-${REPO_ROOT}/../columba}"
RPC_ADDR="${RPC_ADDR:-127.0.0.1:4443}"
TRANSPORT_ADDR="${TRANSPORT_ADDR:-127.0.0.1:39429}"
DAEMON_TO_COLUMBA_CONTENT="${DAEMON_TO_COLUMBA_CONTENT:-hello-from-reticulumd}"
COLUMBA_TO_DAEMON_CONTENT="${COLUMBA_TO_DAEMON_CONTENT:-reply-from-columba}"
LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/interop/columba-reticulumd}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
RUST_LOG="${RUST_LOG:-info}"

if [[ ! -d "${COLUMBA_ROOT}" ]]; then
  echo "Columba repo not found at ${COLUMBA_ROOT}" >&2
  exit 1
fi

mkdir -p "${LOG_DIR}"
TMP_ROOT="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
RET_DB="${TMP_ROOT}/reticulum.db"
RET_LOG="${TMP_ROOT}/reticulumd.log"
COLUMBA_STORAGE="${TMP_ROOT}/columba"
COLUMBA_LOG="${TMP_ROOT}/columba.log"
CONTROL_DIR="${TMP_ROOT}/control"
STATE_JSON="${CONTROL_DIR}/state.json"
mkdir -p "${COLUMBA_STORAGE}" "${CONTROL_DIR}/commands" "${CONTROL_DIR}/results"

cleanup() {
  local status=$?
  if [[ -n "${COLUMBA_PID:-}" ]]; then
    kill "${COLUMBA_PID}" >/dev/null 2>&1 || true
    wait "${COLUMBA_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${RET_PID:-}" ]]; then
    kill "${RET_PID}" >/dev/null 2>&1 || true
    wait "${RET_PID}" >/dev/null 2>&1 || true
  fi
  if [[ ${status} -ne 0 ]]; then
    echo "[columba-reticulumd-smoke] failed" >&2
    echo "[columba-reticulumd-smoke] logs=${TMP_ROOT}" >&2
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
  python3 tools/scripts/columba_control.py serve \
    --columba-root "${COLUMBA_ROOT}" \
    --storage-dir "${COLUMBA_STORAGE}" \
    --control-dir "${CONTROL_DIR}" \
    --transport-host "${TRANSPORT_ADDR%:*}" \
    --transport-port "${TRANSPORT_ADDR##*:}" >"${COLUMBA_LOG}" 2>&1
) &
COLUMBA_PID=$!

for _ in $(seq 1 90); do
  if [[ -f "${STATE_JSON}" ]]; then
    break
  fi
  sleep 1
done

if [[ ! -f "${STATE_JSON}" ]]; then
  echo "Columba control shim did not emit state.json" >&2
  exit 1
fi

COLUMBA_HASH="$(
  python3 - <<'PY' "${STATE_JSON}"
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    print(json.load(handle)["lxmf_hash"])
PY
)"

if [[ -z "${COLUMBA_HASH}" ]]; then
  echo "Failed to read Columba LXMF hash" >&2
  exit 1
fi

sleep 3

"${REPO_ROOT}/target/debug/lxmf-cli" \
  --rpc "${RPC_ADDR}" \
  --output json \
  send \
  --source "${DAEMON_HASH}" \
  --destination "${COLUMBA_HASH}" \
  --content "${DAEMON_TO_COLUMBA_CONTENT}" >"${TMP_ROOT}/lxmf-send.json"

DAEMON_TO_COLUMBA_OK=0
for _ in $(seq 1 60); do
  write_command \
    "${CONTROL_DIR}/commands/find-inbound.json" \
    "{\"command\":\"find_message\",\"context_hash\":\"${DAEMON_HASH}\",\"content\":\"${DAEMON_TO_COLUMBA_CONTENT}\",\"direction\":\"inbound\"}"
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
      DAEMON_TO_COLUMBA_OK=1
      break
    fi
  fi
  sleep 1
done

if [[ "${DAEMON_TO_COLUMBA_OK}" -ne 1 ]]; then
  echo "Columba did not expose the daemon-originated message" >&2
  exit 1
fi

write_command \
  "${CONTROL_DIR}/commands/send-outbound.json" \
  "{\"command\":\"send\",\"destination_hash\":\"${DAEMON_HASH}\",\"content\":\"${COLUMBA_TO_DAEMON_CONTENT}\"}"

for _ in $(seq 1 60); do
  if [[ -f "${CONTROL_DIR}/results/send-outbound.json" ]]; then
    break
  fi
  sleep 0.1
done

if [[ ! -f "${CONTROL_DIR}/results/send-outbound.json" ]]; then
  echo "Columba send command did not complete" >&2
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
  echo "Columba did not accept the outbound send command" >&2
  exit 1
fi

COLUMBA_TO_DAEMON_OK=0
for _ in $(seq 1 60); do
  found="$(
    python3 - <<'PY' "${RET_DB}" "${COLUMBA_TO_DAEMON_CONTENT}" "${COLUMBA_HASH}" "${DAEMON_HASH}"
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
    COLUMBA_TO_DAEMON_OK=1
    break
  fi
  sleep 1
done

if [[ "${COLUMBA_TO_DAEMON_OK}" -ne 1 ]]; then
  echo "reticulumd did not persist the Columba-originated message" >&2
  exit 1
fi

python3 - <<'PY' "${REPORT_PATH}" "${TMP_ROOT}" "${RET_LOG}" "${COLUMBA_LOG}" "${DAEMON_HASH}" "${COLUMBA_HASH}" "${DAEMON_TO_COLUMBA_CONTENT}" "${COLUMBA_TO_DAEMON_CONTENT}"
import json
import os
import sys

report_path, tmp_root, ret_log, columba_log, daemon_hash, columba_hash, daemon_msg, columba_msg = sys.argv[1:9]
payload = {
    "peer": "Columba",
    "tmp_root": tmp_root,
    "reticulumd_log": ret_log,
    "columba_log": columba_log,
    "daemon_hash": daemon_hash,
    "external_client_hash": columba_hash,
    "daemon_to_external_content": daemon_msg,
    "external_to_daemon_content": columba_msg,
}
os.makedirs(os.path.dirname(report_path), exist_ok=True)
with open(report_path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle, indent=2, sort_keys=True)
PY

echo "[columba-reticulumd-smoke] pass"
echo "[columba-reticulumd-smoke] report=${REPORT_PATH}"
echo "[columba-reticulumd-smoke] logs=${TMP_ROOT}"
