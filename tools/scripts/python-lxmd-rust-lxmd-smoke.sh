#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

PYTHON_BIN="${PYTHON_BIN:-python3}"
PYTHON_BIN="${LXMF_PYTHON_BIN:-${PYTHON_BIN}}"
LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/interop/python-lxmd-rust-lxmd}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
TIMEOUT_SECS="${TIMEOUT_SECS:-45}"
REMOTE_STATUS_PREFLIGHT="${REMOTE_STATUS_PREFLIGHT:-0}"
COMPAT_CASE="${COMPAT_CASE:-direct_python_to_rust}"

RUST_RPC_ADDR="${RUST_RPC_ADDR:-127.0.0.1:$((42430 + ($$ % 1000)))}"
RUST_TRANSPORT_ADDR="${RUST_TRANSPORT_ADDR:-127.0.0.1:$((37430 + ($$ % 1000)))}"
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

rpc_call() {
  local rpc_addr="$1"
  local method="$2"
  local params_json="${3:-null}"
  "${PYTHON_BIN}" - <<'PY' "${rpc_addr}" "${method}" "${params_json}"
import json
import socket
import sys
import time

import RNS.vendor.umsgpack as msgpack

rpc_addr, method, params_json = sys.argv[1:4]
params = None if params_json == "null" else json.loads(params_json)
host, port = rpc_addr.split(":", 1)

def is_rate_limited(error):
    if error == "SDK_SECURITY_RATE_LIMITED":
        return True
    if isinstance(error, list) and error:
        return error[0] == "SDK_SECURITY_RATE_LIMITED"
    if isinstance(error, dict):
        return error.get("code") == "SDK_SECURITY_RATE_LIMITED"
    return False

for attempt in range(60):
    payload = {"id": 1, "method": method, "params": params}
    packed = msgpack.packb(payload)
    frame = len(packed).to_bytes(4, "big") + packed
    request = (
        f"POST /rpc HTTP/1.1\r\n"
        f"Host: {rpc_addr}\r\n"
        f"Content-Length: {len(frame)}\r\n"
        f"Connection: close\r\n\r\n"
    ).encode("utf-8") + frame
    with socket.create_connection((host, int(port)), timeout=30) as sock:
        sock.sendall(request)
        response = bytearray()
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            response.extend(chunk)
    header_end = response.find(b"\r\n\r\n")
    if header_end < 0:
        raise SystemExit("missing rpc response body")
    body = response[header_end + 4 :]
    if len(body) < 4:
        raise SystemExit("rpc response too short")
    frame_len = int.from_bytes(body[:4], "big")
    if len(body) < 4 + frame_len:
        raise SystemExit("rpc response incomplete")
    value = msgpack.unpackb(body[4 : 4 + frame_len])
    if isinstance(value, list):
        result = value[1] if len(value) > 1 else None
        error = value[2] if len(value) > 2 else None
    elif isinstance(value, dict):
        result = value.get("result", value)
        error = value.get("error")
        if error is None and isinstance(result, dict):
            error = result.get("error")
    else:
        result = value
        error = None
    if error and is_rate_limited(error) and attempt + 1 < 60:
        time.sleep(5)
        continue
    if error:
        raise SystemExit(json.dumps(error))
    print(json.dumps(result))
    raise SystemExit(0)

raise SystemExit(f"rpc call {method} exhausted retry budget")
PY
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
RUST_HOOK_LOG="${HOOK_STATE_DIR}/rust-hook.log"
PY_HOOK_LOG="${HOOK_STATE_DIR}/python-hook.log"

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

if [[ "${COMPAT_CASE}" == "lxm_interchange" ]]; then
  mkdir -p "${TMP_ROOT}/lxm"
  cargo build -p reticulumd --bin lxm-interchange --quiet

  LXM_PATH="$("${PYTHON_BIN}" - <<'PY' "${TMP_ROOT}/lxm"
import sys
from pathlib import Path

import RNS
import LXMF

out_dir = Path(sys.argv[1])
out_dir.mkdir(parents=True, exist_ok=True)

sender_identity = RNS.Identity()
receiver_identity = RNS.Identity()
sender = RNS.Destination(sender_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "delivery")
receiver = RNS.Destination(receiver_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "delivery")
message = LXMF.LXMessage(
    destination=receiver,
    source=sender,
    title=b"\xfftitle",
    content=b"body\x00\xff",
    fields={"meta": {"kind": "python-storage", "count": 2}},
    desired_method=LXMF.LXMessage.DIRECT,
)
message.timestamp = 1770000000.25
message.pack()
written = message.write_to_directory(str(out_dir))
if written is None:
    raise SystemExit("failed to write Python .lxm container")
print(written)
PY
)"

  DECODED_JSON="$("${REPO_ROOT}/target/debug/lxm-interchange" --file "${LXM_PATH}")"
  "${PYTHON_BIN}" - <<'PY' "${DECODED_JSON}" "${REPORT_PATH}" "${COMPAT_CASE}"
