#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

PYTHON_BIN="${PYTHON_BIN:-python3}"
LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/interop/python-lxmd-rust-lxmd}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
TIMEOUT_SECS="${TIMEOUT_SECS:-45}"

RUST_RPC_ADDR="${RUST_RPC_ADDR:-127.0.0.1:4243}"
RUST_TRANSPORT_ADDR="${RUST_TRANSPORT_ADDR:-127.0.0.1:37429}"
RUST_TRANSPORT_HOST="${RUST_TRANSPORT_ADDR%:*}"
RUST_TRANSPORT_PORT="${RUST_TRANSPORT_ADDR##*:}"

PY_SHARED_INSTANCE_PORT="${PY_SHARED_INSTANCE_PORT:-$((39428 + ($$ % 200)))}"
PY_INSTANCE_CONTROL_PORT="${PY_INSTANCE_CONTROL_PORT:-$((PY_SHARED_INSTANCE_PORT + 1))}"

require_python_modules() {
  "${PYTHON_BIN}" - <<'PY' >/dev/null
import importlib.util
for module in ("RNS", "LXMF"):
    if importlib.util.find_spec(module) is None:
        raise SystemExit(f"missing Python module: {module}")
PY
}

wait_for_file_pattern() {
  local file="$1"
  local pattern="$2"
  local timeout="$3"
  local start
  start="$(date +%s)"
  while true; do
    if [[ -f "${file}" ]] && grep -Eq "${pattern}" "${file}"; then
      return 0
    fi
    if (( "$(date +%s)" - start >= timeout )); then
      return 1
    fi
    sleep 1
  done
}

extract_hash() {
  local file="$1"
  local marker="$2"
  "${PYTHON_BIN}" - <<'PY' "${file}" "${marker}"
import re
import sys
from pathlib import Path

path = Path(sys.argv[1])
marker = sys.argv[2]
pattern = re.compile(r"([0-9a-f]{32})", re.IGNORECASE)

for line in path.read_text(encoding="utf-8", errors="ignore").splitlines():
    if marker in line:
        match = pattern.search(line)
        if match:
            print(match.group(1).lower())
            raise SystemExit(0)

raise SystemExit(1)
PY
}

destination_hash_from_identity() {
  local identity_path="$1"
  local aspect_one="$2"
  local aspect_two="$3"
  local aspect_three="${4:-}"
  "${PYTHON_BIN}" - <<'PY' "${identity_path}" "${aspect_one}" "${aspect_two}" "${aspect_three}"
import os
import sys
import tempfile

import RNS

identity_path, aspect_one, aspect_two, aspect_three = sys.argv[1:5]
cfg = tempfile.mkdtemp(prefix="rns-hash-")
with open(os.path.join(cfg, "config"), "w", encoding="utf-8") as handle:
    handle.write(
        "[reticulum]\n"
        "share_instance = no\n"
        "enable_transport = no\n"
        "discover_interfaces = false\n"
        "autoconnect_discovered_interfaces = 0\n"
    )

RNS.Reticulum(configdir=cfg, loglevel=0)
identity = RNS.Identity.from_file(identity_path)
if identity is None:
    raise SystemExit(f"failed to load identity from {identity_path}")

aspects = [aspect_one, aspect_two]
if aspect_three:
    aspects.append(aspect_three)

destination = RNS.Destination(identity, RNS.Destination.IN, RNS.Destination.SINGLE, *aspects)
print(RNS.hexrep(destination.hash, delimit=False).lower())
PY
}

identity_hash_from_file() {
  local identity_path="$1"
  "${PYTHON_BIN}" - <<'PY' "${identity_path}"
import os
import sys
import tempfile

import RNS

identity_path = sys.argv[1]
cfg = tempfile.mkdtemp(prefix="rns-ident-")
with open(os.path.join(cfg, "config"), "w", encoding="utf-8") as handle:
    handle.write(
        "[reticulum]\n"
        "share_instance = no\n"
        "enable_transport = no\n"
        "discover_interfaces = false\n"
        "autoconnect_discovered_interfaces = 0\n"
    )

RNS.Reticulum(configdir=cfg, loglevel=0)
identity = RNS.Identity.from_file(identity_path)
if identity is None:
    raise SystemExit(f"failed to load identity from {identity_path}")
print(RNS.hexrep(identity.hash, delimit=False).lower())
PY
}

assert_contains() {
  local file="$1"
  local pattern="$2"
  local description="$3"
  if ! grep -Eq "${pattern}" "${file}"; then
    echo "missing expected output: ${description}" >&2
    echo "looked for pattern '${pattern}' in ${file}" >&2
    return 1
  fi
}

