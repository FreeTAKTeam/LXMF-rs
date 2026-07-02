# Meshtastic Tunnel Interface

## Purpose

The Meshtastic interface carries opaque Reticulum packet bytes through
Meshtastic application payloads. It implements the tunnel framing used by the
reference `RETICULUM_TUNNEL_APP` integration while keeping the radio, serial,
TCP, BLE, or protobuf bearer outside the Reticulum packet boundary:

```text
Reticulum packet bytes -> Meshtastic tunnel chunks -> bearer app payload writes
bearer app payload reads -> Meshtastic tunnel reassembly -> Reticulum packet bytes
```

Use this interface when an application already has a Meshtastic bearer and
wants Reticulum packet routing, chunking, retransmit requests, and destination
route learning in `reticulum-rs-transport`.

## Current Support Boundary

This is a transport-library interface, not a daemon-configured hardware
adapter yet.

Implemented:

- `rns_transport::iface::meshtastic` public types.
- Reference tunnel chunk metadata: first byte packet index, second byte signed
  chunk position, with negative position marking the final chunk.
- Missing-chunk request frames using `REQ` plus the requested index and
  position metadata.
- Modem-preset send pacing through `MeshtasticInterfaceConfig`.
- One-byte packet-index wrap handling without overwriting queued packets.
- Node route learning from inbound Reticulum packet destination fields.
- An injectable `MeshtasticInterfaceHandle` for serial, TCP, BLE, or native
  Meshtastic API adapters.
- Runtime counters through `MeshtasticTunnelStatus`.

Not implemented yet:

- `reticulumd` TOML startup for `MeshtasticInterface`.
- Native serial, TCP, BLE, or protobuf Meshtastic device discovery.
- Prepared-host Meshtastic hardware smoke evidence.
- Channel selection beyond the current outbound `channel_index = 0` default.
- Rich Meshtastic device or radio management.

Do not add a `MeshtasticInterface` entry to `reticulumd` config yet; enabled
unknown interface kinds are reported as unsupported startup records by the
daemon.

## Configuration

Use `MeshtasticInterfaceConfig` when constructing either a standalone
`MeshtasticTunnel` or a spawned `MeshtasticInterface`.

Defaults:

| Field | Default | Meaning |
| --- | ---: | --- |
| `hop_limit` | `7` | Hop limit copied to outbound `MeshtasticTransmitFrame` values. |
| `bitrate_bps` | `500` | Interface bitrate metadata for Reticulum pacing/accounting. |
| `max_payload_bytes` | `200` | Maximum Reticulum payload bytes carried in each tunnel chunk after the two metadata bytes. Must be greater than zero. |
| `send_delay` | `7s` | Delay between queued outbound Meshtastic tunnel transmissions. |
| `destination_cache_size` | `20` | Number of Reticulum destination-to-node routes retained for direct replies. |

The advertised interface MTU is `564`, matching the current Meshtastic hardware
MTU assumption. `max_payload_bytes` is deliberately lower so app payloads leave
space for Meshtastic transport overhead and device-specific limits.

### Modem Presets

`MeshtasticInterfaceConfig::from_modem_preset(preset)` keeps the default
fields and selects a send delay for common Meshtastic modem presets:

| Preset | Send delay |
| ---: | ---: |
| `8` | `400ms` |
| `6` | `1s` |
| `5` | `3s` |
| `7` | `12s` |
| `4` | `4s` |
| `3` | `6s` |
| `1` | `15s` |
| `0` | `8s` |
| other | `7s` |

Override `send_delay` directly when local channel conditions or region limits
require a more conservative pacing policy.

Example:

```rust
use std::time::Duration;

use rns_transport::iface::meshtastic::MeshtasticInterfaceConfig;

let mut config = MeshtasticInterfaceConfig::from_modem_preset(8);
config.hop_limit = 3;
config.max_payload_bytes = 180;
config.send_delay = Duration::from_secs(2);
config.destination_cache_size = 32;
```

## Library Usage

The lowest-level API is `MeshtasticTunnel`. It is useful for adapters that want
to own their own task scheduling:

```rust
use rns_transport::iface::meshtastic::{
    MeshtasticInterfaceConfig, MeshtasticReceivedFrame, MeshtasticTunnel,
};

let mut tunnel = MeshtasticTunnel::new(MeshtasticInterfaceConfig::default());

// Queue an outbound Reticulum packet byte buffer.
tunnel.queue_outgoing_packet(&reticulum_packet_bytes)?;

// Drain the next Meshtastic app payload to send through the bearer.
if let Some(frame) = tunnel.next_transmit() {
    // Send frame.payload through the Meshtastic RETICULUM_TUNNEL_APP port.
    // Use frame.destination, frame.hop_limit, and frame.channel_index when the
    // bearer API exposes those controls.
}

// Feed an inbound Meshtastic app payload back into the tunnel.
let received = MeshtasticReceivedFrame::new(from_node_id, &app_payload);
if let Some(reticulum_packet_bytes) = tunnel.process_received(received)? {
    // Decode or forward the complete Reticulum packet bytes.
}
```

