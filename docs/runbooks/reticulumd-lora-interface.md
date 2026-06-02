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
`bandwidth`, `spreadingfactor`, and `codingrate` values instead of inheriting
Rust region defaults. The repo still requires `state_path` so duty-cycle
fail-closed state is explicit:

```toml
interfaces = [
  {
    type = "RNodeInterface",
    enabled = true,
    name = "rnode-main",
    region = "US915",
    state_path = "var/reticulumd/lora-state.json",
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
    region = "US915",
    state_path = "var/reticulumd/lora-state.json",
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
    region = "US915",
    state_path = "var/reticulumd/lora-state.json",
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
BLE write length before they reach the backend. The RNode BLE notification path
also preserves non-READY KISS command responses alongside decoded packet
payloads, and its command monitor exposes retained probe status, radio status,
non-fatal hardware errors, fatal command error, online state, and reported
bitrate. Daemon `RNodeInterface` `ble://` startup appends the same RNode
detect, firmware, platform, MCU, radio configuration, airtime-lock, and
radio-on command frames used by serial/TCP RNode startup, validates startup and
fatal command responses through the same RNode protocol state, and shutdown
writes radio-off plus leave-host frames before BLE cleanup. Broader RNode
management operations over BLE remain incomplete.

## Validation Rules

- `region` required when enabled.
- Supported regions: `EU868`, `US915`, `AU915`, `AS923`, `IN865`, `KR920`, `RU864`.
- `state_path` required and non-empty when enabled.
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
- `max_payload_bytes` allowed range: `1..=255`.
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
`max_payload_bytes` as the interface MTU. READY flow control is disabled by
default for RNode parity and is enabled only when `flow_control = true` is
configured. Like Python RNode startup, a flow-controlled stream is considered
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
(`CMD_RESET` with payload `0xf8`). The interface records
online state from reported RNode radio-state responses. RNode reset responses
are also classified at the protocol layer: an online ESP32 reset is surfaced as
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