import base64
import json
import sys
from pathlib import Path

decoded = json.loads(sys.argv[1])
report_path = Path(sys.argv[2])
case_id = sys.argv[3]

assert decoded["title_utf8"] is None, decoded
assert decoded["content_utf8"] is None, decoded
assert decoded["title_base64"] == base64.b64encode(b"\xfftitle").decode("ascii"), decoded
assert decoded["content_base64"] == base64.b64encode(b"body\x00\xff").decode("ascii"), decoded
assert decoded["fields"] == {"meta": {"kind": "python-storage", "count": 2}}, decoded
assert abs(decoded["timestamp_f64"] - 1770000000.25) < 1e-9, decoded
assert len(decoded["source"]) == 32, decoded
assert len(decoded["destination"]) == 32, decoded

report_path.write_text(json.dumps({
    "status": "pass",
    "case": case_id,
    "decoded": decoded,
}), encoding="utf-8")
PY
  exit 0
fi

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

RUST_ON_INBOUND_LINE="on_inbound = ${RUST_DIR}/on_inbound.sh"
if [[ "${COMPAT_CASE}" == *_rust_to_python ]]; then
  RUST_ON_INBOUND_LINE="# on_inbound disabled for rust_to_python compatibility lanes"
fi

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
propagation_stamp_cost_target = 0
propagation_stamp_cost_flexibility = 0
autopeer = yes
autopeer_maxdepth = 6
control_allowed = ${PY_CONTROL_IDENTITY_HASH}

[lxmf]
display_name = Rust Smoke Node
announce_at_start = yes
announce_interval = 1
${RUST_ON_INBOUND_LINE}

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
} >> "${state_dir}/rust-hook.log"
EOF
chmod +x "${RUST_DIR}/on_inbound.sh"

cat > "${PY_DIR}/on_inbound.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
message_file="${1:-}"
state_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../hook-state && pwd)"
mkdir -p "${state_dir}"
python3 - <<'PY' "${message_file}" "${state_dir}/python-hook.log"
import sys
from pathlib import Path

import LXMF

message_path = Path(sys.argv[1])
log_path = Path(sys.argv[2])
with message_path.open("rb") as handle:
    message = LXMF.LXMessage.unpack_from_file(handle)
if message is None:
    raise SystemExit("failed to unpack Python inbound message")
with log_path.open("a", encoding="utf-8") as handle:
    handle.write(f"message_file={message_path}\n")
    handle.write(f"source={message.source_hash.hex() if message.source_hash else ''}\n")
    handle.write(f"destination={message.destination_hash.hex() if message.destination_hash else ''}\n")
    handle.write(f"title={message.title_as_string() or ''}\n")
    handle.write(f"content={message.content_as_string() or ''}\n")
PY
EOF
chmod +x "${PY_DIR}/on_inbound.sh"

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
propagation_stamp_cost_target = 0
propagation_stamp_cost_flexibility = 0
autopeer = yes
autopeer_maxdepth = 6
control_allowed = ${RUST_CONTROL_IDENTITY_HASH}

