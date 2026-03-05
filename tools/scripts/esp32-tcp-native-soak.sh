#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

RUNS="${RUNS:-5}"
MODE="${MODE:-capture}"
TIMEOUT_SECS="${TIMEOUT_SECS:-30}"
BIND_ADDR="${BIND_ADDR:-0.0.0.0:7443}"
PAYLOAD="${PAYLOAD:-hello}"
CAPTURE_PROFILE="${CAPTURE_PROFILE:-default}"
LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/hil/soak}"
mkdir -p "${LOG_DIR}"

success=0
fail=0
total_bytes=0

echo "[esp32-tcp-native-soak] runs=${RUNS} mode=${MODE} bind=${BIND_ADDR} timeout=${TIMEOUT_SECS}s"

for ((i=1; i<=RUNS; i++)); do
  echo "[esp32-tcp-native-soak] run=${i}/${RUNS}"
  capture_out="${LOG_DIR}/capture-${i}.jpg"
  run_log="${LOG_DIR}/run-${i}.log"
  if [[ "${MODE}" == "capture" ]]; then
    if BIND_ADDR="${BIND_ADDR}" TIMEOUT_SECS="${TIMEOUT_SECS}" LISTENER_MODE="capture" CAPTURE_OUT="${capture_out}" CAPTURE_PROFILE="${CAPTURE_PROFILE}" \
      ./tools/scripts/esp32-tcp-native-smoke.sh >"${run_log}" 2>&1; then
      bytes=$(awk '/capture saved path=/{for(i=1;i<=NF;i++) if($i ~ /^bytes=/){sub("bytes=","",$i); print $i; exit}}' "${run_log}")
      bytes="${bytes:-0}"
      total_bytes=$((total_bytes + bytes))
      success=$((success + 1))
      echo "[esp32-tcp-native-soak] run=${i} status=ok bytes=${bytes} log=${run_log}"
    else
      fail=$((fail + 1))
      echo "[esp32-tcp-native-soak] run=${i} status=fail log=${run_log}" >&2
    fi
  else
    if BIND_ADDR="${BIND_ADDR}" TIMEOUT_SECS="${TIMEOUT_SECS}" LISTENER_MODE="lxmf-ping" PAYLOAD="${PAYLOAD}" \
      ./tools/scripts/esp32-tcp-native-smoke.sh >"${run_log}" 2>&1; then
      success=$((success + 1))
      echo "[esp32-tcp-native-soak] run=${i} status=ok log=${run_log}"
    else
      fail=$((fail + 1))
      echo "[esp32-tcp-native-soak] run=${i} status=fail log=${run_log}" >&2
    fi
  fi
 done

avg_bytes=0
if (( success > 0 )); then
  avg_bytes=$((total_bytes / success))
fi

echo "TCP_NATIVE_SOAK ok: runs=${RUNS} success=${success} fail=${fail} avg_bytes=${avg_bytes} mode=${MODE} log_dir=${LOG_DIR}"
if (( fail > 0 )); then
  exit 1
fi
