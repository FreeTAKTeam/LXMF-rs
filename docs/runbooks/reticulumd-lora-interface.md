# `reticulumd` LoRa Interface Runbook

## Purpose

This runbook documents `lora` startup policy, active serial device startup, state persistence behavior, and fail-closed recovery steps.

## Scope

- Interface kind: `lora`
- Reticulum type alias: `RNodeInterface`
- Startup lifecycle: daemon bootstrap only
- Active transport: started when `device`/`baud_rate`, a `tcp://` RNode port,
  or a feature-gated `ble://` RNode port is configured
- Runtime mutation policy: `set_interfaces`/`reload_config` with `lora` changes require restart
- Compliance posture: fail-closed on uncertain duty-cycle state

For the in-progress shared-serial, multi-vport RNode family, see
`docs/runbooks/reticulumd-rnode-multi-interface.md`.

## Required Config Fields

```toml
interfaces = [
  {
    type = "lora",
    enabled = true,
    name = "lora-main",
    region = "US915",
    state_path = "var/reticulumd/lora-state.json",
    device = "/dev/ttyACM0",
    baud_rate = 115200,
    spreading_factor = 9,
    coding_rate = "4/5",
    bandwidth_hz = 125000,
    airtime_limit_short = 33.0,
    airtime_limit_long = 1.5,
    max_payload_bytes = 220
  }
]
```

Reticulum-style RNode configuration keys are also accepted for migration from
Python configs. `type = "RNodeInterface"` normalizes to `lora`, `port` maps to
the serial `device`, and `frequency`, `bandwidth`, `spreadingfactor`,
`codingrate`, and `txpower` map to the corresponding Rust field names. The
Python alias defaults serial `baud_rate` to `115200` when `speed` is omitted.
Like Python, the `RNodeInterface` alias requires explicit `frequency`,
`bandwidth`, `spreadingfactor`, and `codingrate` values instead of relying on
generic LoRa region defaults. When `region` is omitted, the compatibility path
uses `US915` as the radio-config seed, and `state_path` is optional because
Python RNode configs do not use the daemon LoRa duty-cycle state file:

```toml
interfaces = [
  {
    type = "RNodeInterface",
    enabled = true,
    name = "rnode-main",
    port = "/dev/ttyACM0",
    frequency = 915000000,
    bandwidth = 125000,
    spreadingfactor = 9,
    codingrate = 5,
    txpower = 17,
    outgoing = true,
    bitrate = 1200,
    announce_cap = 2,
    command_timeout_ms = 1500,
    id_callsign = "MYCALL-0",
    id_interval = 600
  }
]
```

RNode Wi-Fi/TCP ports are accepted with the Python-style `tcp://` prefix. They
do not require serial line settings:

```toml
interfaces = [
  {
    type = "RNodeInterface",
    enabled = true,
    name = "rnode-wifi",
    port = "tcp://192.0.2.10:8001",
    frequency = 915000000,
    bandwidth = 125000,
    spreadingfactor = 9,
    codingrate = 5,
    txpower = 17
  }
]
```

Python-style `RNodeInterface` `ble://...` ports are accepted for native
RNode BLE startup when `reticulumd` is built with the `rnode-ble` feature. The
daemon records a failed startup status instead of treating the port as serial
when the feature is disabled:

```toml
interfaces = [
  {
    type = "RNodeInterface",
    enabled = true,
    name = "rnode-ble",
    port = "ble://RNode 1234",
    adapter = "Bluetooth",
    frequency = 915000000,
    bandwidth = 125000,
    spreadingfactor = 9,
    codingrate = 5,
    txpower = 17,
    scan_timeout_ms = 2000,
    ble_connect_timeout_ms = 5000,
    command_timeout_ms = 1500,
    max_write_len = 20
  }
]
```

Android-style RNode selector fields are accepted as migration aliases for the
same native BLE backend. `ble_addr` or `target_device_address` is preferred
over `ble_name` or `target_device_name`, and `allow_bluetooth = true` requires
one of those selectors unless an explicit `port = "ble://..."` is already
configured:

```toml
interfaces = [
  {
    type = "RNodeInterface",
    enabled = true,
    name = "rnode-android-ble",
    allow_bluetooth = true,
    target_device_address = "AA:BB:CC:DD:EE:FF",
    frequency = 915000000,
    bandwidth = 125000,
    spreadingfactor = 9,
    codingrate = 5,
    txpower = 17
  }
]
```