[lxmf]
display_name = Python Smoke Node
announce_at_start = yes
announce_interval = 1
on_inbound = ${PY_DIR}/on_inbound.sh

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

if [[ "${REMOTE_STATUS_PREFLIGHT}" == "1" && "${COMPAT_CASE}" != "propagated_python_to_rust" && "${COMPAT_CASE}" != "propagated_rust_to_python" ]]; then
  PY_REMOTE_STATUS_OK=0
  for _ in $(seq 1 "${TIMEOUT_SECS}"); do
    if "${PYTHON_BIN}" -m LXMF.Utilities.lxmd \
        -v \
        --config "${PY_DIR}" \
        --rnsconfig "${PY_RNS_DIR}" \
        --identity "${PY_DIR}/identity" \
        --timeout 10 \
        --remote "${RUST_PROPAGATION_HASH}" \
        --status >"${PY_REMOTE_STATUS_LOG}" 2>&1; then
      PY_REMOTE_STATUS_OK=1
      break
    fi
    sleep 1
  done

  RUST_REMOTE_STATUS_OK=0
  for _ in $(seq 1 "${TIMEOUT_SECS}"); do
    if "${REPO_ROOT}/target/debug/lxmd" \
        --config "${RUST_DIR}/launcher.toml" \
        --timeout 10 \
        --remote "${PY_PROPAGATION_HASH}" \
        --status >"${RUST_REMOTE_STATUS_LOG}" 2>&1; then
      RUST_REMOTE_STATUS_OK=1
      break
    fi
    sleep 1
  done

  if [[ "${PY_REMOTE_STATUS_OK}" -ne 1 ]]; then
    echo "Python lxmd could not query Rust propagation node status" >&2
    exit 1
  fi
  if [[ "${RUST_REMOTE_STATUS_OK}" -ne 1 ]]; then
    echo "Rust lxmd could not query Python propagation node status" >&2
    exit 1
  fi
  assert_contains "${RUST_REMOTE_STATUS_LOG}" "Remote LXMF Propagation Node status" "Rust remote status against Python node"
else
  printf 'skipped remote-status preflight\n' >"${PY_REMOTE_STATUS_LOG}"
  printf 'skipped remote-status preflight\n' >"${RUST_REMOTE_STATUS_LOG}"
fi

SMOKE_MESSAGE_MARKER="smoke-message-${COMPAT_CASE}-$(date +%s)"
SMOKE_MESSAGE_CONTENT="${SMOKE_MESSAGE_MARKER}"
if [[ "${COMPAT_CASE}" == "resource_transfer" ]]; then
  SMOKE_MESSAGE_CONTENT="${SMOKE_MESSAGE_MARKER}:$(printf 'x%.0s' $(seq 1 16384))"
fi
if [[ "${COMPAT_CASE}" == *_python_to_rust ]]; then
  "${PYTHON_BIN}" - <<'PY' \
  "${COMPAT_CASE}" \
  "${PY_SENDER_RNS_DIR}" \
  "${PY_SENDER_DIR}" \
  "${RUST_DELIVERY_HASH}" \
  "${RUST_PROPAGATION_HASH}" \
  "${TIMEOUT_SECS}" \
  "${SMOKE_MESSAGE_CONTENT}" >"${PY_SEND_LOG}"
import json
import os
import sys
import time

import RNS
import LXMF

case_id, rns_config, storage_dir, destination_hash_hex, propagation_hash_hex, timeout_secs, content = sys.argv[1:8]
timeout_secs = max(float(timeout_secs), 1.0)
destination_hash = bytes.fromhex(destination_hash_hex)
propagation_hash = bytes.fromhex(propagation_hash_hex)

RNS.Reticulum(configdir=rns_config, loglevel=0)
identity = RNS.Identity()
router = LXMF.LXMRouter(identity=identity, storagepath=storage_dir)
source = router.register_delivery_identity(identity, display_name="Python Smoke Sender")

