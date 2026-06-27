#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

REPORT_PATH="${ROOT_DIR}/target/supply-chain/reproducible/reproducible-build-report.txt"
BUILD_A_DIR="${ROOT_DIR}/target/reproducible/build-a"
BUILD_B_DIR="${ROOT_DIR}/target/reproducible/build-b"

BINARIES=(
  "lxmf-cli"
  "reticulumd"
  "rnsd"
  "rnx"
)

if command -v sha256sum >/dev/null 2>&1; then
  sha256_cmd=(sha256sum)
elif command -v shasum >/dev/null 2>&1; then
  sha256_cmd=(shasum -a 256)
else
  echo "error: missing sha256sum/shasum tool" >&2
  exit 1
fi

sha256_file() {
  "${sha256_cmd[@]}" "$1" | awk '{print $1}'
}

build_once() {
  local target_dir="$1"
  local rustflags="${RUSTFLAGS:-} --remap-path-prefix=${target_dir}=/target --remap-path-prefix=${ROOT_DIR}=/workspace"
  mkdir -p "${target_dir}/zig-local-cache" "${ROOT_DIR}/target/reproducible/zig-global-cache"
  CARGO_TARGET_DIR="$target_dir" \
  CARGO_INCREMENTAL=0 \
  SOURCE_DATE_EPOCH=1 \
  TZ=UTC \
  LC_ALL=C \
  LANG=C \
  ZIG_LOCAL_CACHE_DIR="${target_dir}/zig-local-cache" \
  ZIG_GLOBAL_CACHE_DIR="${ROOT_DIR}/target/reproducible/zig-global-cache" \
  RUSTFLAGS="${rustflags}" \
  cargo build --release --workspace --bins --locked
}

normalized_copy() {
  local input="$1"
  local output="$2"
  cp "$input" "$output"

  local canonical_target_prefix="${ROOT_DIR}/target/reproducible/build-x"
  local canonical_workspace_target="/workspace/target/reproducible/build-x"

  python3 - "$output" "$BUILD_A_DIR" "$BUILD_B_DIR" "$canonical_target_prefix" "$canonical_workspace_target" <<'PY'
import re
import sys
from pathlib import Path

path = Path(sys.argv[1])
build_a_dir = sys.argv[2].encode()
build_b_dir = sys.argv[3].encode()
canonical_target_prefix = sys.argv[4].encode()
canonical_workspace_target = sys.argv[5].encode()
data = path.read_bytes()

for old, new in (
    (build_a_dir, canonical_target_prefix),
    (build_b_dir, canonical_target_prefix),
    (b"/workspace/target/reproducible/build-a", canonical_workspace_target),
    (b"/workspace/target/reproducible/build-b", canonical_workspace_target),
):
    data = data.replace(old, new)

data = re.sub(
    rb"(/release/deps/rustc)[A-Za-z0-9_]{6}(/raw-dylibs)",
    rb"\g<1>000000\g<2>",
    data,
)

path.write_bytes(data)
PY

  case "$(uname -s)" in
    Darwin)
      python3 - "$output" "$BUILD_A_DIR" "$BUILD_B_DIR" "$canonical_target_prefix" "$canonical_workspace_target" <<'PY'
import struct
import sys
from pathlib import Path

path = Path(sys.argv[1])
build_a_dir = sys.argv[2].encode()
build_b_dir = sys.argv[3].encode()
canonical_target_prefix = sys.argv[4].encode()
canonical_workspace_target = sys.argv[5].encode()
data = bytearray(path.read_bytes())

MH_MAGIC_64 = 0xfeedfacf
LC_UUID = 0x1B
LC_CODE_SIGNATURE = 0x1D

if len(data) < 32:
    path.write_bytes(data)
    raise SystemExit(0)

magic, = struct.unpack_from("<I", data, 0)
if magic != MH_MAGIC_64:
    path.write_bytes(data)
    raise SystemExit(0)

ncmds, sizeofcmds = struct.unpack_from("<II", data, 16)
offset = 32
end = offset + sizeofcmds
code_sig_range = None
for _ in range(ncmds):
    if offset + 8 > len(data) or offset + 8 > end:
        break
    cmd, cmdsize = struct.unpack_from("<II", data, offset)
    if cmdsize < 8 or offset + cmdsize > len(data):
        break
    if cmd == LC_UUID and cmdsize >= 24:
        data[offset + 8:offset + 24] = b"\x00" * 16
    elif cmd == LC_CODE_SIGNATURE and cmdsize >= 16:
        dataoff, datasize = struct.unpack_from("<II", data, offset + 8)
        code_sig_range = (dataoff, datasize)
    offset += cmdsize

for old, new in (
    (build_a_dir, canonical_target_prefix),
    (build_b_dir, canonical_target_prefix),
    (b"/workspace/target/reproducible/build-a", canonical_workspace_target),
    (b"/workspace/target/reproducible/build-b", canonical_workspace_target),
):
    data = data.replace(old, new)

if code_sig_range is not None:
    dataoff, datasize = code_sig_range
    sig_end = min(len(data), dataoff + datasize)
    data[dataoff:sig_end] = b"\x00" * (sig_end - dataoff)

path.write_bytes(data)
PY
      ;;
  esac
}

mkdir -p "$(dirname "$REPORT_PATH")"
rm -rf "$BUILD_A_DIR" "$BUILD_B_DIR"

build_once "$BUILD_A_DIR"
build_once "$BUILD_B_DIR"

{
  echo "# Reproducible Build Report"
  echo
  echo "root=${ROOT_DIR}"
  echo "source_date_epoch=1"
  echo "rustc=$(rustc --version)"
  echo "cargo=$(cargo --version)"
  echo
} >"$REPORT_PATH"

status=0
for binary in "${BINARIES[@]}"; do
  artifact_a="${BUILD_A_DIR}/release/${binary}"
  artifact_b="${BUILD_B_DIR}/release/${binary}"
  if [[ ! -f "$artifact_a" || ! -f "$artifact_b" ]]; then
    echo "MISSING ${binary}" >>"$REPORT_PATH"
    status=1
    continue
  fi

  normalized_a="$(mktemp "${TMPDIR:-/tmp}/repro-a-${binary}.XXXXXX")"
  normalized_b="$(mktemp "${TMPDIR:-/tmp}/repro-b-${binary}.XXXXXX")"
  trap 'rm -f "${normalized_a}" "${normalized_b}"' RETURN

  normalized_copy "$artifact_a" "$normalized_a"
  normalized_copy "$artifact_b" "$normalized_b"

  digest_a="$(sha256_file "$normalized_a")"
  digest_b="$(sha256_file "$normalized_b")"
  if [[ "$digest_a" == "$digest_b" ]]; then
    echo "MATCH ${binary} ${digest_a}" >>"$REPORT_PATH"
  else
    echo "MISMATCH ${binary} A=${digest_a} B=${digest_b}" >>"$REPORT_PATH"
    status=1
  fi

  rm -f "$normalized_a" "$normalized_b"
  trap - RETURN
done

if [[ "$status" -ne 0 ]]; then
  echo "error: reproducible build mismatch detected; see ${REPORT_PATH}" >&2
  exit 1
fi

echo "reproducible build check passed; report written to ${REPORT_PATH}"
