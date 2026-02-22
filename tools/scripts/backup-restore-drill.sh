#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

echo "[drill] running backup/restore simulation test"
cargo test -p rns-rpc sdk_backup_restore_drill_recovers_snapshot_and_messages -- --nocapture
echo "[drill] backup/restore simulation completed"