Adapter discovery, permissions, pairing, bonding, and trust prompts are host OS
responsibilities outside this repository. For VT-N76/VR-N76 Bluetooth KISS
operation, use the `vrn76_kiss_ble` interface because those devices use the
VR-N76 BLE command profile rather than the generic Nordic UART RNode profile.
The transport layer now exposes the Python RNode BLE Nordic UART profile
constants and defaults (`6E400001-B5A3-F393-E0A9-E50E24DCCA9E` service,
`6E400002-B5A3-F393-E0A9-E50E24DCCA9E` write characteristic,
`6E400003-B5A3-F393-E0A9-E50E24DCCA9E` notify characteristic, write without
response, two-second scan timeout, five-second connect timeout, and 1250 ms
read-frame timeout). It also exposes a raw-KISS BLE session that subscribes
before sending KISS setup commands, writes raw KISS frames to the UART RX
characteristic without response, decodes raw notification bytes from the UART
TX characteristic, and honors READY-based flow control. The BLE session also
mirrors the existing KISS/RNode timeout and station-ID behavior by discarding
stale partial notification frames after the Python BLE read timeout and
suppressing its own station-ID beacon if it is received back from the radio.
Outbound RNode BLE station-ID beacons are emitted as raw KISS data frames to
the UART RX characteristic and queue behind READY-based flow control when that
mode is enabled. A backend-neutral runtime contract now connects, subscribes to
notifications, writes startup and outbound KISS frames, polls notifications,
and flushes pending READY-gated writes through the same session state. With
the `rnode-ble` Cargo feature enabled, the transport crate also exposes a
native `btleplug` backend that scans for a configured peripheral name, address,
or platform id, connects, discovers the Nordic UART service and
write/notification characteristics, subscribes to notifications, and writes
raw KISS payload chunks to the backend characteristic writer. Outbound BLE
packet writes are rejected before backend I/O when they exceed the configured
RNode BLE MTU, and encoded raw-KISS bytes are chunked by the configured maximum
BLE write length before they reach the backend. The native backend reads the
negotiated ATT MTU exposed by `btleplug` 0.12 after connect/service discovery,
raises the default write chunk size when the platform reports a larger payload,
and warns when a reported MTU remains below the 173-byte LXMF notification
minimum. A 20-byte partial notification is diagnosed as likely ATT MTU 23 /
20-byte payload evidence, which means the host/backend did not report a usable
larger negotiated MTU before notifications. The RNode BLE notification path also
preserves non-READY KISS command responses alongside decoded packet payloads,
and its command monitor exposes retained probe status, radio status, non-fatal
hardware errors, fatal command error, online state, and reported bitrate.
Daemon `RNodeInterface` `ble://` startup appends the same RNode
detect, firmware, platform, MCU, radio configuration, airtime-lock, and
radio-on command frames used by serial/TCP RNode startup and validates startup
and fatal command responses through the same RNode protocol state. If startup
probe identifies an ESP32 or NRF52 display-capable platform, BLE shutdown
prepends Python-style external-framebuffer disable before radio-off plus
leave-host frames and backend cleanup. The transport layer exposes
Python-compatible KISS management frame helpers for blink indication, Bluetooth
disable/enable/pair control, display/NeoPixel controls, interference-avoidance
control, Wi-Fi settings, configuration save/delete, firmware-update indicator,
firmware hash, and ROM/EEPROM read/write/wipe requests. Serial/TCP RNode
streams also expose a transport-local management dispatch handle that writes
pre-encoded command frames through the live KISS runtime; radio-state query and
blink dispatch are covered by local duplex tests. Feature-gated BLE RNode
streams expose the same management queue through the Nordic UART write path,
including BLE write-length chunking. The daemon exposes those initial safe
serial/TCP/BLE operations through `rnode_management` RPC and the
`rnodeconf-rs query-radio-state` / `rnodeconf-rs blink` commands. The same path
also queues read/display/local-radio safe controls: config read, ROM read,
display intensity/blanking/rotation/recondition/address, NeoPixel intensity,
and interference-avoidance enable/disable. Daemon RPC and `rnodeconf-rs`
also expose guarded
persistent or destructive management commands for Bluetooth control, config
save/delete, ROM write/wipe, hard reset, firmware update/hash metadata, and
Wi-Fi mode/channel/IP/netmask/SSID/PSK set or clear. Persistent commands
require `confirm_persistent = true`; destructive commands require
`confirm_destructive = true` plus `confirm_command` matching the canonical
command. Prepared-host RNode runs now exercise safe `query-radio-state` and
`blink` management dispatch against the selected serial, TCP/Wi-Fi, or BLE
bearer and record post-dispatch status; broader BLE management hardware
coverage still requires device-matrix evidence.

