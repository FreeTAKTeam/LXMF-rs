#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/hil}"
mkdir -p "${LOG_DIR}"
LISTENER_MODE="${LISTENER_MODE:-passive}"
CAPTURE_OUT="${CAPTURE_OUT:-}"
CAPTURE_DIR_DEFAULT="${LOG_DIR}/captures"
if [[ "${LISTENER_MODE}" == "capture" && -z "${CAPTURE_OUT}" ]]; then
  mkdir -p "${CAPTURE_DIR_DEFAULT}"
  CAPTURE_OUT="${CAPTURE_DIR_DEFAULT}/capture-$(date +%s).jpg"
fi

LISTENER_LOG="${LOG_DIR}/esp32-tcp-native-listener.log"
BIND_ADDR="${BIND_ADDR:-0.0.0.0:7443}"
TIMEOUT_SECS="${TIMEOUT_SECS:-20}"
RUNTIME_SEQ="${RUNTIME_SEQ:-}"
PAYLOAD="${PAYLOAD:-ping}"
SOURCE_HEX="${SOURCE_HEX:-99999999999999999999999999999999}"
DESTINATION_HEX="${DESTINATION_HEX:-22222222222222222222222222222222}"
EXPECT_MIN_RESPONSES="${EXPECT_MIN_RESPONSES:-}"
CAPTURE_PROFILE="${CAPTURE_PROFILE:-default}"

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
if [[ "${LISTENER_MODE}" != "passive" ]]; then
  cmd+=(--payload "${PAYLOAD}")
fi
if [[ "${LISTENER_MODE}" == "lxmf-ping" ]]; then
  cmd+=(--source-hex "${SOURCE_HEX}" --destination-hex "${DESTINATION_HEX}")
fi
if [[ "${LISTENER_MODE}" == "capture" && -n "${CAPTURE_OUT}" ]]; then
  cmd+=(--capture-out "${CAPTURE_OUT}" --capture-profile "${CAPTURE_PROFILE}")
fi

if [[ -z "${EXPECT_MIN_RESPONSES}" ]]; then
  if [[ "${LISTENER_MODE}" == "passive" ]]; then
    EXPECT_MIN_RESPONSES=0
  else
    EXPECT_MIN_RESPONSES=1
  fi
fi

echo "[esp32-tcp-native-smoke] listening bind=${BIND_ADDR} mode=${LISTENER_MODE} expect_min_responses=${EXPECT_MIN_RESPONSES}"
echo "[esp32-tcp-native-smoke] log=${LISTENER_LOG}"
"${cmd[@]}" | tee "${LISTENER_LOG}"

if ! grep -q "TCP_NATIVE_LISTENER ok:" "${LISTENER_LOG}"; then
  echo "[esp32-tcp-native-smoke] missing success marker; see ${LISTENER_LOG}" >&2
  exit 1
fi

responses="$(awk '
  /TCP_NATIVE_LISTENER ok:/ {
    for (i = 1; i <= NF; i++) {
      if ($i ~ /^responses=/) {
        sub("responses=", "", $i);
        print $i;
        exit;
      }
    }
  }
' "${LISTENER_LOG}")"

responses="${responses:-0}"
if [[ "${responses}" =~ ^[0-9]+$ ]] && (( responses >= EXPECT_MIN_RESPONSES )); then
  echo "[esp32-tcp-native-smoke] pass"
else
  echo "[esp32-tcp-native-smoke] expected at least ${EXPECT_MIN_RESPONSES} responses, got ${responses}; see ${LISTENER_LOG}" >&2
  exit 1
fi
