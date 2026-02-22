#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

REPORT_PATH="target/supply-chain/reproducible/reproducible-build-report.txt"
BUILD_A_DIR="target/reproducible/build-a"
BUILD_B_DIR="target/reproducible/build-b"

BINARIES=(
  "lxmf-cli"
  "reticulumd"
  "rncp"
  "rnid"
  "rnir"
  "rnodeconf"
  "rnpath"
  "rnpkg"
  "rnprobe"
  "rnsd"
  "rnstatus"
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
  CARGO_TARGET_DIR="$target_dir" \
  CARGO_INCREMENTAL=0 \
  SOURCE_DATE_EPOCH=1 \
  TZ=UTC \
  LC_ALL=C \
  LANG=C \
  RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=${ROOT_DIR}=/workspace" \
  cargo build --release --workspace --bins --locked
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

  digest_a="$(sha256_file "$artifact_a")"
  digest_b="$(sha256_file "$artifact_b")"
  if [[ "$digest_a" == "$digest_b" ]]; then
    echo "MATCH ${binary} ${digest_a}" >>"$REPORT_PATH"
  else
    echo "MISMATCH ${binary} A=${digest_a} B=${digest_b}" >>"$REPORT_PATH"
    status=1
  fi
done

if [[ "$status" -ne 0 ]]; then
  echo "error: reproducible build mismatch detected; see ${REPORT_PATH}" >&2
  exit 1
fi

echo "reproducible build check passed; report written to ${REPORT_PATH}"
