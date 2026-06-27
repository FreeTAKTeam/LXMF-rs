# `reticulumd` Weave Interface Runbook

## Purpose

This runbook documents the in-progress `WeaveInterface` slice. It supports a
shared serial parent using WDCL packets over HDLC framing and virtual child
interfaces for discovered Weave endpoints. It is not yet a production-complete
Weave parity claim.

## Scope

- Reticulum type alias: `WeaveInterface`
- Physical transport: one serial device
- Default serial speed: `3000000`
- Default MTU: `1024`
- Runtime role: multicast parent with virtual unicast endpoint children
- Runtime metadata: `_runtime.weave.status`

## Configuration

```toml
interfaces = [
  {
    type = "WeaveInterface",
    enabled = true,
    name = "weave-main",
    port = "/dev/ttyACM0",
    configured_bitrate = 250000
  }
]
```

The parser accepts Python-style `port` and `speed` aliases. `configured_bitrate`
maps to the interface bitrate used by announce pacing.

## Runtime Behavior

Startup opens the serial port and sends a WDCL discovery broadcast framed with
HDLC. When a valid discovery response arrives, the runtime sends the WDCL
connect handshake. Endpoint-alive and endpoint-via events register virtual
unicast child interfaces. Endpoint alive, via, and packet activity refreshes
the child lifecycle timestamp; idle endpoint children are stopped and removed
from runtime status. Stream shutdown and software cancellation/stop mark the
runtime link state `closed`, clear the WDCL-connected flag, and clear any
remaining endpoint children.

Inbound WDCL endpoint packets are deserialized as Reticulum packets and
delivered to the matching virtual child. Direct outbound sends to a virtual
child write a WDCL endpoint packet command for that endpoint. Broadcast sends
fan out to known endpoint children.

The transport keeps an initial runtime status snapshot with the configured
device, baud rate, MTU, local/remote switch IDs, WDCL connection state,
endpoint counters, byte/frame counters, last WDCL log event, and per-log-event
counts. `reticulumd` seeds this under `_runtime.weave.status` during startup
and periodically refreshes it into the cached interface records returned by
`daemon_status_ex` and `list_interfaces`.
`rnstatus-rs` also renders this runtime state in human output, including link
state, endpoint count, WDCL connection state, byte counters, display progress,
CPU load, and memory usage when the daemon has reported those fields.
Incoming WDCL display frames addressed to the local switch update
`_runtime.weave.status.display` with the remote framebuffer color format, fixed
128x64 dimensions, total size, received size, completion flag, and a hex
framebuffer snapshot when a complete frame has arrived. Completion is based on
actual byte coverage, so out-of-order chunks do not report a complete
framebuffer until all byte ranges have arrived. Targeted CPU, task CPU, and
memory log events update `_runtime.weave.status.device_stats`; off-target
display and log frames are ignored.

## Prepared-Host Smoke

The opt-in prepared-host smoke validates the daemon against a host with a
connected Weave serial device. By default it requires the WDCL connection log
event, which proves that strict startup opened the serial device, the daemon
sent discovery, the device responded with a remote switch ID, and the Weave
runtime transitioned to connected state.

```sh
WEAVE_PORT=/dev/ttyACM0 \
WEAVE_BAUD_RATE=3000000 \
WEAVE_REQUIRE_CONNECTED=true \
./tools/scripts/weave-prepared-host-smoke.sh
```

The script builds `reticulumd` and `rnstatus-rs`, starts the daemon with
`--strict-interface-startup`, polls `rnstatus-rs --json`, and writes artifacts
under `target/weave-hil/`. A passing default run requires:

- `_runtime.startup_status = "spawned"`
- `_runtime.iface` populated with the runtime parent interface hash
- `_runtime.weave.status.link_state = "connected"`
- `_runtime.weave.status.wdcl_connected = true`
- `_runtime.weave.status.remote_switch_id` populated
- `_runtime.weave.status.last_error = null`
- non-zero `_runtime.weave.status.frames_tx` and `bytes_tx`

Set `WEAVE_REQUIRE_CONNECTED=false` only for bench bring-up where the desired
evidence is limited to serial open plus discovery transmission; full
prepared-host evidence should keep the default connected gate. Reports are
written to `report.json` and include the latest link state, WDCL connection
flag, switch IDs, endpoint counters, byte/frame counters, display status, and
device stats when the prepared host emits them.

Nightly HIL exposes the same smoke through `HIL_WEAVE_ENABLED=true` with
`HIL_WEAVE_PORT`, optional `HIL_WEAVE_BAUD_RATE`, optional `HIL_WEAVE_MTU`,
optional `HIL_WEAVE_CONFIGURED_BITRATE`, optional
`HIL_WEAVE_REQUIRE_CONNECTED`, and optional `HIL_WEAVE_TIMEOUT_SECS`.
Artifacts are uploaded as `weave-prepared-host-artifacts`, including
`target/weave-hil/report.json` and `target/weave-hil/run.*`.

## Known Gaps

- Broader prepared-host Weave hardware evidence across devices and firmware
  combinations is still required.
- Remote display/status UI integration is not complete.
- I2PInterface has a separate in-progress outbound SAM peer slice.