mkdir -p "${LOG_DIR}"
TMP_ROOT="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"

RUST_DIR="${TMP_ROOT}/rust-lxmd"
PY_DIR="${TMP_ROOT}/python-lxmd"
PY_RNS_DIR="${TMP_ROOT}/python-rns"
PY_SENDER_DIR="${TMP_ROOT}/python-sender"
PY_SENDER_RNS_DIR="${TMP_ROOT}/python-sender-rns"
HOOK_STATE_DIR="${TMP_ROOT}/hook-state"

RUST_LOG="${TMP_ROOT}/rust-lxmd.log"
PY_LOG="${TMP_ROOT}/python-lxmd.log"
PY_REMOTE_STATUS_LOG="${TMP_ROOT}/python-remote-status.log"
RUST_REMOTE_STATUS_LOG="${TMP_ROOT}/rust-remote-status.log"
PY_SEND_LOG="${TMP_ROOT}/python-send.json"
HOOK_LOG="${HOOK_STATE_DIR}/hook.log"

cleanup() {
  local status=$?
  if [[ -n "${PY_PID:-}" ]]; then
    kill "${PY_PID}" >/dev/null 2>&1 || true
    wait "${PY_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${RUST_PID:-}" ]]; then
    kill "${RUST_PID}" >/dev/null 2>&1 || true
    wait "${RUST_PID}" >/dev/null 2>&1 || true
  fi
  if [[ ${status} -ne 0 ]]; then
    echo "[python-lxmd-rust-lxmd-smoke] failed" >&2
    echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}" >&2
  fi
}
trap cleanup EXIT

require_python_modules

mkdir -p "${RUST_DIR}" "${PY_DIR}" "${PY_RNS_DIR}" "${PY_SENDER_DIR}" "${PY_SENDER_RNS_DIR}" "${HOOK_STATE_DIR}"

PY_CONTROL_IDENTITY_HASH="$("${PYTHON_BIN}" - <<'PY' "${PY_DIR}/identity"
import sys
import RNS

path = sys.argv[1]
identity = RNS.Identity()
identity.to_file(path)
print(RNS.hexrep(identity.hash, delimit=False).lower())
PY
)"

cat > "${RUST_DIR}/launcher.toml" <<EOF
[lxmd]
rpc = "${RUST_RPC_ADDR}"
transport = "${RUST_TRANSPORT_ADDR}"
propagation_node = true
service = true
EOF

cat > "${RUST_DIR}/config" <<EOF
[propagation]
enable_node = yes
announce_at_start = yes
announce_interval = 1
autopeer = yes
autopeer_maxdepth = 6
control_allowed = ${PY_CONTROL_IDENTITY_HASH}

[lxmf]
display_name = Rust Smoke Node
announce_at_start = yes
announce_interval = 1
on_inbound = ${RUST_DIR}/on_inbound.sh

[logging]
loglevel = 4
EOF

cat > "${RUST_DIR}/on_inbound.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
message_file="${1:-}"
state_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../hook-state && pwd)"
mkdir -p "${state_dir}"
{
  printf 'message_file=%s\n' "${message_file}"
  printf 'source=%s\n' "${LXMD_MESSAGE_SOURCE:-}"
  printf 'destination=%s\n' "${LXMD_MESSAGE_DESTINATION:-}"
  printf 'title=%s\n' "${LXMD_MESSAGE_TITLE:-}"
  printf 'content=%s\n' "${LXMD_MESSAGE_CONTENT:-}"
} >> "${state_dir}/hook.log"
EOF
chmod +x "${RUST_DIR}/on_inbound.sh"

RUST_CONTROL_IDENTITY_HASH=""

cat > "${PY_RNS_DIR}/config" <<EOF
[reticulum]
  enable_transport = true
  share_instance = yes
  shared_instance_port = ${PY_SHARED_INSTANCE_PORT}
  instance_control_port = ${PY_INSTANCE_CONTROL_PORT}
  discover_interfaces = false
  autoconnect_discovered_interfaces = 0

[logging]
  loglevel = 4

[interfaces]
  [[Rust LXMD]]
    type = TCPClientInterface
    enabled = yes
    target_host = ${RUST_TRANSPORT_HOST}
    target_port = ${RUST_TRANSPORT_PORT}
EOF

cat > "${PY_SENDER_RNS_DIR}/config" <<EOF
[reticulum]
  enable_transport = true
  share_instance = no
  discover_interfaces = false
  autoconnect_discovered_interfaces = 0

[logging]
  loglevel = 4

