# reticulumd Columba-Compatible Reticulum BLE Interface

This runbook documents the `reticulum_ble` daemon interface for Columba and
`ble-reticulum` Protocol v2.2 peer links. It is separate from `ble_gatt`, which
remains the generic configured-peripheral GATT adapter.

## Interface Kind

- Primary kind: `reticulum_ble`
- Compatibility aliases: `AndroidBLE`, `AndroidBLEInterface`, `BLEInterface`
- Runtime status key: `_runtime.reticulum_ble`
- Runtime dependency boundary: BLE OS dependencies stay in the `reticulumd`
  adapter layer; `rns-transport` remains BLE-agnostic.

## Default GATT Shape

The daemon defaults to Columba Protocol v2.2 UUIDs:

- Service: `37145b00-442d-4a94-917f-8f42c5da28e3`
- TX notification characteristic: `37145b00-442d-4a94-917f-8f42c5da28e4`
- RX write characteristic: `37145b00-442d-4a94-917f-8f42c5da28e5`
- Identity characteristic: `37145b00-442d-4a94-917f-8f42c5da28e6`

The identity value is the daemon transport identity hash, exactly 16 bytes.

## Example

```toml
interfaces = [
  { type = "reticulum_ble", enabled = true, name = "columba-ble" }
]
```

Optional peer settings:

```toml
interfaces = [
  {
    type = "reticulum_ble",
    enabled = true,
    name = "columba-ble",
    adapter = "hci0",
    mtu = 247,
    max_connections = 4,
    scan_duration_ms = 10000,
    discovery_interval_ms = 5000,
    discovery_interval_idle_ms = 30000,
    advertising_refresh_interval_ms = 30000,
    min_rssi_dbm = -85,
    enable_central = true,
    enable_peripheral = true,
  }
]
```

## Protocol Notes

The central flow is:

1. Scan for the service UUID.
2. Connect and discover GATT services.
3. Read the identity characteristic.
4. Request MTU.
5. Subscribe to TX notifications.
6. Write the local 16-byte identity to RX.

The peripheral flow is:

1. Advertise the service UUID.
2. Serve RX, TX, and identity characteristics.
3. Treat the first 16-byte RX write as the peer identity.
4. Key peer state by identity, not BLE address, so MAC rotation updates the
   active address rather than creating duplicate Reticulum peers.

When central and peripheral links exist to the same identity, the lower stable
identity hash keeps the central role. The other direction is closed without
dropping the peer record.

## Fragmentation

The Rust fragment codec mirrors pinned `ble-reticulum` commit
`07d941304c9a1dc3a8e58087b3b974ff3d229e56`:

- Header: 5 bytes, `type: u8`, `sequence: u16`, `total: u16`
- Byte order: network/big-endian for sequence and total
- Types: `0x01` start, `0x02` continue, `0x03` end
- Payload per fragment: negotiated peer MTU minus 5 bytes
- Reassembly timeout: 30 seconds
- Fragments larger than 512 bytes are rejected and counted

Keepalive uses a raw one-byte `0x00` on idle links every 15 seconds; links are
disconnected after three failed keepalives.

## Status

`_runtime.reticulum_ble.status` reports the configured UUIDs, local identity,
role enablement, scan and advertising state, peer identities, active addresses,
MTU, fragment and packet counters, duplicate role rejections, stale reassembly
drops, reconnects, malformed fragments, and the last runtime error.

## Native Backend Status

The daemon now has config parsing, runtime status plumbing, Columba-compatible
fragment/reassembly logic, identity-based peer bookkeeping, and software unit
coverage. The OS-specific dual-role BLE backend is intentionally isolated behind
the daemon adapter boundary. Until that backend is implemented for the target
platform, `reticulum_ble` starts with scan and advertising states marked
`native_backend_pending`.

`ble_gatt`, RNode BLE, and VR-N76 BLE behavior are unchanged.
