#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RPC_ADDR="${RPC_ADDR:-127.0.0.1:4544}"
DB_DIR="${DB_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/lxmf-flutter-workspace.XXXXXX")}"
KEEP_DB="${KEEP_DB:-0}"
DAEMON_LOG="${DAEMON_LOG:-/tmp/lxmf-flutter-workspace-smoke.log}"

cleanup() {
  if [[ -n "${DAEMON_PID:-}" ]]; then
    kill "${DAEMON_PID}" >/dev/null 2>&1 || true
    wait "${DAEMON_PID}" 2>/dev/null || true
  fi
  if [[ "${KEEP_DB}" != "1" ]]; then
    rm -rf "${DB_DIR}"
  fi
}
trap cleanup EXIT

cd "${ROOT_DIR}"

echo "building reticulumd"
cargo build -p reticulumd --bin reticulumd --quiet

TARGET_DIR="${CARGO_TARGET_DIR:-$(
  cargo metadata --format-version=1 --no-deps \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])'
)}"
RETICULUMD_BIN="${TARGET_DIR}/debug/reticulumd"

echo "starting reticulumd on http://${RPC_ADDR}"
"${RETICULUMD_BIN}" \
  --rpc "${RPC_ADDR}" \
  --db "${DB_DIR}/reticulum.db" \
  --announce-interval-secs 0 \
  >"${DAEMON_LOG}" 2>&1 &
DAEMON_PID=$!

RPC_HOST="${RPC_ADDR%:*}"
RPC_PORT="${RPC_ADDR##*:}"

for _ in $(seq 1 100); do
  if python3 - "${RPC_HOST}" "${RPC_PORT}" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
sock = socket.socket()
sock.settimeout(0.2)
try:
    sock.connect((host, port))
except OSError:
    raise SystemExit(1)
finally:
    sock.close()
PY
  then
    break
  fi
  sleep 0.2
done

echo "running flutter workspace flows smoke"
SMOKE_OUTPUT=""
SMOKE_OK=0
for _ in $(seq 1 5); do
  if SMOKE_OUTPUT="$(
    cd wrappers/flutter/lxmf_sdk_app
    dart pub get >/dev/null
    dart run example/workspace_flows_smoke.dart "http://${RPC_ADDR}/rpc" 2>&1
  )"; then
    SMOKE_OK=1
    break
  fi
  sleep 0.5
done

echo "${SMOKE_OUTPUT}"

if [[ "${SMOKE_OK}" != "1" ]]; then
  echo "workspace smoke did not complete successfully" >&2
  exit 1
fi

grep -q 'peer=' <<<"${SMOKE_OUTPUT}"
grep -q 'topic=' <<<"${SMOKE_OUTPUT}"
grep -q 'report=' <<<"${SMOKE_OUTPUT}"
grep -q 'mission=' <<<"${SMOKE_OUTPUT}"

echo "workspace smoke complete"
