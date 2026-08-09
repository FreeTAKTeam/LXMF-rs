# VR-N76 KISS BLE Interface

This runbook tracks the transport-layer VR-N76 KISS-over-BLE compatibility
slice and the feature-gated `reticulumd` runtime interface. The generic
`ble_gatt` interface remains available for HDLC-over-GATT adapters; VR-N76
devices should use the `vrn76_kiss_ble` interface kind because it applies the
VR-N76 UUID profile and Benshi TNC data wrapping.

KISS is the packet framing layer, not the physical bearer. Serial KISS, KISS
over Bluetooth, and KISS over Wi-Fi/TCP or other links can share framing
semantics while requiring different connection setup. For VR-N76-class radios
this slice targets the Bluetooth bearer; Wi-Fi/TCP KISS bridges should use
`kiss_tcp_client`.

## Profile

- BLE service UUID: `00001100-d102-11e1-9b23-00025b00a5a5`
- BLE write characteristic UUID: `00001101-d102-11e1-9b23-00025b00a5a5`
- BLE indication characteristic UUID: `00001102-d102-11e1-9b23-00025b00a5a5`
- Writes use write-with-response.
- Outbound KISS bytes are split into Benshi `TncDataFragment` writes by the
  configured BLE maximum write length.
- Indications are subscribed before KISS configuration frames are sent.
- KISS data frames carry opaque Reticulum packet bytes.

## Frame Modes

The default `Vrn76FrameMode::BenshiTncData` follows the BLE command protocol
used by `benlink`: KISS bytes are placed in a Benshi `HT_SEND_DATA` message
with a `TncDataFragment`, and inbound `DATA_RXD` event notifications are
reassembled and unwrapped before the KISS decoder sees the payload.

`Vrn76FrameMode::RawKiss` remains available for firmware or host stacks that
expose Bluetooth KISS bytes directly, for example through an RFCOMM/serial TNC
path. Raw mode is not the default for the VR-N76 BLE UUID profile.

## Runtime Contract

The transport crate exposes a backend-neutral runtime:

- `Vrn76KissBleBackend` connects, subscribes to indications, writes BLE payloads,
  and returns the next indication bytes.
- `Vrn76KissBleRuntime` applies the VR-N76 session state on top of that backend.
  It sends startup KISS configuration frames after subscription, wraps outbound
  packet bytes, unwraps inbound indication bytes, and flushes queued writes when
  KISS READY flow control allows transmission.

Startup KISS command write failures are non-fatal for VR-N76 sessions because
some firmware may ignore or reject parameter commands while still accepting data
frames. The runtime records these as `startup_write_failures`; later packet
writes still mark the runtime disconnected if the Bluetooth write path fails.

Native platform BLE code should implement `Vrn76KissBleBackend` and keep the
Reticulum/LXMF boundary opaque: the runtime only handles byte packets and KISS
framing.

With the `vrn76-kiss-ble` Cargo feature enabled, `rns-transport` also exposes
`NativeVrn76BleBackend`, `NativeVrn76BleSettings`, and
`NativeVrn76KissBleInterface`. The native backend uses `btleplug` to scan for a
configured peripheral name/address/id, connect, discover the VR-N76 UUID
profile, subscribe to indications, and write outbound frames with response. The
native interface pairs that backend with `Vrn76KissBleRuntime` and feeds decoded
Reticulum packets into the normal interface manager.

The repository boundary stops at this backend/runtime contract. OS-dependent
Bluetooth readiness is a host responsibility: the operator must provide a
working adapter, platform permissions, pairing or bonding state, and any radio
trust prompts required by the target operating system.
The daemon intentionally does not treat Python-style `RNodeInterface`
`ble://...` ports as serial LoRa/RNode devices; VT-N76/VR-N76 Bluetooth KISS
configuration should use this `vrn76_kiss_ble` interface.

## reticulumd Configuration

Enable the daemon feature and configure a `vrn76_kiss_ble` interface. For
VT-N76/VR-N76 radios, this is the Bluetooth KISS path; serial KISS TNCs use
`kiss`, and Wi-Fi/TCP KISS bridges use `kiss_tcp_client`:

```powershell
cargo run -p reticulumd --features vrn76-kiss-ble -- --config .\daemon.toml
```

