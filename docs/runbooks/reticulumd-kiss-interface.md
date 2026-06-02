# `reticulumd` KISS Interface Runbook

## Purpose

This runbook documents active KISS interface startup and the modem
configuration fields exposed by `reticulumd`. KISS is the framing layer; the
connection underneath can be serial, Bluetooth, Wi-Fi/TCP, or another byte
stream transport.

## Scope

- Interface kinds: `kiss`, `kiss_tcp_client`
- Reticulum type alias: `KISSInterface`
- Active transports: serial KISS modem, TCP-connected KISS modem or bridge
- Runtime mutation policy: `set_interfaces`/`reload_config` with `kiss`
  or `kiss_tcp_client` changes require restart

## Required Config Fields

Serial KISS:

```toml
interfaces = [
  {
    type = "kiss",
    enabled = true,
    name = "kiss-main",
    device = "/dev/ttyACM0",
    baud_rate = 9600,
    preamble_ms = 350,
    tx_tail_ms = 20,
    persistence = 64,
    slot_time_ms = 20,
    kiss_flow_control = true
  }
]
```

Reticulum-style serial KISS configuration keys are also accepted for migration
from Python configs. `type = "KISSInterface"` normalizes to `kiss`, `port`
maps to `device`, `speed` maps to `baud_rate`, and `databits`, `stopbits`,
`preamble`, `txtail`, `slottime`, and boolean `flow_control` map to the
corresponding Rust fields. When `speed` is omitted on the Python
`KISSInterface` alias, `baud_rate` defaults to Python's `9600` baud:

```toml
interfaces = [
  {
    type = "KISSInterface",
    enabled = true,
    name = "kiss-main",
    port = "/dev/ttyACM0",
    speed = 9600,
    databits = 8,
    parity = "N",
    stopbits = 1,
    preamble = 350,
    txtail = 20,
    persistence = 64,
    slottime = 20,
    flow_control = true,
    outgoing = false,
    bitrate = 1200,
    announce_cap = 5,
    id_callsign = "MYCALL-0",
    id_interval = 600
  }
]
```

TCP KISS, for example a KISS modem or bridge reachable over Wi-Fi:

```toml
interfaces = [
  {
    type = "kiss_tcp_client",
    enabled = true,
    name = "kiss-wifi",
    host = "192.0.2.10",
    port = 8001,
    preamble_ms = 350,
    tx_tail_ms = 20,
    persistence = 64,
    slot_time_ms = 20,
    kiss_flow_control = true
  }
]
```

Python `TCPClientInterface` entries with `kiss_framing = true` are normalized
to `kiss_tcp_client`; `target_host` and `target_port` map to `host` and `port`,
and `fixed_mtu` maps to `mtu`:

```toml
interfaces = [
  {
    type = "TCPClientInterface",
    enabled = true,
    name = "python-kiss-tcp",
    target_host = "192.0.2.10",
    target_port = 8001,
    kiss_framing = true,
    fixed_mtu = 512
  }
]
```

## Validation Rules

- For `kiss`, `device` is required when enabled.
- For `kiss`, `baud_rate` is required and must be greater than zero when
  enabled.
- For `KISSInterface` compatibility, `port` is accepted as `device`, `speed`
  as `baud_rate`, `databits` as `data_bits`, `stopbits` as `stop_bits`,
  `preamble` as `preamble_ms`, `txtail` as `tx_tail_ms`, `slottime` as
  `slot_time_ms`, and boolean `flow_control` as `kiss_flow_control`. If
  `speed` is omitted, the Python alias uses the Python default `9600`.
- When `kiss_flow_control` or compatibility `flow_control` is enabled, the
  stream starts ready after the KISS startup frames are flushed, matching
  Python `KISSInterface` configuration. The first outbound packet is sent
  immediately, then subsequent packets wait for device `CMD_READY` frames.
  If a modem misses `CMD_READY`, the stream unlocks flow control after the
  Python-compatible five-second timeout and sends the next queued packet.
- Startup always writes the Python KISS `CMD_READY 0x01` setup command,
  independent of whether runtime READY flow-control gating is enabled.
