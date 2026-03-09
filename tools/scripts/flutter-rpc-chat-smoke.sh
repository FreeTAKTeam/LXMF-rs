#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PEER_HASH="${1:-0123456789abcdef0123456789abcdef}"
RPC_ADDR="${RPC_ADDR:-127.0.0.1:4543}"
DB_DIR="${DB_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/lxmf-flutter-rpc-chat.XXXXXX")}"
KEEP_DB="${KEEP_DB:-0}"

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

echo "starting reticulumd on http://${RPC_ADDR}"
cargo run -p reticulumd --bin reticulumd -- \
  --rpc "${RPC_ADDR}" \
  --db "${DB_DIR}/reticulum.db" \
  --announce-interval-secs 0 \
  >/tmp/lxmf-flutter-rpc-chat-smoke.log 2>&1 &
DAEMON_PID=$!

RPC_HOST="${RPC_ADDR%:*}"
RPC_PORT="${RPC_ADDR##*:}"

for _ in $(seq 1 50); do
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

echo "running flutter rpc chat smoke"
(
  cd wrappers/flutter/lxmf_sdk_app
  dart run example/rpc_chat_smoke.dart "http://${RPC_ADDR}/rpc" "${PEER_HASH}"
)

echo "smoke complete"