[interfaces]
  [[Rust LXMD Sender]]
    type = TCPClientInterface
    enabled = yes
    target_host = ${RUST_TRANSPORT_HOST}
    target_port = ${RUST_TRANSPORT_PORT}
EOF

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p lxmf-cli --bin lxmd --quiet

(
  "${REPO_ROOT}/target/debug/lxmd" \
    --config "${RUST_DIR}/launcher.toml" >"${RUST_LOG}" 2>&1
) &
RUST_PID=$!

if ! wait_for_file_pattern "${RUST_LOG}" "listening on http://|delivery destination hash=" "${TIMEOUT_SECS}"; then
  echo "Rust lxmd did not become ready" >&2
  exit 1
fi

RUST_DELIVERY_HASH="$(destination_hash_from_identity "${RUST_DIR}/identity" "lxmf" "delivery")"
RUST_PROPAGATION_HASH="$(destination_hash_from_identity "${RUST_DIR}/identity" "lxmf" "propagation")"
RUST_CONTROL_IDENTITY_HASH="$(identity_hash_from_file "${RUST_DIR}/identity")"

cat > "${PY_DIR}/config" <<EOF
[propagation]
enable_node = yes
announce_at_start = yes
announce_interval = 1
autopeer = yes
autopeer_maxdepth = 6
control_allowed = ${RUST_CONTROL_IDENTITY_HASH}

[lxmf]
display_name = Python Smoke Node
announce_at_start = yes
announce_interval = 1

[logging]
loglevel = 4
EOF

(
  "${PYTHON_BIN}" -m LXMF.Utilities.lxmd \
    --config "${PY_DIR}" \
    --rnsconfig "${PY_RNS_DIR}" \
    --propagation-node >"${PY_LOG}" 2>&1
) &
PY_PID=$!

for _ in $(seq 1 "${TIMEOUT_SECS}"); do
  if [[ -f "${PY_DIR}/identity" ]] && kill -0 "${PY_PID}" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if [[ ! -f "${PY_DIR}/identity" ]] || ! kill -0 "${PY_PID}" >/dev/null 2>&1; then
  echo "Python lxmd did not become ready" >&2
  exit 1
fi

PY_DELIVERY_HASH="$(destination_hash_from_identity "${PY_DIR}/identity" "lxmf" "delivery")"
PY_PROPAGATION_HASH="$(destination_hash_from_identity "${PY_DIR}/identity" "lxmf" "propagation")"

for _ in $(seq 1 "${TIMEOUT_SECS}"); do
  if "${PYTHON_BIN}" -m LXMF.Utilities.lxmd \
      -v \
      --config "${PY_DIR}" \
      --rnsconfig "${PY_RNS_DIR}" \
      --identity "${PY_DIR}/identity" \
      --timeout 10 \
      --remote "${RUST_PROPAGATION_HASH}" \
      --status >"${PY_REMOTE_STATUS_LOG}" 2>&1; then
    break
  fi
  sleep 1
done

for _ in $(seq 1 "${TIMEOUT_SECS}"); do
  if "${REPO_ROOT}/target/debug/lxmd" \
      --config "${RUST_DIR}/launcher.toml" \
      --timeout 10 \
      --remote "${PY_PROPAGATION_HASH}" \
      --status >"${RUST_REMOTE_STATUS_LOG}" 2>&1; then
    break
  fi
  sleep 1
done

assert_contains "${RUST_REMOTE_STATUS_LOG}" "Remote LXMF Propagation Node status" "Rust remote status against Python node"

PY_MESSAGE_CONTENT="python-smoke-message-$(date +%s)"
"${PYTHON_BIN}" - <<'PY' \
  "${PY_SENDER_RNS_DIR}" \
  "${PY_SENDER_DIR}" \
  "${RUST_DELIVERY_HASH}" \
  "${PY_MESSAGE_CONTENT}" >"${PY_SEND_LOG}"
import json
import os
import sys
import time

import RNS
import LXMF

rns_config, storage_dir, destination_hash_hex, content = sys.argv[1:5]
destination_hash = bytes.fromhex(destination_hash_hex)

RNS.Reticulum(configdir=rns_config, loglevel=0)
identity = RNS.Identity()
router = LXMF.LXMRouter(identity=identity, storagepath=storage_dir)
source = router.register_delivery_identity(identity, display_name="Python Smoke Sender")

deadline = time.time() + 30
while time.time() < deadline:
    if RNS.Transport.has_path(destination_hash):
        break
    RNS.Transport.request_path(destination_hash)
    time.sleep(0.5)
else:
    raise SystemExit("timed out waiting for Rust delivery path")