## Validation Rules

- `region` required when native `lora` is enabled. `RNodeInterface` may omit it
  and will use `US915` as the compatibility seed.
- Supported regions: `EU868`, `US915`, `AU915`, `AS923`, `IN865`, `KR920`, `RU864`.
- `state_path` required and non-empty when native `lora` is enabled.
  `RNodeInterface` may omit it; in that case the daemon skips the LoRa
  duty-cycle state-file preflight.
- `device` and `baud_rate` are optional as a pair for serial RNodes. Without an
  active port, startup only validates and persists LoRa compliance state.
- For `RNodeInterface` compatibility, a serial `port` without `baud_rate`
  inherits Python's `115200` default.
- For `RNodeInterface` compatibility, `frequency`, `bandwidth`,
  `spreadingfactor`, and `codingrate` are required. Native `lora` configs may
  still use region defaults.
- `port = "tcp://host:port"` is accepted for RNode Wi-Fi/TCP operation and does
  not require `baud_rate`.
- `port = "ble://..."` is accepted for `RNodeInterface` and does not require
  `baud_rate`. Startup requires the `reticulumd` `rnode-ble` feature; without
  it, the interface remains in failed startup status with an explicit feature
  error instead of opening as a serial path.
- Generic RNode BLE profile constants live in `rns-transport::iface::rnode_ble`;
  the same module also contains the raw-KISS session state, notification event
  model for packet payloads plus command responses, and feature-gated native
  BLE backend plus daemon startup wiring for `RNodeInterface`.
- `port` is accepted as a Reticulum-style alias for `device` on `lora` and
  `RNodeInterface`.
- `frequency_hz` must be in the Python RNode range
  `137000000..=3000000000`.
- `spreading_factor` allowed range: `5..=12`.
- `spreadingfactor` is accepted as a Reticulum-style alias for
  `spreading_factor`.
- `coding_rate` allowed: `4/5`, `4/6`, `4/7`, `4/8`.
- `codingrate` is accepted as a Reticulum-style alias for `coding_rate`; values
  `5`, `6`, `7`, and `8` are accepted as shorthand for the corresponding `4/n`
  coding rates.
- `bandwidth_hz` must be in the Python RNode range `7800..=1625000`.
- `frequency`, `bandwidth`, and `txpower` are accepted as aliases for
  `frequency_hz`, `bandwidth_hz`, and `tx_power_dbm`.
- `tx_power_dbm` must be in the Python RNode range `0..=37`.
- `flow_control` is accepted as a Reticulum-style RNode boolean. It defaults to
  `false`, matching the Python RNode default, and enables READY-based KISS
  packet flow control only when explicitly set.
- `id_callsign` and `id_interval` are accepted for Reticulum-style RNode
  station identification. The callsign is emitted as a raw KISS data frame after
  a real outbound packet and the configured interval have elapsed.
- `airtime_limit_short` and `airtime_limit_long` are optional percentages in
  the Python RNode range `0..=100`.
- `max_payload_bytes` allowed range: `1..=255` for native `lora` and `1..=508`
  for `RNodeInterface`. Omitted `RNodeInterface` payload size defaults to `508`,
  matching Python's RNode hardware MTU.
- `outgoing` defaults to `true`. Set `outgoing = false` to keep the interface
  available for inbound packets while suppressing daemon-initiated outbound
  broadcast and direct transmissions on that interface.
- `bitrate` and `announce_cap` are accepted as Reticulum-style per-interface
  announce pacing controls. `bitrate` is bits per second; `announce_cap` is a
  percentage in the range `1..=100`. Unspecified fields keep the runtime
  defaults.
- `command_timeout_ms` is accepted as the Reticulum-style RNode startup
  response deadline. It defaults to `1500 ms` and must be greater than zero.
  For `ble://` RNode ports this deadline remains separate from the BLE
  connection timeout: native BLE connect defaults to five seconds and can be
  tuned with `ble_connect_timeout_ms`, while RNode command-response validation
  defaults to 1500 ms unless `command_timeout_ms` is set.

## Active Device Behavior

