# Sideband <-> reticulumd Interop

This runbook executes a reproducible external-client interoperability proof
between `reticulumd` and Sideband.

## What It Proves

- `reticulumd` can send a real LXMF message to Sideband over a Reticulum TCP
  interface.
- Sideband can send a real LXMF reply back to `reticulumd`.
- Both directions are checked through each side's own observable state:
  - Sideband persisted message store via the control shim
  - `reticulumd` message store

## Prerequisites

- Sideband source checkout exists locally.
- `python3` can import `RNS` and `LXMF`.
- The Rust workspace builds `reticulumd` and `lxmf-cli`.

Default Sideband path:

```bash
../Sideband
```

Override it with `SIDEBAND_ROOT` if needed.

## Run

```bash
./tools/scripts/sideband-reticulumd-smoke.sh
```

Useful overrides:

```bash
SIDEBAND_ROOT=/path/to/Sideband \
REPORT_PATH=target/interop/sideband-report.json \
RETICULUMD_DIAGNOSTICS=1 \
./tools/scripts/sideband-reticulumd-smoke.sh
```

## Expected Output

```text
[sideband-reticulumd-smoke] pass
[sideband-reticulumd-smoke] report=...
[sideband-reticulumd-smoke] logs=...
```

## Verification Model

The script verifies both directions:

1. `reticulumd -> Sideband`
   Sideband must expose the daemon-originated message through its own persisted
   message store, decoded via the control shim.

2. `Sideband -> reticulumd`
   `reticulumd` must persist the Sideband-originated reply in its SQLite
   `messages` table with:
   - `direction = 'in'`
   - `source = sideband_hash`
   - `destination = daemon_hash`

## Artifacts

The report contains:

- daemon delivery hash
- Sideband delivery hash
- proof message bodies
- temp/log artifact paths

The report contract is defined in
[external-client-interop-acceptance-v1.md](../contracts/external-client-interop-acceptance-v1.md).
