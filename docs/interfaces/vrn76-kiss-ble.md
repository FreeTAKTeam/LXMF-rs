# VR-N76 KISS-over-BLE Interface

## Purpose

`vrn76_kiss_ble` carries opaque Reticulum packets over the Bluetooth KISS path
used by VT-N76/VR-N76-class radios. The interface keeps LXMF out of the radio
transport boundary:

```text
Reticulum packet bytes -> KISS frame -> Benshi HT_SEND_DATA -> BLE write-with-response
BLE indication -> Benshi DATA_RXD -> KISS decoder -> Reticulum packet bytes
```

KISS itself is bearer-agnostic framing: it can be carried over serial,
Bluetooth, Wi-Fi, or another byte-stream/link transport. Use this interface for
VT-N76/VR-N76 Bluetooth KISS operation. Use `kiss` for serial KISS TNCs and `ble_gatt`
for generic HDLC-over-GATT adapters. Use `kiss_tcp_client` when the same KISS
framing is exposed by a TCP bridge, including Wi-Fi-backed KISS devices.

## Supported Radios

The implementation targets VR-N76-compatible firmware that exposes the Benshi
BLE UUID profile:

- Service UUID: `00001100-d102-11e1-9b23-00025b00a5a5`
- Write characteristic UUID: `00001101-d102-11e1-9b23-00025b00a5a5`
- Indication characteristic UUID: `00001102-d102-11e1-9b23-00025b00a5a5`

The native implementation scans for a configured peripheral name, address, or
platform id, connects, discovers these UUIDs, subscribes to indications, and
writes outbound frames with response.
Outbound KISS bytes are fragmented into Benshi TNC data writes according to the
configured maximum BLE write payload length before the native backend writes to
the characteristic.

## Host Bluetooth Boundary

LXMF-rs owns the Reticulum packet, KISS framing, Benshi wrapping, daemon
configuration, runtime state, and probe surfaces for VR-N76 KISS-over-BLE.
Operating-system Bluetooth availability is outside this repository's
responsibility: adapter drivers, platform permissions, pairing or bonding,
radio trust prompts, and host BLE stack behavior must be configured and
validated on the target machine.

The native backend reports scan, connect, subscription, write, indication, and
disconnect outcomes from the host BLE stack, but it does not provision the OS
Bluetooth environment.

## Configuration

Build and run `reticulumd` with the feature enabled:

```powershell
cargo run -p reticulumd --features vrn76-kiss-ble -- --config .\daemon.toml
```

Example `reticulumd` config:

```toml
interfaces = [
  { type = "vrn76_kiss_ble", enabled = true, name = "vrn76-main", peripheral_id = "VR-N76", adapter = "Bluetooth", mtu = 564, max_write_len = 512, preamble_ms = 350, tx_tail_ms = 20, persistence = 64, slot_time_ms = 20, kiss_flow_control = false, scan_timeout_ms = 10000, connect_timeout_ms = 3000 }
]
```

The daemon also accepts the #197 compatibility aliases used by the original
VR-N76 interface sketch:

```toml
[interfaces.vrn76_kiss_ble]
type = "Vrn76KissBluetoothInterface"
enabled = true
device_name_filter = "VR-N76"
device_address = ""
ble_scan_timeout_ms = 10000
command_timeout_ms = 3000
mtu = 564
preamble = 350
txtail = 20
persistence = 64
slottime = 20
flow_control = false
mode = "full"
outgoing = true
```

Both `interfaces = [ ... ]` and `[interfaces.<name>]` TOML shapes are accepted.
For table-style entries, `<name>` becomes the default interface name and, if no
`type` is supplied, the default interface type.

Fields:

- `type`: use `vrn76_kiss_ble` for the daemon-native spelling. The
  Reticulum-style aliases `Vrn76KissBluetoothInterface` and
  `Vrn76KissBleInterface` normalize to the same interface kind.
- `peripheral_id`: required. Matches the BLE peripheral name, address, or
  platform id after case-insensitive punctuation normalization.
- `device_name_filter` and `device_address`: compatibility aliases for
  `peripheral_id`. A non-empty `device_address` wins over `device_name_filter`
  when `peripheral_id` is not set.
- `adapter`: optional. Matches a specific host BLE adapter.
- `frame_mode`: optional. `benshi_tnc_data` (default) wraps KISS bytes in
  VR-N76 Benshi TNC data messages. `raw_kiss` sends and receives KISS bytes
  directly for firmware or host stacks that expose raw KISS rather than the
  VR-N76 BLE UUID profile.
- `mtu`: optional KISS payload MTU. Default `564`.
- `max_write_len`: optional BLE write payload limit. Default `512`. In Benshi
  mode, each `HT_SEND_DATA` write carries one ordered TNC fragment and stays
  within this limit.
- `preamble_ms`: optional KISS `CMD_TXDELAY` source value. Default `350`.
- `preamble`: compatibility alias for `preamble_ms`.
- `tx_tail_ms`: optional KISS `CMD_TXTAIL` source value. Default `20`.
- `txtail`: compatibility alias for `tx_tail_ms`.
- `persistence`: optional KISS `CMD_P` value. Default `64`.
- `slot_time_ms`: optional KISS `CMD_SLOTTIME` source value. Default `20`.
- `slottime`: compatibility alias for `slot_time_ms`.
- `kiss_flow_control`: optional KISS READY gating. Default `false`.
- `flow_control`: boolean compatibility alias for `kiss_flow_control` on this
  interface. The daemon still treats `flow_control` as a serial line-control
  string on the `serial` interface.
- `scan_timeout_ms`: optional BLE scan timeout. Default `10000`.
- `ble_scan_timeout_ms`: compatibility alias for `scan_timeout_ms`.
- `connect_timeout_ms`: optional connect, command, and indication timeout.
  Default `3000`.