`queue_outgoing_packet` rejects payloads that cannot be split into at most
`i8::MAX` chunks. It also refuses to reuse a one-byte packet index while queued
chunks for an older packet with the same index are still pending.

## InterfaceManager Usage

Use `spawn_meshtastic` when the Meshtastic tunnel should behave like a normal
Reticulum interface inside `InterfaceManager` while an external bearer task
does the actual device I/O:

```rust
use rns_transport::iface::InterfaceManager;
use rns_transport::iface::meshtastic::{
    spawn_meshtastic, MeshtasticInterfaceConfig, MeshtasticReceivedFrame,
};

let mut manager = InterfaceManager::new(128);
let (_iface_address, handle) = spawn_meshtastic(
    &mut manager,
    "mesh-main",
    MeshtasticInterfaceConfig::from_modem_preset(8),
);

// In the bearer receive path:
handle
    .inject_received(MeshtasticReceivedFrame::new(from_node_id, &app_payload))
    .await?;

// In the bearer transmit path:
if let Some(frame) = handle.recv_transmit().await {
    // Map frame.destination to broadcast or direct Meshtastic send.
    // Write frame.payload as the Meshtastic app payload.
}
```

`MeshtasticInterfaceHandle::inject_received` accepts only the Meshtastic app
payload bytes for the Reticulum tunnel app. Strip any Meshtastic protobuf,
serial framing, BLE packet framing, or device envelope before constructing
`MeshtasticReceivedFrame`.

`MeshtasticInterfaceHandle::recv_transmit` yields `MeshtasticTransmitFrame`:

| Field | Adapter responsibility |
| --- | --- |
| `destination` | Broadcast when `MeshtasticDestination::Broadcast`; direct-send to a node when `MeshtasticDestination::Node(id)`. |
| `payload` | Write as the Meshtastic tunnel app payload. |
| `hop_limit` | Apply to the Meshtastic send request when supported by the bearer. |
| `want_ack` | Currently `false`; pass through if the bearer requires an explicit value. |
| `want_response` | Currently `false`; pass through if the bearer requires an explicit value. |
| `channel_index` | Currently `0`; pass through to the Meshtastic channel selector when supported. |

## Routing Behavior

The tunnel learns direct node routes from complete inbound Reticulum packet
bytes. If a packet contains a Reticulum destination field, the tunnel remembers
that destination hash as reachable through the Meshtastic node that sent the
frame. Later outbound packets for that destination use
`MeshtasticDestination::Node(node_id)` instead of broadcast until the route is
evicted from the bounded destination cache.

This cache is local runtime state. It is not persisted and it is not a
substitute for Reticulum path-table persistence.

## Retransmit Behavior

Inbound chunks are reassembled by packet index and chunk position. When the
tunnel sees a later chunk before an expected earlier chunk, it queues a request
frame:

```text
"REQ" || packet_index || missing_position
```

The request is sent as a normal outbound Meshtastic tunnel payload. A peer that
receives the request retransmits the referenced chunk if the packet is still in
its local outgoing storage. When multiple adjacent chunks are missing, the
tunnel keeps requesting the next missing position after each repaired non-final
chunk arrives.

## Status

`MeshtasticTunnel::status()` and `MeshtasticInterface::runtime_status_json()`
expose:

| Field | Meaning |
| --- | --- |
| `queued_transmissions` | Pending chunk or request frames waiting to be sent. |
| `destination_routes` | Learned destination-to-node route entries. |
| `packets_rx` | Complete Reticulum packets reassembled from Meshtastic chunks. |
| `packets_tx` | Complete Reticulum packets whose final chunk has been transmitted. |
| `chunks_rx` | Meshtastic tunnel chunks or request frames received. |
| `chunks_tx` | Meshtastic tunnel chunks or request frames transmitted. |
| `requested_retransmits` | Missing-chunk request frames queued locally. |
| `decode_errors` | Malformed Meshtastic chunks or Reticulum packet decode failures. |
| `last_error` | Last recorded tunnel, queue, or decode error. |

## Validation

Focused coverage lives in:

```powershell
cargo test -p reticulum-rs-transport --test meshtastic_interface
```

That test target covers reference metadata splitting/reassembly,
modem-preset pacing, missing-chunk requests, chained repair after adjacent
loss, one-byte packet-index wrap protection, empty-payload handling, and
destination route learning for direct replies.