When a serial RNode (`device` plus `baud_rate`), Wi-Fi/TCP RNode (`tcp://`
port), or feature-gated BLE RNode (`ble://` port) is active, startup writes
RNode-style KISS startup probe frames for device
detection, firmware version, platform, and MCU metadata before radio
configuration. It then writes configuration frames for frequency, bandwidth, TX
power, spreading factor, coding rate, optional short-term and long-term airtime
locks, and radio-on state. Airtime locks are encoded as Python-compatible
hundredths of a percent. Packet I/O then uses KISS data frames with
`max_payload_bytes` as the interface MTU. Outbound packets whose serialized
Reticulum wire length exceeds that MTU are rejected before they are queued to
the interface, with diagnostics that include packet context, size, and budget.
For BLE RNodes this rejection does not mark the BLE session disconnected; it
means the packet was unsuitable for the configured LoRa/RNode payload budget.
READY flow control is disabled by default for RNode parity and is enabled only
when `flow_control = true` is configured. Like Python RNode startup, a
flow-controlled stream is considered
ready after startup frames are flushed: the first outbound packet is sent
immediately, then later packets wait for device `CMD_READY` frames. If an RNode
misses `CMD_READY`, the stream unlocks flow control after the Python-compatible
five-second timeout and sends the next queued packet. The same shared KISS
decoder mirrors Python's lenient read loop for inbound escape handling: unknown
bytes after `FESC` are retained literally, while a trailing `FESC` at frame end
is dropped. Inbound payloads larger than `max_payload_bytes` are capped to that
MTU and still delivered, matching Python's RNode `HW_MTU` retention behavior.
Stale partial inbound frames are discarded after Python's RNode read timeout
before later bytes are decoded. When `id_callsign` and `id_interval` are
configured, the same KISS stream emits the
station ID as raw KISS data after the interval has elapsed since the first real
packet transmission.

For TCP/Wi-Fi RNode connections, the KISS stream also mirrors Python's TCP
activity keepalive: after 3.5 seconds without a successful write, it sends the
RNode detect command (`CMD_DETECT` with `DETECT_REQ`) as a raw KISS command
frame. Serial RNodes do not use this TCP activity probe.

The probe sequence is emitted for Python/RNode wire compatibility. Hardware
response frames for detect, firmware version, platform, and MCU metadata have a
typed parser in `rns-transport`, and the active LoRa stream records those
responses into interface probe status when the device sends them. The transport
layer can also validate a completed startup probe: detect must confirm an RNode
device, firmware must be at least Python's required `1.52`, and platform plus
MCU metadata must be present. The transport layer also records reported radio
parameters for frequency, bandwidth, TX power, spreading factor, coding rate,
radio state, and radio-lock state, and validates the Python
`validateRadioState` subset: frequency within 100 Hz when reported, exact
bandwidth, TX power, spreading factor, coding rate, and radio-on state. When
reported bandwidth, spreading factor, and coding rate are present, it also exposes
Python's on-air bitrate calculation. The protocol surface exposes Python's
RNode radio-state ask value and the matching KISS query frame. RNode runtime
stat responses for RX/TX counters, RSSI, SNR, SNR-derived quality, reported
airtime locks, airtime use, channel load, current RSSI, noise floor,
interference, PHY parameters, CSMA contention-window parameters, battery state,
battery percentage, temperature, and random byte are decoded with
Python-compatible scaling and RSSI offset. Battery state values also expose the
Python-compatible status strings `charged`, `charging`, `discharging`, and
`unknown`. Retained telemetry defaults match Python's RNode constructor for
initial airtime/channel-load counters, battery state/percentage, and empty
display buffers before the first hardware response arrives. Like Python
`process_incoming`, inbound RNode KISS data frames clear the retained
per-packet RSSI and SNR fields after delivery. The probe status identifies
display-capable ESP32 and NRF52 platforms like Python and exposes the matching
external-framebuffer
enable/disable, framebuffer-read, display-read, and framebuffer-write KISS
command frames only for those display platforms.
Framebuffer image data is split into Python-compatible 8-byte lines with a
one-byte line number. Framebuffer and display-read command responses are
retained with Python's expected 512-byte and 1024-byte payload sizes. The
same protocol helper also exposes Python's hard-reset KISS command frame
(`CMD_RESET` with payload `0xf8`), blink indication frame, Bluetooth
disable/enable/pair frames, display/NeoPixel control frames,
interference-avoidance control frame, Wi-Fi configuration frames,
configuration save/delete frames, firmware-update indicator and hash frames,
and ROM/EEPROM read/write/wipe request frames. The interface records online
state from reported RNode radio-state responses. RNode reset responses are also
classified at the protocol layer: an online ESP32 reset is surfaced as
Python's fatal `ESP32 reset` condition, while offline or non-ESP32 reset
responses are accepted as informational. RNode hardware error command responses
are classified with Python-compatible fatality: memory-low and modem-timeout
errors are retained as non-fatal hardware errors, while radio-initialisation,
TX failure, and unknown hardware errors reject the command response. Fatal
command-response errors, including online ESP32 reset and fatal hardware errors,
are retained as the interface's last command error for runtime visibility.
The transport layer exposes a combined startup-response validator that folds
retained fatal command errors, startup probe validation, and reported radio
configuration validation into one enforceable result. During active packet I/O,
active streams invoke that validator after the RNode startup-response deadline.
Startup response state is connection-scoped: a new opened stream clears stale
probe/radio status, non-fatal hardware errors, retained fatal command errors,
and reported online state before collecting fresh RNode responses. Validation
failures and retained fatal command-response errors tear down the current LoRa
KISS stream so the existing reconnect loop can reopen the device; malformed
non-fatal command responses are logged without cancelling the stream. The
daemon bootstrap status still does not synchronously fail on RNode
startup-response validation because enforcement happens in the opened stream
after startup frames are sent. Configure `command_timeout_ms` to tune that
deadline for slow RNode serial or TCP bridges.

