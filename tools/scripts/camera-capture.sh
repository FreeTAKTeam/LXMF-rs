#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

BACKEND="${BACKEND:-auto}"
NAME_HINT="${NAME_HINT:-LXMF}"
DEVICE_ID="${DEVICE_ID:-}"
SERVICE_UUID="${SERVICE_UUID:-12345678-1234-1234-1234-1234567890ab}"
WRITE_CHAR_UUID="${WRITE_CHAR_UUID:-12345678-1234-1234-1234-1234567890ac}"
NOTIFY_CHAR_UUID="${NOTIFY_CHAR_UUID:-12345678-1234-1234-1234-1234567890ad}"
SCAN_SECS="${SCAN_SECS:-15}"
TIMEOUT_SECS="${TIMEOUT_SECS:-30}"
ROUNDS="${ROUNDS:-3}"
MAX_PROBES="${MAX_PROBES:-25}"
PERMISSIVE_SCAN="${PERMISSIVE_SCAN:-1}"
LOG_LEVEL="${LOG_LEVEL:-info}"
OUT="${OUT:-/tmp/lxmf-capture.bin}"
UPLOAD="${UPLOAD:-0}"
RPC_ADDR="${RPC_ADDR:-127.0.0.1:4243}"
RNS_BIN="${RNS_BIN:-${REPO_ROOT}/target/debug/rnx}"
CHUNK_SIZE="${CHUNK_SIZE:-8192}"
CONTENT_TYPE="${CONTENT_TYPE:-image/jpeg}"
CAPTURE_TRACE="${CAPTURE_TRACE:-1}"

timestamp() {
  date '+%H:%M:%S'
}

trace() {
  if [[ "${CAPTURE_TRACE}" != "1" ]]; then
    return
  fi
  echo "[capture $(timestamp)] $*"
}

print_cmd() {
  local rendered=""
  for arg in "$@"; do
    if [[ -n "${rendered}" ]]; then
      rendered+=" "
    fi
    rendered+="$(printf '%q' "${arg}")"
  done
  echo "${rendered}"
}

usage() {
  cat <<USAGE
Usage: ./tools/scripts/camera-capture.sh [options]

Options:
  --backend auto|bleak|rust
  --device-id <id>
  --name-hint <hint>
  --service-uuid <uuid>
  --write-char-uuid <uuid>
  --notify-char-uuid <uuid>
  --scan-secs <n>
  --timeout-secs <n>
  --rounds <n>
  --max-probes <n>
  --permissive-scan 0|1
  --log-level debug|info|warn|error
  --out <path>
  --upload 0|1
  --rpc <host:port>
  --chunk-size <n>
  --content-type <mime>

Result line format:
  CAMERA_CAPTURE_RESULT status=ok backend=<backend> bytes=<n> output_file=<path> [attachment_id=<id>]
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --backend) BACKEND="$2"; shift 2 ;;
    --device-id) DEVICE_ID="$2"; shift 2 ;;
    --name-hint) NAME_HINT="$2"; shift 2 ;;
    --service-uuid) SERVICE_UUID="$2"; shift 2 ;;
    --write-char-uuid) WRITE_CHAR_UUID="$2"; shift 2 ;;
    --notify-char-uuid) NOTIFY_CHAR_UUID="$2"; shift 2 ;;
    --scan-secs) SCAN_SECS="$2"; shift 2 ;;
    --timeout-secs) TIMEOUT_SECS="$2"; shift 2 ;;
    --rounds) ROUNDS="$2"; shift 2 ;;
    --max-probes) MAX_PROBES="$2"; shift 2 ;;
    --permissive-scan) PERMISSIVE_SCAN="$2"; shift 2 ;;
    --log-level) LOG_LEVEL="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --upload) UPLOAD="$2"; shift 2 ;;
    --rpc) RPC_ADDR="$2"; shift 2 ;;
    --chunk-size) CHUNK_SIZE="$2"; shift 2 ;;
    --content-type) CONTENT_TYPE="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

select_backend() {
  if [[ "${BACKEND}" != "auto" ]]; then
    echo "${BACKEND}"
    return
  fi
  case "$(uname -s)" in
    Darwin) echo "bleak" ;;
    *) echo "rust" ;;
  esac
}

ACTIVE_BACKEND="$(select_backend)"
trace "resolved backend=${ACTIVE_BACKEND} requested_backend=${BACKEND} out=${OUT} upload=${UPLOAD} log_level=${LOG_LEVEL}"
trace "config name_hint=${NAME_HINT} device_id=${DEVICE_ID:-<none>} service_uuid=${SERVICE_UUID} write_char_uuid=${WRITE_CHAR_UUID} notify_char_uuid=${NOTIFY_CHAR_UUID} scan_secs=${SCAN_SECS} timeout_secs=${TIMEOUT_SECS} rounds=${ROUNDS} max_probes=${MAX_PROBES} permissive_scan=${PERMISSIVE_SCAN}"

