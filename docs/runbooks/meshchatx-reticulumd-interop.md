# MeshChatX <-> reticulumd Interop

This runbook executes the first reproducible external-client interoperability
proof between `reticulumd` and MeshChatX.

## What It Proves

- `reticulumd` can send a real LXMF message to MeshChatX over a Reticulum TCP
  interface.
- MeshChatX can send a real LXMF reply back to `reticulumd`.
- Both directions are checked through each side's own observable state:
  - MeshChatX conversation API
  - `reticulumd` message store

## Prerequisites

- MeshChatX source checkout exists locally.
- `uv` is installed.
- `curl` is installed.
- The Rust workspace builds `reticulumd` and `lxmf-cli`.

Default MeshChatX path:

```bash
../MeshChatX
```

Override it with `MESHCHATX_ROOT` if needed.

## Run

```bash
./tools/scripts/meshchatx-reticulumd-smoke.sh
```

Useful overrides:

```bash
MESHCHATX_ROOT=/path/to/MeshChatX \
REPORT_PATH=target/interop/meshchatx-report.json \
RUST_LOG="reticulumd=trace,reticulum_rs_transport=trace" \
./tools/scripts/meshchatx-reticulumd-smoke.sh
```

## Expected Output

```text
[meshchatx-reticulumd-smoke] pass
[meshchatx-reticulumd-smoke] report=...
[meshchatx-reticulumd-smoke] logs=...
```

## Verification Model

The script verifies both directions:

1. `reticulumd -> MeshChatX`
   MeshChatX must expose the daemon-originated message through
   `GET /api/v1/lxmf-messages/conversation/{daemon_hash}`.

2. `MeshChatX -> reticulumd`
   `reticulumd` must persist the MeshChatX-originated reply in its SQLite
   `messages` table with:
   - `direction = 'in'`
   - `source = meshchatx_hash`
   - `destination = daemon_hash`

## Artifacts

The report contains:

- daemon delivery hash
- MeshChatX delivery hash
- proof message bodies
- temp/log artifact paths

The report contract is defined in
[external-client-interop-acceptance-v1.md](../contracts/external-client-interop-acceptance-v1.md).