- Inbound KISS escape decoding mirrors Python's lenient read loop: `FESC TFEND`
  and `FESC TFESC` are translated, an unknown byte after `FESC` is retained
  literally, and a trailing `FESC` before frame end is dropped.
- Oversized inbound KISS payloads are capped to the configured MTU and still
  delivered, matching Python's `HW_MTU` buffer-retention behavior.
- Stale partial inbound frames are discarded after Python's KISS serial read
  timeout before later bytes are decoded.
- Serial KISS parity accepts Reticulum shorthand (`N`, `E`, `O`) and long-form
  names (`none`, `even`, `odd`).
- For `kiss_tcp_client`, `host` and `port` are required when enabled.
- For `kiss_tcp_client`, `port` must be greater than zero.
- Python `TCPClientInterface` with `kiss_framing = true` selects
  `kiss_tcp_client`; without KISS framing it remains a normal `tcp_client`.
- `mtu` defaults to 564 and must be between 64 and 65535 if provided.
- `reconnect_backoff_ms` defaults to 500 and must be at least 50 if provided.
- `max_reconnect_backoff_ms` defaults to at least 5000 and must be greater
  than or equal to `reconnect_backoff_ms`.
- `id_callsign` and `id_interval` are accepted for Reticulum-style station
  identification. If `id_interval` is configured without `id_callsign`, Python
  `KISSInterface` parity emits an empty station-ID payload padded to 15 bytes.
  If `id_callsign` is provided, it must be non-empty and at most 32 bytes.
  `id_interval` is seconds and must be greater than zero.
- `outgoing` defaults to `true`. Set `outgoing = false` to keep the interface
  available for inbound traffic while suppressing daemon-initiated outbound
  broadcast and direct transmissions on that interface.
- `bitrate` and `announce_cap` are accepted as Reticulum-style per-interface
  announce pacing controls. `bitrate` is bits per second; `announce_cap` is a
  percentage in the range `1..=100`. Unspecified fields keep the runtime
  defaults.

## Active Device Behavior

Startup writes KISS modem commands for preamble, TX tail, persistence, slot
time, and Python's READY setup command. Packet I/O uses KISS data frames;
inbound READY commands release one queued outbound frame when flow control is enabled.
The same KISS codec and startup commands are used by the serial and TCP
bearers. For Python `KISSInterface` parity, serial and TCP KISS strip the high
port nibble from inbound KISS command bytes and treat the low nibble as the
single supported port command. RNode/LoRa keeps full command bytes because
Python `RNodeInterface` uses full values such as firmware command `0x50`.
If a peer starts a frame and then goes quiet beyond the Python KISS read
timeout, the partial frame is dropped before later bytes are decoded.

When `id_interval` is configured, the KISS stream emits the callsign as a KISS
data frame after a real outbound packet and the configured interval have
elapsed. For Python `KISSInterface` parity, a missing callsign is treated as an
empty payload and callsigns shorter than 15 bytes are zero-padded before
transmission. RNode station identification uses the same scheduling path but
requires a callsign and does not apply this serial KISS padding.

## Health Signals

Expected startup log:

- `kiss enabled iface=<iface> name=<name> device=<device> baud_rate=<baud>`
- `kiss_tcp_client enabled iface=<iface> name=<name> endpoint=<host>:<port>`

Runtime status visibility:

- `list_interfaces` includes `_runtime.startup_status = "spawned"`.
- `list_interfaces` includes `_runtime.iface` with the active interface hash.

## Verification Commands

```bash
cargo test -p reticulum-rs-transport --test kiss_codec
cargo test -p reticulumd --test config kiss
cargo test -p reticulumd --test config kiss_tcp_client
cargo test -p reticulumd --bin reticulumd kiss_tcp_client_builder
cargo test -p reticulumd --bin reticulumd bootstrap_best_effort_starts_kiss_interface_without_transport_flag
cargo check -p reticulumd --all-targets
```

## Rollback

- Disable `kiss` interface entries and restart daemon.
- Confirm only the intended remaining interfaces are active with
  `list_interfaces`.