deadline = time.time() + timeout_secs
remote_identity = None
desired_method = LXMF.LXMessage.OPPORTUNISTIC
if case_id in ("direct_python_to_rust", "opportunistic_python_to_rust"):
    while time.time() < deadline:
        if RNS.Transport.has_path(destination_hash):
            break
        RNS.Transport.request_path(destination_hash)
        time.sleep(0.5)
    else:
        raise SystemExit("timed out waiting for Rust delivery path")

    deadline = time.time() + 15
    while time.time() < deadline:
        remote_identity = RNS.Identity.recall(destination_hash)
        if remote_identity is not None:
            break
        time.sleep(0.2)

    if remote_identity is None:
        raise SystemExit("timed out recalling Rust delivery identity")

    if case_id == "direct_python_to_rust":
        desired_method = LXMF.LXMessage.DIRECT
elif case_id == "propagated_python_to_rust":
    desired_method = LXMF.LXMessage.PROPAGATED
    deadline = time.time() + timeout_secs
    while time.time() < deadline:
        if RNS.Transport.has_path(propagation_hash):
            break
        RNS.Transport.request_path(propagation_hash)
        time.sleep(0.5)
    else:
        raise SystemExit("timed out waiting for Rust propagation path")

    deadline = time.time() + max(15.0, timeout_secs / 3.0)
    while time.time() < deadline:
        remote_identity = RNS.Identity.recall(propagation_hash)
        if remote_identity is not None:
            break
        time.sleep(0.2)

    if remote_identity is None:
        raise SystemExit("timed out recalling Rust identity from propagation path")

    router.set_outbound_propagation_node(propagation_hash)
elif case_id != "opportunistic_python_to_rust":
    raise SystemExit(f"unsupported smoke case: {case_id}")

if remote_identity is None:
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
    desired_method=desired_method,
)
router.handle_outbound(message)

