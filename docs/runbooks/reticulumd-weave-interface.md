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
from runtime status, and stream shutdown clears any remaining endpoint children.

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
framebuffer snapshot when a complete frame has arrived. Targeted CPU, task CPU,
and memory log events update `_runtime.weave.status.device_stats`; off-target
display and log frames are ignored.

## Known Gaps

- Prepared-host Weave hardware evidence is still required.
- Remote display/status UI integration is not complete.
- I2PInterface has a separate in-progress outbound SAM peer slice.
