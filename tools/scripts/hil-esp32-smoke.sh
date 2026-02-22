#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

LOG_DIR="target/hil"
LOG_PATH="${LOG_DIR}/esp32-smoke.log"
REPORT_PATH="${LOG_DIR}/esp32-smoke-report.json"

HIL_SERIAL_PORT="${HIL_SERIAL_PORT:-}"
HIL_RPC_ENDPOINT="${HIL_RPC_ENDPOINT:-127.0.0.1:4242}"
HIL_SEND_SOURCE="${HIL_SEND_SOURCE:-}"
HIL_SEND_DESTINATION="${HIL_SEND_DESTINATION:-}"
HIL_TICK_MAX_WORK_ITEMS="${HIL_TICK_MAX_WORK_ITEMS:-32}"
HIL_TICK_MAX_DURATION_MS="${HIL_TICK_MAX_DURATION_MS:-25}"

mkdir -p "$LOG_DIR"
: >"$LOG_PATH"

log() {
  echo "$*" | tee -a "$LOG_PATH"
}

fail() {
  local msg="$1"
  log "ERROR: ${msg}"
  cat >"$REPORT_PATH" <<JSON
{"status":"fail","reason":"${msg}","log_path":"${LOG_PATH}"}
JSON
  exit 1
}

if [[ -z "$HIL_SERIAL_PORT" ]]; then
  fail "HIL_SERIAL_PORT is required"
fi
if [[ ! -e "$HIL_SERIAL_PORT" ]]; then
  fail "serial port not found at ${HIL_SERIAL_PORT}"
fi
if [[ -z "$HIL_SEND_SOURCE" ]]; then
  fail "HIL_SEND_SOURCE is required"
fi
if [[ -z "$HIL_SEND_DESTINATION" ]]; then
  fail "HIL_SEND_DESTINATION is required"
fi

run_lxmf() {
  local label="$1"
  shift
  log "== ${label} =="
  log "command: lxmf $*"
  local output
  if ! output="$(cargo run --quiet -p lxmf-cli -- "$@" 2>&1)"; then
    log "$output"
    fail "${label} command failed"
  fi
  log "$output"
  if ! grep -q '"ok":true' <<<"$output"; then
    fail "${label} did not return ok=true"
  fi
}

common_args=(
  --rpc "$HIL_RPC_ENDPOINT"
  --profile embedded-alloc
  --max-poll-events 32
  --max-event-bytes 8192
  --max-batch-bytes 262144
  --idempotency-ttl-ms 7200000
  --output json
)

log "ESP32 HIL smoke start"
log "serial_port=${HIL_SERIAL_PORT}"
log "rpc_endpoint=${HIL_RPC_ENDPOINT}"

run_lxmf \
  "start" \
  "${common_args[@]}" \
  start

run_lxmf \
  "send" \
  "${common_args[@]}" \
  send \
  --source "$HIL_SEND_SOURCE" \
  --destination "$HIL_SEND_DESTINATION" \
  --content "esp32-hil-smoke"

run_lxmf \
  "tick" \
  "${common_args[@]}" \
  tick \
  --max-work-items "$HIL_TICK_MAX_WORK_ITEMS" \
  --max-duration-ms "$HIL_TICK_MAX_DURATION_MS"

run_lxmf \
  "poll" \
  "${common_args[@]}" \
  poll \
  --max 16

run_lxmf \
  "snapshot" \
  "${common_args[@]}" \
  snapshot

run_lxmf \
  "shutdown" \
  "${common_args[@]}" \
  shutdown \
  --mode graceful

cat >"$REPORT_PATH" <<JSON
{"status":"pass","serial_port":"${HIL_SERIAL_PORT}","rpc_endpoint":"${HIL_RPC_ENDPOINT}","log_path":"${LOG_PATH}"}
JSON

log "ESP32 HIL smoke success"
log "report=${REPORT_PATH}"
