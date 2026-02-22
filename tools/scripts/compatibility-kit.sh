#!/usr/bin/env bash
set -euo pipefail

mode="${1:---dry-run}"
out_dir="${COMPAT_KIT_OUT_DIR:-target/compat-kit}"

fail() {
  echo "compatibility-kit failed: $*" >&2
  exit 1
}

require_file() {
  local path="$1"
  [ -f "$path" ] || fail "required artifact missing: $path"
}

require_command() {
  local cmd="$1"
  command -v "$cmd" >/dev/null || fail "required command missing: $cmd"
}

require_command cargo
require_file "docs/contracts/compatibility-contract.md"
require_file "docs/contracts/compatibility-matrix.md"
require_file "docs/contracts/rpc-contract.md"
require_file "docs/contracts/payload-contract.md"
require_file "docs/contracts/baselines/interop-artifacts-manifest.json"
require_file "docs/fixtures/interop/v1/golden-corpus.json"

if [ "$mode" = "--dry-run" ]; then
  echo "compatibility-kit dry-run: required artifacts and toolchain are present"
  exit 0
fi

if [ "$mode" != "--build" ]; then
  fail "unknown mode '$mode' (expected --dry-run or --build)"
fi

mkdir -p "$out_dir/contracts" "$out_dir/fixtures"

cargo run -p xtask -- interop-artifacts
cargo run -p xtask -- interop-matrix-check
cargo run -p xtask -- sdk-schema-check
cargo run -p xtask -- sdk-conformance
cargo run -p xtask -- e2e-compatibility

cp docs/contracts/compatibility-contract.md "$out_dir/contracts/"
cp docs/contracts/compatibility-matrix.md "$out_dir/contracts/"
cp docs/contracts/rpc-contract.md "$out_dir/contracts/"
cp docs/contracts/payload-contract.md "$out_dir/contracts/"
cp docs/contracts/baselines/interop-artifacts-manifest.json "$out_dir/contracts/"
cp docs/fixtures/interop/v1/golden-corpus.json "$out_dir/fixtures/"

cat > "$out_dir/README.md" <<'EOF'
# LXMF-rs External Compatibility Kit

This bundle is generated from the in-repo contract and fixture sources.

Validation gates used to build this kit:
- `cargo run -p xtask -- interop-artifacts`
- `cargo run -p xtask -- interop-matrix-check`
- `cargo run -p xtask -- sdk-schema-check`
- `cargo run -p xtask -- sdk-conformance`
- `cargo run -p xtask -- e2e-compatibility`

Use the copied contracts and fixtures as the source of truth for external client conformance checks.
EOF

echo "compatibility-kit build: wrote artifacts to $out_dir"