When an active LoRa/RNode KISS stream is cancelled by daemon shutdown or
interface teardown, the stream sends Python-style detach commands before
closing: radio state off followed by the RNode leave-host command. Plain KISS
serial and TCP interfaces do not send these RNode-specific shutdown commands.

For active serial or TCP RNode ports, daemon/RPC status now refreshes the live
transport-side RNode status under `_runtime.lora.rnode_status`. `rnstatus-rs`
renders a compact human summary with bearer, online/detected state, firmware,
radio configuration, counters, battery percentage, hardware-error count, and
last command error when present. The JSON status includes:

- `endpoint`, `bearer`, and serial `baud_rate` when applicable.
- `configured.frequency_hz`, `bandwidth_hz`, `spreading_factor`,
  `coding_rate`, `tx_power_dbm`, and `max_payload_bytes`.
- `probe_status.detected`, firmware version, platform, MCU, and display
  capability.
- `radio_status` fields for reported radio configuration, radio state,
  RX/TX counters, signal telemetry, airtime/channel load, PHY/CSMA telemetry,
  battery, temperature, framebuffer/display payload lengths, random byte, and
  reported bitrate.
- `hardware_errors`, `last_command_error`, `online`, `flow_control`, and
  `reported_bitrate_bps`.

This status surface is the evidence foundation for prepared-host serial, TCP,
and feature-gated BLE RNode lifecycle smoke tests.

## RNode BLE Software Smoke

The software-only RNode BLE smoke keeps the host-side `#385` behavior
reproducible without requiring a prepared BLE RNode:

```sh
./tools/scripts/rnode-ble-software-smoke.sh
```

The script runs the feature-gated RNode BLE regression target, the
feature-gated `reticulumd` native RNode BLE management bridge regression, the
`rnodeconf-rs` extended management CLI matrix, and the shared transport
closed-queue cleanup regression, then writes artifacts under
`target/rnode-ble-software-smoke/`. A passing report records
`evidence_scope = "software_rnode_ble_fallback_management"` and covers:

- Python Nordic UART profile defaults
- configured RNode BLE identifier and alias matching
- configured Android peripheral exclusion during fallback scan
- RNode BLE command-monitor startup, degraded fallback, and runtime status JSON
- RNode BLE packet MTU rejection, successful MTU-sized transmit, shutdown, and
  management-frame chunking
- RNode BLE management handle queueing
- reticulumd daemon RnodeBle management bridge dispatch
- `rnodeconf-rs` extended management command-to-RPC matrix
- persistent and destructive RNode management CLI guard enforcement
- shared transport cleanup of closed TX queues

This is software regressions only. BLE hardware, firmware, radio, and management operation evidence still requires prepared-host devices.

## RNode Fake TCP Smoke

The fake TCP RNode smoke runs the ordinary prepared-host RNode path against a
local fake KISS TCP peer:

```sh
./tools/scripts/rnode-fake-tcp-smoke.sh
```

The wrapper starts a fake RNode TCP endpoint, runs
`./tools/scripts/rnode-prepared-host-smoke.sh` with `RNODE_PORT=tcp://...`,
and writes artifacts under `target/rnode-fake-tcp-smoke/`. A passing report
records `evidence_scope = "software_fake_tcp_rnode_prepared_host_management"`
and embeds the prepared-host `prepared_host_tcp_rnode` report. The fake KISS
TCP peer verifies startup probe and radio configuration frames, the
`rnodeconf-query-radio-state.json` and `rnodeconf-blink.json` command artifacts,
`management_commands`, `post_management_status`, `radio_query_seen`, and
`management_blink_seen`.