```toml
interfaces = [
  { type = "vrn76_kiss_ble", enabled = true, name = "vrn76-main", peripheral_id = "VR-N76", adapter = "Bluetooth", kiss_flow_control = true, max_write_len = 512 }
]
```

`frame_mode = "benshi_tnc_data"` is the default for the VR-N76 BLE UUID
profile. Use `frame_mode = "raw_kiss"` only when the host stack or firmware
exposes direct KISS bytes over the selected Bluetooth path.

The Reticulum-style type alias and field aliases from #197 are accepted and
normalized to the same daemon model:

```toml
[interfaces.vrn76_kiss_ble]
type = "Vrn76KissBluetoothInterface"
enabled = true
device_name_filter = "VR-N76"
ble_scan_timeout_ms = 10000
command_timeout_ms = 3000
mtu = 564
preamble = 350
txtail = 20
persistence = 64
slottime = 20
flow_control = false
id_callsign = "MYCALL-0"
id_interval = 600
mode = "full"
outgoing = true
bitrate = 1200
announce_cap = 2
```

The daemon accepts both `interfaces = [ ... ]` and `[interfaces.<name>]` TOML
shapes. Table-style entries use `<name>` as the default interface name and type
when those fields are omitted.

Supported settings are:

- `type`: `vrn76_kiss_ble`, `Vrn76KissBluetoothInterface`, or
  `Vrn76KissBleInterface`.
- `peripheral_id`: required name, address, or platform id match for the radio.
  The daemon also accepts `device_name_filter` or `device_address` as
  compatibility aliases; a non-empty `device_address` wins when both aliases are
  provided and `peripheral_id` is absent.
- `adapter`: optional host adapter name/id match.
- `mtu`, `preamble_ms`, `tx_tail_ms`, `persistence`, `slot_time_ms`,
  `kiss_flow_control`: optional KISS/profile tuning fields.
- `max_write_len`: optional maximum BLE write payload length. In Benshi mode,
  outbound KISS bytes are fragmented so each `HT_SEND_DATA` write stays within
  this limit.
- `preamble`, `txtail`, `slottime`, and boolean `flow_control`: compatibility
  aliases for the KISS/profile tuning fields.
- `id_callsign`, `id_interval`: optional KISS station-ID beacon settings. The
  VR-N76 path uses the same Python KISS 15-byte minimum ID payload padding as
  serial and TCP KISS, treats a missing callsign with `id_interval` as an empty
  padded payload, suppresses its own ID beacon on receive, and emits station ID
  frames after outbound activity.
- `scan_timeout_ms`, `connect_timeout_ms`: optional native BLE lifecycle
  timeouts. The `ble_scan_timeout_ms` and `command_timeout_ms` aliases are also
  accepted.
- `outgoing`: defaults to `true`. Set `outgoing = false` to keep the Bluetooth
  KISS interface available for inbound packets while suppressing
  daemon-initiated outbound broadcast and direct transmissions on that
  interface.
- `bitrate`, `announce_cap`: optional Reticulum-style per-interface announce
  pacing controls. `bitrate` is bits per second; `announce_cap` is a percentage
  in the range `1..=100`.
- `reconnect_backoff_ms`, `max_reconnect_backoff_ms`: optional reconnect
  backoff controls.

## Defaults

- MTU: `564`
- Maximum BLE write length: `512`
- Scan timeout: `10000 ms`
- Command timeout: `3000 ms`
- KISS read-frame timeout: `1250 ms`
- Frame mode: `BenshiTncData`
- KISS preamble: `350 ms`
- KISS tx tail: `20 ms`
- KISS persistence: `64`
- KISS slot time: `20 ms`
- KISS flow control: `false`

## Runtime Visibility

With `reticulumd --features vrn76-kiss-ble`, the native interface refreshes
daemon/RPC `_runtime.vrn76.status` metadata while it runs. `rnstatus-rs`
renders the same status as a compact row with connection, subscription,
readiness, startup KISS write failure, pending payload, pending write, and
pending packet counters.

## Prepared-Host Smoke

The prepared-host smoke is opt-in hardware evidence for hosts where Bluetooth
has already been provisioned and a VR-N76-class peripheral is ready to connect:

```powershell
VRN76_PERIPHERAL_ID=VR-N76 \
VRN76_ADAPTER=Bluetooth \
VRN76_TIMEOUT_SECS=180 \
./tools/scripts/vrn76-kiss-ble-prepared-host-smoke.sh
```