if [[ "${ACTIVE_BACKEND}" == "bleak" ]]; then
  PYTHON_BIN="${PYTHON_BIN:-python3}"
  BLEAK_SCRIPT="${REPO_ROOT}/tools/experimental/esp32_camera_capture_bleak.py"
  cmd=("${PYTHON_BIN}" "${BLEAK_SCRIPT}"
    --name-hint "${NAME_HINT}"
    --service-uuid "${SERVICE_UUID}"
    --write-char-uuid "${WRITE_CHAR_UUID}"
    --notify-char-uuid "${NOTIFY_CHAR_UUID}"
    --scan-secs "${SCAN_SECS}"
    --timeout-secs "${TIMEOUT_SECS}"
    --rounds "${ROUNDS}"
    --max-probes "${MAX_PROBES}"
    --log-level "${LOG_LEVEL}"
    --out "${OUT}")
  if [[ -n "${DEVICE_ID}" ]]; then
    cmd+=(--device-id "${DEVICE_ID}")
  fi
  if [[ "${PERMISSIVE_SCAN}" == "1" ]]; then
    cmd+=(--permissive-scan)
  fi
  if [[ "${UPLOAD}" == "1" ]]; then
    cmd+=(--upload --rnx "${RNS_BIN}" --rpc "${RPC_ADDR}" --chunk-size "${CHUNK_SIZE}" --content-type "${CONTENT_TYPE}")
  fi

  trace "executing bleak backend"
  trace "command=$(print_cmd "${cmd[@]}")"
  start_epoch="$(python3 - <<'PY'
import time
print(f"{time.time():.6f}")
PY
)"
  output="$(${cmd[@]} 2>&1)"
  end_epoch="$(python3 - <<'PY'
import time
print(f"{time.time():.6f}")
PY
)"
  echo "${output}"
  if ! grep -q "BLE_CAPTURE ok:" <<<"${output}"; then
    trace "bleak backend failed"
    echo "CAMERA_CAPTURE_RESULT status=error backend=bleak" >&2
    exit 1
  fi
  bytes="$(sed -n 's/.*BLE_CAPTURE ok: bytes=\([0-9][0-9]*\).*/\1/p' <<<"${output}" | tail -n 1)"
  attachment_id="$(sed -n 's/.*attachment_id=\([^[:space:]]\+\).*/\1/p' <<<"${output}" | tail -n 1)"
  if [[ -f "${OUT}" ]]; then
    file_bytes="$(wc -c < "${OUT}" | tr -d '[:space:]')"
    trace "output file ready path=${OUT} bytes=${file_bytes}"
  else
    trace "expected output file missing path=${OUT}"
  fi
  duration_ms="$(python3 - <<PY
start = float("${start_epoch}")
end = float("${end_epoch}")
print(int((end - start) * 1000))
PY
)"
  trace "bleak backend completed bytes=${bytes} duration_ms=${duration_ms}"
  if [[ -n "${attachment_id}" ]]; then
    echo "CAMERA_CAPTURE_RESULT status=ok backend=bleak bytes=${bytes} output_file=${OUT} attachment_id=${attachment_id}"
  else
    echo "CAMERA_CAPTURE_RESULT status=ok backend=bleak bytes=${bytes} output_file=${OUT}"
  fi
  exit 0
fi

if [[ "${ACTIVE_BACKEND}" == "rust" ]]; then
  if [[ -z "${DEVICE_ID}" ]]; then
    echo "rust backend requires --device-id" >&2
    echo "CAMERA_CAPTURE_RESULT status=error backend=rust" >&2
    exit 2
  fi
  trace "executing rust backend"
  trace "command=$(print_cmd "${RNS_BIN}" camera-capture-upload --rpc "${RPC_ADDR}" --peripheral-id "${DEVICE_ID}" --service-uuid "${SERVICE_UUID}" --write-char-uuid "${WRITE_CHAR_UUID}" --notify-char-uuid "${NOTIFY_CHAR_UUID}" --content-type "${CONTENT_TYPE}" --chunk-size "${CHUNK_SIZE}" --timeout-secs "${TIMEOUT_SECS}")"
  start_epoch="$(python3 - <<'PY'
import time
print(f"{time.time():.6f}")
PY
)"
  output="$(${RNS_BIN} camera-capture-upload \
    --rpc "${RPC_ADDR}" \
    --peripheral-id "${DEVICE_ID}" \
    --service-uuid "${SERVICE_UUID}" \
    --write-char-uuid "${WRITE_CHAR_UUID}" \
    --notify-char-uuid "${NOTIFY_CHAR_UUID}" \
    --content-type "${CONTENT_TYPE}" \
    --chunk-size "${CHUNK_SIZE}" \
    --timeout-secs "${TIMEOUT_SECS}" 2>&1)"
  end_epoch="$(python3 - <<'PY'
import time
print(f"{time.time():.6f}")
PY
)"
  echo "${output}"
  if ! grep -q "CAMERA_CAPTURE_UPLOAD ok:" <<<"${output}"; then
    trace "rust backend failed"
    echo "CAMERA_CAPTURE_RESULT status=error backend=rust" >&2
    exit 1
  fi
  bytes="$(sed -n 's/.*bytes=\([0-9][0-9]*\).*/\1/p' <<<"${output}" | tail -n 1)"
  attachment_id="$(sed -n 's/.*attachment_id=\([^[:space:]]\+\).*/\1/p' <<<"${output}" | tail -n 1)"
  duration_ms="$(python3 - <<PY
start = float("${start_epoch}")
end = float("${end_epoch}")
print(int((end - start) * 1000))
PY
)"
  trace "rust backend completed bytes=${bytes} duration_ms=${duration_ms} attachment_id=${attachment_id}"
  echo "CAMERA_CAPTURE_RESULT status=ok backend=rust bytes=${bytes} output_file=<uploaded> attachment_id=${attachment_id}"
  exit 0
fi

echo "unsupported backend: ${ACTIVE_BACKEND}" >&2
exit 2