This is not prepared hardware. It proves the live daemon, `rnstatus-rs`,
`rnodeconf-rs`, and TCP KISS management dispatch path against deterministic
software, while broad serial/TCP/BLE hardware parity still requires prepared
devices.

## Prepared-Host Smoke

The opt-in prepared-host smoke validates `reticulumd` against a host with a
single serial, TCP/Wi-Fi, or feature-gated BLE RNode available as
`RNodeInterface`.

```sh
RNODE_PORT=/dev/ttyACM0 \
RNODE_FREQUENCY=915000000 \
RNODE_BANDWIDTH=125000 \
RNODE_SPREADING_FACTOR=9 \
RNODE_CODING_RATE=5 \
RNODE_TX_POWER=17 \
RNODE_COMMAND_TIMEOUT_MS=1500 \
./tools/scripts/rnode-prepared-host-smoke.sh
```

For Wi-Fi/TCP RNodes, use `RNODE_PORT=tcp://192.0.2.10:8001`; the generated
config omits serial baud rate and the pass gate expects
`_runtime.lora.rnode_status.endpoint` to match `host:port`.

For BLE RNodes, use `RNODE_PORT=ble://RNode 1234`. The script builds
`reticulumd` with `--features rnode-ble`, omits serial baud rate, and expects
`_runtime.lora.rnode_status.bearer = "ble"` plus the full `ble://...`
endpoint. `RNODE_BLE_ADAPTER`, `RNODE_BLE_SCAN_TIMEOUT_MS`,
`RNODE_BLE_CONNECT_TIMEOUT_MS`, and `RNODE_BLE_MAX_WRITE_LEN` are optional.

The script builds `reticulumd`, `rnstatus-rs`, and `rnodeconf-rs`, starts the
daemon with `--strict-interface-startup`, polls `rnstatus-rs --json`, dispatches
safe `rnodeconf-rs` management commands through the live daemon binding, and
writes artifacts under `target/rnode-hil/`. A passing run requires:

- `_runtime.startup_status = "spawned"`
- `_runtime.iface` populated with the runtime interface hash
- `_runtime.lora.rnode_status.probe_status.detected = true`
- `_runtime.lora.rnode_status.probe_status.firmware_version` populated
- `_runtime.lora.rnode_status.probe_status.platform` and `mcu` populated
- `_runtime.lora.rnode_status.radio_status.frequency_hz` is within 100 Hz of
  the configured value
- `_runtime.lora.rnode_status.radio_status.bandwidth_hz`, `spreading_factor`,
  `coding_rate`, and `tx_power_dbm` match the configured values
- `_runtime.lora.rnode_status.radio_status.radio_state = 1`
- `_runtime.lora.rnode_status.online = true`
- `_runtime.lora.rnode_status.last_command_error = null`
- `_runtime.lora.rnode_status.hardware_errors` is empty
- `rnodeconf-rs query-radio-state --interface rnode-prepared-host` returns
  `queued = true` and `command = "radio_state_query"`
- `rnodeconf-rs blink --interface rnode-prepared-host --pattern` with the
  configured pattern returns
  `queued = true`, `command = "blink"`, and the configured pattern
- post-management `rnstatus-rs --json` still reports the RNode online, radio
  on, no command error, and no hardware errors

Reports are written to `report.json` and include the latest endpoint, bearer,
probe status, radio status, reported bitrate, hardware errors, and command
error fields. The report also records `rnodeconf-query-radio-state.json`,
`rnodeconf-blink.json`, `rnstatus-post-management.json`,
`management_commands`, and `post_management_status`. For archived HIL evidence,
the report also records `report_schema = "rnode_prepared_host_smoke.v1"`,
`captured_at_utc`, `captured_by_host`, and the producing script path. The report
also records a bearer-specific `evidence_scope`:
`prepared_host_serial_rnode`, `prepared_host_tcp_rnode`, or
`prepared_host_ble_rnode`. A passing run proves one prepared endpoint for that
bearer; the `product_boundary` note records that broader hardware parity still
requires evidence across serial, TCP/Wi-Fi, BLE, device, firmware, and radio
combinations.