The script builds `reticulumd --features vrn76-kiss-ble`, starts the daemon
with `--strict-interface-startup`, and polls both `rnstatus-rs --json` and
human `rnstatus-rs` output. It writes the generated config, daemon log,
`rnstatus-rs` JSON/human output, and `report.json` under `target/vrn76-hil/`.

Passing evidence requires the `vrn76-prepared-host` row to report
`_runtime.vrn76.status.connected = true`,
`_runtime.vrn76.status.subscribed = true`, and
`_runtime.vrn76.status.interface_ready = true`. The report also records
`startup_write_failures`, `pending_payloads`, `pending_writes`, and
`pending_packets` as non-negative runtime counters. It also records
`evidence_scope = "prepared_host_vrn76_ble_readiness"` plus a
`product_boundary` note that broader hardware parity still requires write,
indication, disconnect, reconnect, adapter, firmware, and channel-ID evidence.

The nightly HIL matrix exposes the same harness through the `vr-n76` profile.
Repository variables can provide `HIL_VRN76_PERIPHERAL_ID`,
`HIL_VRN76_ADAPTER`, `HIL_VRN76_MTU`, `HIL_VRN76_MAX_WRITE_LEN`,
`HIL_VRN76_FRAME_MODE`, `HIL_VRN76_KISS_FLOW_CONTROL`,
`HIL_VRN76_SCAN_TIMEOUT_MS`, `HIL_VRN76_CONNECT_TIMEOUT_MS`, and
`HIL_VRN76_TIMEOUT_SECS`.

## Current Verification

```powershell
cargo test -p reticulum-rs-transport --test vrn76_kiss_ble
cargo test -p reticulum-rs-transport --features vrn76-kiss-ble --test vrn76_kiss_ble
cargo check -p reticulum-rs-transport --features vrn76-kiss-ble --example vrn76_kiss_ble_probe
cargo test -p reticulumd --test config vrn76
cargo test -p reticulumd --features vrn76-kiss-ble --bin reticulumd vrn76_builder
cargo test -p reticulumd --features vrn76-kiss-ble --bin reticulumd vrn76_runtime_status_refresh_updates_matching_interface_record
cargo test -p reticulumd --test vrn76_prepared_host_smoke_contract
cargo test -p rns-tools --bin rnstatus-rs human_status_includes_interface_runtime_detail
```

The current tests cover profile constants, Benshi `HT_SEND_DATA` wrapping,
`DATA_RXD` event unwrapping, raw KISS compatibility mode, write-with-response
KISS framing, split indication decoding, ordered Benshi `TncDataFragment`
reassembly, channel-ID stripping and consistency checks, out-of-order fragment
rejection, startup ordering, backend lifecycle ordering, backend writes,
inbound polling, native backend settings, native identifier matching, and
READY-gated flow control, including non-fatal startup KISS command write
failures. They also cover Python-compatible KISS station-ID beacon config,
15-byte payload padding, own-beacon suppression, and backend writes over the
VR-N76 BLE frame mode. Outbound Benshi writes are covered for configured
maximum BLE write length fragmentation. Stale partial inbound KISS frames are
discarded after the Python BLE read timeout before later Benshi or raw KISS
indication bytes are decoded. Daemon/RPC runtime status refresh and
`rnstatus-rs` summary rendering are covered without requiring Bluetooth
hardware.

The probe example covers argument parsing, lifecycle setup, KISS configuration
submission, startup command failure counting, runtime status printing, and
explicit opt-in test-frame transmission at compile time. It still requires
hardware-backed execution for scan/connect/write/indication evidence on a host
where Bluetooth has already been provisioned.

The prepared-host smoke contract covers the generated HIL script, nightly
workflow wiring, and documentation artifacts without requiring Bluetooth
hardware.

## Remaining Work

- Confirm hardware behavior for Benshi `TncDataFragment` channel IDs matches
  the implemented parser semantics.
- Run the prepared-host smoke against real VR-N76 hardware and capture
  scan/connect/subscribe/readiness evidence. Do not treat OS Bluetooth adapter
  setup or device bonding as an LXMF-rs implementation task.
- Capture broader hardware or adapter-backed integration evidence for write,
  indication, disconnect, and reconnect behavior on a prepared host.
