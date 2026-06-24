# Columba <-> reticulumd Interop

This runbook executes a reproducible external-client interoperability proof
between `reticulumd` and Columba.

## What It Proves

- `reticulumd` can send a real LXMF message to Columba over a Reticulum TCP
  interface.
- Columba can send a real LXMF reply back to `reticulumd`.
- Both directions are checked through each side's own observable state:
  - Columba's real Python `ReticulumWrapper.poll_received_messages()` path
  - `reticulumd` message store

## Prerequisites

- Columba source checkout exists locally.
- `python3` can import `RNS` and `LXMF`.
- The Rust workspace builds `reticulumd` and `lxmf-cli`.

Default Columba path:

```bash
../columba
```

Override it with `COLUMBA_ROOT` if needed.

## Run

```bash
./tools/scripts/columba-reticulumd-smoke.sh
```

Useful overrides:

```bash
COLUMBA_ROOT=/path/to/columba \
REPORT_PATH=target/interop/columba-report.json \
RUST_LOG="reticulumd=trace,reticulum_rs_transport=trace" \
./tools/scripts/columba-reticulumd-smoke.sh
```

## Expected Output

```text
[columba-reticulumd-smoke] pass
[columba-reticulumd-smoke] report=...
[columba-reticulumd-smoke] logs=...
```

## Verification Model

The script verifies both directions:

1. `reticulumd -> Columba`
   Columba must expose the daemon-originated message through its own
   `poll_received_messages()` path, surfaced by the control shim.

2. `Columba -> reticulumd`
   `reticulumd` must persist the Columba-originated reply in its SQLite
   `messages` table with:
   - `direction = 'in'`
   - `source = columba_hash`
   - `destination = daemon_hash`

## Artifacts

The report contains:

- daemon delivery hash
- Columba delivery hash
- proof message bodies
- temp/log artifact paths

The report contract is defined in
[external-client-interop-acceptance-v1.md](../contracts/external-client-interop-acceptance-v1.md).