remote_identity = None
deadline = time.time() + 15
while time.time() < deadline:
    remote_identity = RNS.Identity.recall(destination_hash)
    if remote_identity is not None:
        break
    time.sleep(0.2)

if remote_identity is None:
    raise SystemExit("timed out recalling Rust delivery identity")

destination = RNS.Destination(
    remote_identity,
    RNS.Destination.OUT,
    RNS.Destination.SINGLE,
    LXMF.APP_NAME,
    "delivery",
)
message = LXMF.LXMessage(
    destination,
    source,
    content=content,
    desired_method=LXMF.LXMessage.OPPORTUNISTIC,
)
router.handle_outbound(message)

deadline = time.time() + 45
while time.time() < deadline:
    if message.state in (LXMF.LXMessage.DELIVERED, LXMF.LXMessage.SENT):
        print(
            json.dumps(
                {
                    "state": int(message.state),
                    "destination": destination_hash_hex,
                    "source": RNS.hexrep(source.hash, delimit=False).lower(),
                }
            )
        )
        raise SystemExit(0)
    time.sleep(0.2)

raise SystemExit(f"timed out waiting for Python message delivery, state={message.state}")
PY

PY_SENDER_SOURCE_HASH="$("${PYTHON_BIN}" - <<'PY' "${PY_SEND_LOG}"
import json
import sys
from pathlib import Path

payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(payload["source"])
PY
)"

for _ in $(seq 1 "${TIMEOUT_SECS}"); do
  if [[ -f "${HOOK_LOG}" ]] && grep -q "${PY_MESSAGE_CONTENT}" "${HOOK_LOG}"; then
    break
  fi
  sleep 1
done

assert_contains "${HOOK_LOG}" "${PY_MESSAGE_CONTENT}" "Rust lxmd on-inbound hook content"
assert_contains "${HOOK_LOG}" "${PY_SENDER_SOURCE_HASH}" "Rust lxmd on-inbound hook source hash"

HOOK_MESSAGE_FILE="$("${PYTHON_BIN}" - <<'PY' "${HOOK_LOG}"
import sys
from pathlib import Path

for line in Path(sys.argv[1]).read_text(encoding="utf-8").splitlines():
    if line.startswith("message_file="):
        print(line.split("=", 1)[1])
        raise SystemExit(0)
raise SystemExit(1)
PY
)"

if [[ ! -s "${HOOK_MESSAGE_FILE}" ]]; then
  echo "expected inbound message file at ${HOOK_MESSAGE_FILE}" >&2
  exit 1
fi

"${PYTHON_BIN}" - <<'PY' \
  "${REPORT_PATH}" \
  "${TMP_ROOT}" \
  "${RUST_LOG}" \
  "${PY_LOG}" \
  "${PY_REMOTE_STATUS_LOG}" \
  "${RUST_REMOTE_STATUS_LOG}" \
  "${HOOK_LOG}" \
  "${RUST_DELIVERY_HASH}" \
  "${RUST_PROPAGATION_HASH}" \
  "${PY_DELIVERY_HASH}" \
  "${PY_PROPAGATION_HASH}" \
  "${HOOK_MESSAGE_FILE}" \
  "${PY_MESSAGE_CONTENT}"
import json
import sys

(
    report_path,
    tmp_root,
    rust_log,
    py_log,
    py_remote_status_log,
    rust_remote_status_log,
    hook_log,
    rust_delivery_hash,
    rust_propagation_hash,
    py_delivery_hash,
    py_propagation_hash,
    hook_message_file,
    py_message_content,
) = sys.argv[1:14]

report = {
    "status": "pass",
    "proof": {
        "python_remote_status_to_rust": rust_propagation_hash,
        "rust_remote_status_to_python": py_propagation_hash,
        "python_to_rust_inbound_content": py_message_content,
        "rust_hook_message_file": hook_message_file,
    },
    "hashes": {
        "rust_delivery": rust_delivery_hash,
        "rust_propagation": rust_propagation_hash,
        "python_delivery": py_delivery_hash,
        "python_propagation": py_propagation_hash,
    },
    "logs": {
        "tmp_root": tmp_root,
        "rust_lxmd": rust_log,
        "python_lxmd": py_log,
        "python_remote_status": py_remote_status_log,
        "rust_remote_status": rust_remote_status_log,
        "hook": hook_log,
    },
}

with open(report_path, "w", encoding="utf-8") as handle:
    json.dump(report, handle, indent=2)
    handle.write("\n")
PY

echo "[python-lxmd-rust-lxmd-smoke] pass"
echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
