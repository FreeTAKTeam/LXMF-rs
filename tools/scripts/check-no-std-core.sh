#!/usr/bin/env bash
# Verifies that reticulum-rs-core builds for a bare-metal no_std target.
#
# Issue #676 reported that the crate's conditional `no_std` attribute was not
# backed by its dependency graph. This gate keeps the claim honest: a host
# build with `--no-default-features` is not evidence of no_std support, only a
# build for a target without `std` is.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

NO_STD_TARGET="${NO_STD_TARGET:-thumbv7em-none-eabi}"
PACKAGE="reticulum-rs-core"

if ! rustup target list --installed 2>/dev/null | grep -Fxq "$NO_STD_TARGET"; then
  echo "error: Rust target ${NO_STD_TARGET} is not installed." >&2
  echo "       Install it with: rustup target add ${NO_STD_TARGET}" >&2
  exit 1
fi

for features in "alloc" "alloc,fernet-aes128"; do
  echo "no_std check: ${PACKAGE} --features ${features} --target ${NO_STD_TARGET}"
  cargo check \
    --locked \
    -p "$PACKAGE" \
    --no-default-features \
    --features "$features" \
    --target "$NO_STD_TARGET"
done

echo "no_std core check passed for ${PACKAGE} on ${NO_STD_TARGET}"