Nightly HIL exposes the same smoke through the `rnode-prepared` profile with
`HIL_RNODE_PORT`, optional `HIL_RNODE_BAUD_RATE`, optional `HIL_RNODE_REGION`,
optional `HIL_RNODE_FREQUENCY`, optional `HIL_RNODE_BANDWIDTH`, optional
`HIL_RNODE_SPREADING_FACTOR`, optional `HIL_RNODE_CODING_RATE`, optional
`HIL_RNODE_TX_POWER`, optional `HIL_RNODE_BITRATE`, optional
`HIL_RNODE_COMMAND_TIMEOUT_MS`, optional `HIL_RNODE_MAX_PAYLOAD_BYTES`, optional
`HIL_RNODE_BLE_ADAPTER`, optional
`HIL_RNODE_BLE_SCAN_TIMEOUT_MS`, optional `HIL_RNODE_BLE_CONNECT_TIMEOUT_MS`,
optional `HIL_RNODE_BLE_MAX_WRITE_LEN`, optional
`HIL_RNODE_MANAGEMENT_TIMEOUT_SECS`, optional `HIL_RNODE_BLINK_PATTERN`, and
optional `HIL_RNODE_TIMEOUT_SECS`.
Artifacts are uploaded as
`rnode-prepared-host-artifacts`, including
`target/rnode-hil/report.json` and `target/rnode-hil/run.*`.

## Reticulum Interface Parity Audit

```sh
./tools/scripts/reticulum-interface-parity-audit.sh
```

The audit reads the LocalInterface #384 evidence and the RNode BLE #385
evidence, then writes `target/reticulum-interface-parity-audit/report.json`
with `evidence_scope = "reticulum_interfaces_384_385_parity_audit"`. For the
RNode side it requires the software BLE fallback/management smoke, the fake TCP
prepared-host path smoke, and serial, TCP/Wi-Fi, and BLE prepared-host RNode
hardware reports. Hardware reports must use `prepared_host_serial_rnode`,
`prepared_host_tcp_rnode`, and `prepared_host_ble_rnode`, include queued
`management_commands`, and include `post_management_status` that remains
online, radio-on, command-error free, and hardware-error free. The strict audit
also requires each report to identify the hardware path with `endpoint`,
`transport_kind`, `bearer`, `detected`, `firmware_version.label`, `platform`,
and `mcu`, plus provenance fields `report_schema`, `captured_at_utc`,
`captured_by_host`, and `script`.

By default the audit writes a machine-readable summary even when full parity is
still incomplete. Use `./tools/scripts/reticulum-interface-parity-audit.sh --require-full`
for a strict gate; it exits non-zero while `missing_full_parity` lists missing
or failing hardware evidence. If prepared-host reports are archived outside
`target/rnode-hil/`, pass a colon-separated `RNODE_HIL_REPORTS` list.

To collect the missing serial/TCP/BLE hardware reports in one pass, run:

```sh
RIF_RNODE_SERIAL_PORT=/dev/ttyACM0 \
RIF_RNODE_TCP_PORT=tcp://192.0.2.10:8001 \
RIF_RNODE_BLE_PORT='ble://RNode 1234' \
./tools/scripts/reticulum-interface-hil-matrix.sh
```

The matrix runner executes `rnode-prepared-host-smoke.sh` once per bearer,
writes `target/reticulum-interface-hil-matrix/report.json` with
`evidence_scope = "reticulum_interfaces_384_385_hil_matrix"`, stores the
bearer reports at `target/rnode-hil/matrix/serial.report.json`,
`target/rnode-hil/matrix/tcp.report.json`, and
`target/rnode-hil/matrix/ble.report.json`, then invokes the strict parity audit.
It also writes `target/reticulum-interface-hil-matrix/artifact-manifest.json`
with `schema = "reticulum_interface_hil_matrix_artifacts.v1"` and SHA-256
digests for the matrix report, strict audit report, and each bearer report.
To re-run the strict audit against archived HIL artifacts with digest
verification, set
`RNODE_HIL_ARTIFACT_MANIFEST=target/reticulum-interface-hil-matrix/artifact-manifest.json`
or the `RIF_HIL_ARTIFACT_MANIFEST` / `RIF_ARTIFACT_MANIFEST` aliases; the audit
verifies the `rnode_serial_report`, `rnode_tcp_report`, and `rnode_ble_report`
SHA-256 digests before using those reports.
It accepts legacy aliases `RNODE_SERIAL_PORT`, `RNODE_TCP_PORT`, and
`RNODE_BLE_PORT`. Strict matrix runs refresh the LocalInterface #384 smokes by
default before the final `--require-full` audit, so clean HIL runners collect
the full evidence set in one pass. Use `--allow-partial` for diagnostics when
only some devices are attached and `--audit-existing` to re-check archived
reports without touching devices or refreshing smokes; partial mode skips local
smokes unless `--run-local-smokes` or `RIF_RUN_LOCAL_SMOKES=true` is set.

