#!/usr/bin/env bash
# Validate Cargo's normalized distributable, never the host workspace lockfile.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
fixture="$root/tools/fixtures/bluetooth-msrv"
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
cargo +stable package -p reticulum-rs-bluetooth --locked --allow-dirty --no-verify
version="$(python3 -c 'import tomllib; print(tomllib.load(open("crates/libs/rns-bluetooth/Cargo.toml", "rb"))["package"]["version"])')"
tar -xf "target/package/reticulum-rs-bluetooth-$version.crate" -C "$staging"
mv "$staging/reticulum-rs-bluetooth-$version" "$staging/package"
cp "$fixture/package.lock" "$staging/package/Cargo.lock"
cp -R "$fixture/consumer" "$staging/consumer"
# Cargo archives normalize mtimes. A reused build directory could accept stale
# fingerprints across unpacked versions; keep compilation evidence fresh too.
export CARGO_TARGET_DIR="$staging/target"
cd "$staging/package"
cargo +1.85.0 test --locked --no-default-features
cargo +1.85.0 test --locked --no-default-features --features android
cargo +1.85.0 test --locked --no-default-features --features bluez
cargo +1.85.0 check --locked --target aarch64-linux-android --no-default-features --features android
cd "$staging/consumer"
cargo +1.85.0 run --locked