- `command_timeout_ms`: compatibility alias for `connect_timeout_ms`.
- `outgoing`: accepted for compatibility with Reticulum-style examples and
  surfaced in settings metadata; current send/receive behavior is controlled by
  the interface mode and daemon transport manager.
- `reconnect_backoff_ms` and `max_reconnect_backoff_ms`: optional daemon
  reconnect backoff controls.

## KISS Rules

The KISS codec is independent from Bluetooth code. It uses `FEND` frame
boundaries, escapes `FEND` and `FESC`, emits only `CMD_DATA` payloads to the
Reticulum receive path, treats `CMD_READY` as flow-control state, and rejects
malformed escapes or oversized decoded payloads.
For Python BLE KISS read-loop parity, stale partial inbound KISS frames are
discarded after `1250 ms` before later indication bytes are decoded.

If a single BLE indication decodes to more than one KISS `CMD_DATA` frame, the
runtime queues the decoded Reticulum packet payloads and emits them one at a
time through the receive path.

Each connect/configure cycle starts a fresh KISS/Benshi session and clears
stale decoded-packet, fragment, flow-control, and queued-write state from any
previous BLE connection.

On startup the runtime subscribes to indications before sending KISS parameter
commands:

- `CMD_TXDELAY` from `preamble_ms / 10`
- `CMD_TXTAIL` from `tx_tail_ms / 10`
- `CMD_P` from `persistence`
- `CMD_SLOTTIME` from `slot_time_ms / 10`
- `CMD_READY 0x01` for Python KISS startup parity, regardless of whether
  runtime `kiss_flow_control` gating is enabled

Command values are clamped into one byte by the KISS configuration encoder.
If the radio rejects or ignores these startup KISS command writes, the runtime
keeps the Bluetooth session connected and records `startup_write_failures` in
the status snapshot. Later data writes still fail normally if the underlying
Bluetooth link or write characteristic is unavailable.

## MTU Behavior

The default interface MTU is `564`, matching the current VR-N76 KISS profile
assumption. The daemon validates configured MTUs in the KISS range
`64..=65535`. Outbound Reticulum packet payloads larger than the configured MTU
are rejected with a typed transport error before any BLE write is attempted.
The default maximum BLE write payload length is `512`; Benshi mode fragments
encoded KISS bytes into ordered TNC fragments before writing to the BLE
characteristic. Hardware validation may justify lowering this for specific
firmware or OS BLE stacks.

## Probe Example

The transport crate includes a feature-gated probe example:

```powershell
cargo run -p reticulum-rs-transport --features vrn76-kiss-ble --example vrn76_kiss_ble_probe -- --peripheral-id VR-N76
```

The probe scans, connects, subscribes, sends KISS configuration frames, and
prints `connected`, `subscribed`, `interface_ready`, `pending_payloads`,
`startup_write_failures`, `pending_payloads`, `pending_writes`, and
`pending_packets` from the runtime status snapshot. It
does not transmit a test KISS data frame unless explicitly requested:

```powershell
cargo run -p reticulum-rs-transport --features vrn76-kiss-ble --example vrn76_kiss_ble_probe -- --peripheral-id VR-N76 --send-test-kiss-frame
```

Use `--test-payload-hex <hex>` to send a specific explicit test KISS payload.

## Prepared-Host Smoke

Prepared-host smoke evidence is captured by
`tools/scripts/vrn76-kiss-ble-prepared-host-smoke.sh`. The harness builds the
feature-gated daemon, starts a `vrn76_kiss_ble` interface on a host that already
has Bluetooth and the target radio provisioned, polls daemon/RPC status through
`rnstatus-rs`, and writes `report.json`, logs, generated config, and status
snapshots under `target/vrn76-hil/`.

This confirms daemon startup, scan/connect/subscribe, interface readiness, and
runtime counter visibility for a prepared host. It does not provision the host
Bluetooth adapter, perform pairing or bonding, or replace broader disconnect
and reconnect validation.

## Known Limitations

- Hardware-backed scan, connect, subscribe, write, indication, disconnect, and
  reconnect evidence depends on a prepared host Bluetooth environment and real
  VR-N76 hardware. That validation is an integration prerequisite, not an OS
  provisioning responsibility of this repository.
- The Benshi `TncDataFragment` implementation supports ordered multi-fragment
  reassembly with or without the optional trailing channel ID. Channel IDs are
  stripped from payload bytes and must remain stable across a fragment sequence.
  Outbound Benshi writes now generate ordered fragments when encoded KISS bytes
  exceed `max_write_len`.
- RFCOMM is not used. Add it only if firmware evidence proves BLE
  write/indication transport is insufficient.
- The interface reports daemon startup state through the normal interface
  manager. The transport-level runtime exposes connection, subscription,
  flow-control readiness, and queued-write counters; it does not expose a richer
  radio status model yet.

## Hardware Validation Checklist

- Confirm the host adapter discovers the configured `peripheral_id`.
- Confirm the VR-N76 service and write/indication characteristics are present.
- Confirm write-with-response succeeds for startup KISS command frames.
- Confirm indications arrive after subscription.
- Confirm outbound Reticulum packet bytes are KISS-framed and transmitted.
- Confirm inbound Benshi `DATA_RXD` indications unwrap to KISS `CMD_DATA`.
- Confirm multi-fragment Benshi `DATA_RXD` indications preserve byte order on
  the target firmware.
- Confirm observed Benshi channel IDs match the expected firmware channel
  behavior.
- Confirm READY flow control gates queued outbound payloads when enabled.
- Confirm reconnect works after peripheral disconnect or adapter interruption.