The matrix inherits the ordinary `RNODE_*` radio, timeout, management, and BLE
settings for every bearer, but each bearer can override them with
`RIF_RNODE_SERIAL_*`, `RIF_RNODE_TCP_*`, or `RIF_RNODE_BLE_*`. For example,
`RIF_RNODE_SERIAL_FREQUENCY`, `RIF_RNODE_TCP_FREQUENCY`,
`RIF_RNODE_BLE_FREQUENCY`, `RIF_RNODE_BLE_ADAPTER`,
`RIF_RNODE_BLE_MAX_WRITE_LEN`, and `RIF_RNODE_BLE_TIMEOUT_SECS` override only
the selected bearer while the others keep the shared `RNODE_*` default.

Nightly HIL exposes the same matrix through the `reticulum-interface-matrix`
profile.
Set `HIL_RNODE_MATRIX_SERIAL_PORT`, `HIL_RNODE_MATRIX_TCP_PORT`, and
`HIL_RNODE_MATRIX_BLE_PORT`; the job reuses the ordinary `HIL_RNODE_*` radio,
timeout, BLE adapter, and management variables documented above unless a
bearer-specific `HIL_RNODE_MATRIX_SERIAL_*`, `HIL_RNODE_MATRIX_TCP_*`, or
`HIL_RNODE_MATRIX_BLE_*` variable is set. Artifacts are uploaded as
`reticulum-interface-hil-matrix-artifacts`, including
`target/reticulum-interface-hil-matrix/report.json`,
`target/reticulum-interface-hil-matrix/parity-audit-report.json`,
`target/reticulum-interface-hil-matrix/artifact-manifest.json`, and
`target/rnode-hil/matrix/*.report.json`.

When `device` and `baud_rate` are absent, the interface remains
`validated_startup_only` and only the compliance state gate runs.

## State Persistence and Fail-Closed Policy

`state_path` stores duty-cycle debt and uncertainty markers.

Persistence guarantees:

1. State writes use `*.tmp` + rename.
2. Temporary file is `fsync`'d before rename.
3. Parent directory is `fsync`'d after rename.

Fail-closed conditions:

1. State payload unreadable/invalid JSON.
2. Unsupported state schema version.
3. State marked `uncertain`.
4. Startup clock rollback beyond uncertainty threshold relative to persisted timestamp.
5. Persisted `last_updated_unix_ms` is zero/invalid.
6. Persisted state timestamp is stale beyond compliance threshold.
7. Persisted `duty_cycle_debt_ms` exceeds compliance maximum.

When a fail-closed condition is hit, startup rejects interface activation and logs the reason.

Debt carryover normalization:

1. On startup, elapsed wall-clock time since the last persisted update is calculated.
2. Persisted `duty_cycle_debt_ms` is reduced by elapsed time (saturating at zero).
3. Updated debt/timestamp metadata is persisted atomically before startup returns.

Startup policy controls:

- Default mode is best-effort (daemon continues in degraded mode when some interfaces fail).
- `--strict-interface-startup` makes startup/preflight failures fatal.

## Operator Recovery

1. Confirm host clock integrity (NTP/system clock).
2. Inspect the persisted state file and reason.
3. If state is unrecoverable or uncertain, archive and replace/reset the state file.
4. Restart daemon and verify startup log reports `uncertain=false`.

## Health Signals

Expected startup log:

- `lora configured name=<name> region=<region> state_path=<path> duty_cycle_debt_ms=<n> debt_elapsed_ms=<m> uncertain=false`
- `lora compliance gate name=<name> debt_remaining_ms=<n> tx_allowed_after_additional_wait_ms=<n>` (emitted when debt remains)
- `lora enabled iface=<iface> name=<name> device=<device> baud_rate=<baud>` when an active device starts

Failure log:

- `lora startup rejected name=<name> err=<fail-closed reason>`
- `interface startup degraded started=<n> failed=<m> strict=<bool>`

Runtime status visibility:

- `list_interfaces` includes `_runtime.startup_status`.
- Active serial/TCP/BLE RNode interfaces include refreshed
  `_runtime.lora.rnode_status`.
- Failed interfaces include `_runtime.startup_error`.

## Verification Commands

```bash
cargo test -p reticulumd --test config
cargo test -p reticulumd --bin reticulumd bootstrap_best_effort_starts_active_lora_interface_without_transport_flag
cargo test -p reticulumd --bin reticulumd lora_state::tests
cargo check -p reticulumd --all-targets
```

## Rollback

- Disable `lora` interface entries and restart daemon.
- Keep state files for forensic review before deletion.
