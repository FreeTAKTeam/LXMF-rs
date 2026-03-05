#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/hil}"
LOG_PATH="${LOG_DIR}/native-node.log"
REPORT_PATH="${LOG_DIR}/native-node-report.json"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"

mkdir -p "${LOG_DIR}"

cargo build -p rns-tools --bin rnx --quiet
cargo build -p reticulumd --bin reticulumd --quiet

set +e
"${REPO_ROOT}/target/debug/rnx" e2e --timeout-secs "${TIMEOUT_SECS}" --mode direct >"${LOG_PATH}" 2>&1
status=$?
set -e

announce_ok=false
tiny_message_ok=false
if grep -q "E2E ok: peer discovery A<->B succeeded" "${LOG_PATH}"; then
  announce_ok=true
fi
if grep -q "E2E ok: mode=direct message" "${LOG_PATH}"; then
  tiny_message_ok=true
fi

if [[ $status -eq 0 && "${announce_ok}" == "true" && "${tiny_message_ok}" == "true" ]]; then
  run_status="pass"
else
  run_status="fail"
fi

cat > "${REPORT_PATH}" <<EOF
{"status":"${run_status}","announce_ok":${announce_ok},"tiny_message_ok":${tiny_message_ok},"exit_code":${status}}
EOF

if [[ "${run_status}" != "pass" ]]; then
  echo "[embedded-native-interop-smoke] failed; see ${LOG_PATH}" >&2
  exit 1
fi

echo "[embedded-native-interop-smoke] pass"
echo "[embedded-native-interop-smoke] report=${REPORT_PATH}"
echo "[embedded-native-interop-smoke] log=${LOG_PATH}"