deadline = time.time() + timeout_secs
while time.time() < deadline:
    if message.state in (LXMF.LXMessage.DELIVERED, LXMF.LXMessage.SENT):
        print(
            json.dumps(
                {
                    "state": int(message.state),
                    "case": case_id,
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
    if [[ -f "${RUST_HOOK_LOG}" ]] && grep -q "${SMOKE_MESSAGE_CONTENT}" "${RUST_HOOK_LOG}"; then
      break
    fi
    sleep 1
  done

  assert_contains "${RUST_HOOK_LOG}" "${SMOKE_MESSAGE_CONTENT}" "Rust lxmd on-inbound hook content"
  assert_contains "${RUST_HOOK_LOG}" "${PY_SENDER_SOURCE_HASH}" "Rust lxmd on-inbound hook source hash"

  HOOK_MESSAGE_FILE="$("${PYTHON_BIN}" - <<'PY' "${RUST_HOOK_LOG}"
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
else
  case "${COMPAT_CASE}" in
    direct_rust_to_python)
      RUST_SEND_METHOD="direct"
      ;;
    opportunistic_rust_to_python)
      RUST_SEND_METHOD="opportunistic"
      ;;
    propagated_rust_to_python)
      RUST_SEND_METHOD="propagated"
      rpc_call "${RUST_RPC_ADDR}" "set_outbound_propagation_node" "{\"peer\":\"${PY_PROPAGATION_HASH}\"}" >/dev/null
      assert_contains <(
        rpc_call "${RUST_RPC_ADDR}" "get_outbound_propagation_node" "null"
      ) "\"peer\": *\"${PY_PROPAGATION_HASH}\"" "selected outbound propagation node"
      ;;
    resource_transfer)
      RUST_SEND_METHOD="direct"
      ;;
    *)
      echo "unsupported compatibility case: ${COMPAT_CASE}" >&2
      exit 2
      ;;
  esac

  RUST_MESSAGE_ID="rust-smoke-${COMPAT_CASE}-$(date +%s)"
  rpc_call "${RUST_RPC_ADDR}" "announce_now" "null" >/dev/null
  if [[ "${COMPAT_CASE}" == "opportunistic_rust_to_python" ]]; then
    PEER_VISIBLE=0
    for _ in $(seq 1 "${TIMEOUT_SECS}"); do
      if rpc_call "${RUST_RPC_ADDR}" "list_peers" "null" | grep -Eq "\"peer\": *\"${PY_DELIVERY_HASH}\""; then
        PEER_VISIBLE=1
        break
      fi
      sleep 1
    done
    if [[ "${PEER_VISIBLE}" -ne 1 ]]; then
      echo "Rust did not learn Python delivery announce for opportunistic send" >&2
      exit 1
    fi
  fi
  rpc_call "${RUST_RPC_ADDR}" "send_message_v2" "$(cat <<EOF
{"id":"${RUST_MESSAGE_ID}","source":"${RUST_DELIVERY_HASH}","destination":"${PY_DELIVERY_HASH}","title":"","content":"${SMOKE_MESSAGE_CONTENT}","method":"${RUST_SEND_METHOD}"}
EOF
)" >"${PY_SEND_LOG}"

  for _ in $(seq 1 "${TIMEOUT_SECS}"); do
    if [[ -f "${PY_HOOK_LOG}" ]] && grep -q "${SMOKE_MESSAGE_MARKER}" "${PY_HOOK_LOG}"; then
      break
    fi
    sleep 1
  done

  assert_contains "${PY_HOOK_LOG}" "${SMOKE_MESSAGE_MARKER}" "Python lxmd on-inbound hook content"
  assert_contains "${PY_HOOK_LOG}" "${PY_DELIVERY_HASH}" "Python lxmd on-inbound hook destination hash"
  if [[ "${COMPAT_CASE}" == "resource_transfer" ]]; then
    assert_contains "${RUST_LOG}" "resource_hash=|sending: link resource|sent: link resource" "Rust resource transfer trace"
  fi

  HOOK_MESSAGE_FILE="$("${PYTHON_BIN}" - <<'PY' "${PY_HOOK_LOG}"
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
fi

"${PYTHON_BIN}" - <<'PY' \
  "${REPORT_PATH}" \
  "${TMP_ROOT}" \
  "${RUST_LOG}" \
  "${PY_LOG}" \
  "${PY_REMOTE_STATUS_LOG}" \
  "${RUST_REMOTE_STATUS_LOG}" \
  "${RUST_HOOK_LOG}" \
  "${PY_HOOK_LOG}" \
  "${RUST_DELIVERY_HASH}" \
  "${RUST_PROPAGATION_HASH}" \
  "${PY_DELIVERY_HASH}" \
  "${PY_PROPAGATION_HASH}" \
  "${HOOK_MESSAGE_FILE}" \
  "${SMOKE_MESSAGE_MARKER}" \
  "${COMPAT_CASE}"
import json
import sys

(
    report_path,
    tmp_root,
    rust_log,
    py_log,
    py_remote_status_log,
    rust_remote_status_log,
    rust_hook_log,
    py_hook_log,
    rust_delivery_hash,
    rust_propagation_hash,
    py_delivery_hash,
    py_propagation_hash,
    hook_message_file,
    smoke_message_content,
    compat_case,
) = sys.argv[1:16]

report = {
    "status": "pass",
    "case": compat_case,
    "proof": {
        "python_remote_status_to_rust": rust_propagation_hash,
        "rust_remote_status_to_python": py_propagation_hash,
        "smoke_message_content": smoke_message_content,
        "hook_message_file": hook_message_file,
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
        "rust_hook": rust_hook_log,
        "python_hook": py_hook_log,
    },
}

with open(report_path, "w", encoding="utf-8") as handle:
    json.dump(report, handle, indent=2)
    handle.write("\n")
PY

echo "[python-lxmd-rust-lxmd-smoke] pass"
echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
