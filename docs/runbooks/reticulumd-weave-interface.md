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
state, endpoint count, WDCL connection state, remote switch ID, byte and frame
counters, invalid-frame count, last log event, display completion and byte
progress, display color format, CPU load, memory usage, and task-stat counts
when the daemon has reported those fields.
Use `rnstatus-rs --weave-display <interface-name>` for a display-focused view
of the captured remote framebuffer metadata, complete framebuffer hex snapshot
when available, and current device-stat summary; add `--json` to return only
the Weave display/status subset for that interface.
Incoming WDCL display frames addressed to the local switch update
`_runtime.weave.status.display` with the remote framebuffer color format, fixed
128x64 dimensions, total size, received size, completion flag, and a hex
framebuffer snapshot when a complete frame has arrived. Completion is based on
actual byte coverage, so out-of-order chunks do not report a complete
framebuffer until all byte ranges have arrived. Targeted CPU, task CPU, and
memory log events update `_runtime.weave.status.device_stats`; off-target
display and log frames are ignored.
The transport also has the Python-compatible WDCL remote-display service
control frame primitive (`WDCL_CMD_REMOTE_DISPLAY`, payload `0x01` to enable
and `0x00` to disable) covered by software tests. `reticulumd` exposes live
dispatch through the `weave_remote_display_control` RPC bridge, resolving the
Weave interface by runtime interface hash or configured name. The
`weaveconf-rs` helper queues those controls against the active stream:

```sh
weaveconf-rs --rpc 127.0.0.1:4243 enable-remote-display --interface weave-main
weaveconf-rs --rpc 127.0.0.1:4243 disable-remote-display --interface weave-main
```

By default the daemon uses the remote switch ID learned during WDCL discovery.
For bench diagnostics before discovery has populated runtime state, pass an
explicit four-byte switch ID:

```sh
weaveconf-rs --rpc 127.0.0.1:4243 enable-remote-display \
  --interface weave-main \
  --remote-switch-id-hex 10203040
```

## Software Fake-PTY Smoke

The software fake-PTY smoke validates the daemon and operator tooling without
attached Weave hardware:

```sh
./tools/scripts/weave-fake-pty-smoke.sh
```

The script starts a local pseudo-terminal fake Weave peer, configures
`reticulumd` with `WeaveInterface` on the generated PTY slave, and runs
`--strict-interface-startup`. The fake peer decodes HDLC/WDCL frames, captures
the daemon discovery broadcast, returns a signed WDCL discovery response,
observes the daemon connect handshake, and emits connection, endpoint,
display-frame, CPU, task, and memory log/status frames. A passing run requires:

- `_runtime.startup_status = "spawned"`
- `_runtime.weave.status.link_state = "connected"`
- `_runtime.weave.status.wdcl_connected = true`
- `_runtime.weave.status.remote_switch_id` populated from the signed fake peer
- `_runtime.weave.status.local_endpoint_id` populated
- `_runtime.weave.status.endpoint_count = 1`
- `_runtime.weave.status.display.buffer_hex = "aabbccdd"`
- `_runtime.weave.status.device_stats.cpu_load = 37`
- human `rnstatus-rs` output summarizing the same Weave runtime metadata
- `rnstatus-rs --weave-display weave-fake-pty` reporting the captured display
  framebuffer and device stats
- `weaveconf-rs enable-remote-display --interface weave-fake-pty`
- `weaveconf-rs disable-remote-display --interface weave-fake-pty`
- the fake peer recording `remote_display_enable_seen` and
  `remote_display_disable_seen` from `WDCL_CMD_REMOTE_DISPLAY` command frames

The smoke writes structured evidence under `target/weave-fake-pty-smoke/`,
including `report.json`, fake-peer state with `device_stats_sent`,
`remote_display_enable_seen`, and `remote_display_disable_seen`, daemon logs,
`rnstatus-rs` JSON/human output, `rnstatus-rs --weave-display` JSON/human
output, and both `weaveconf-rs` command responses. This proves signed
discovery, WDCL connection status refresh, display/status rendering, and live
remote-display control dispatch through the real daemon path. The report
records `evidence_scope = "software_fake_pty_weave"` plus a
`product_boundary` note: not prepared-host or broad hardware production parity.
It is still not a
substitute for prepared-host execution against real Weave hardware.

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
written to `report.json` and include `evidence_scope`. A default connected
run records `evidence_scope = "prepared_host_connected_serial"`. A
`WEAVE_REQUIRE_CONNECTED=false` run records
`evidence_scope = "prepared_host_serial_discovery_only"` and should not be used
as connected-device parity evidence. Reports also include a `product_boundary`
note that broader production parity still requires evidence across devices,
firmware, display/status payloads, and operator workflows; latest link state,
WDCL connection flag, switch IDs, endpoint counters, byte/frame counters,
display status, and device stats when the prepared host emits them.

Set `WEAVE_REMOTE_DISPLAY_CONTROL=true` to additionally prove the live
`weaveconf-rs enable-remote-display` and `weaveconf-rs disable-remote-display`
RPC path against the connected device. This opt-in gate requires
`WEAVE_REQUIRE_CONNECTED=true`, queues both controls against
`weave-prepared-host`, refreshes `rnstatus-rs --json` afterwards, and records
`remote_display_control_requested` plus `remote_display_control_result` in
`report.json`.

Nightly HIL exposes the same smoke through the `weave` profile with
`HIL_WEAVE_PORT`, optional `HIL_WEAVE_BAUD_RATE`, optional `HIL_WEAVE_MTU`,
optional `HIL_WEAVE_CONFIGURED_BITRATE`, optional
`HIL_WEAVE_REQUIRE_CONNECTED`, optional `HIL_WEAVE_REMOTE_DISPLAY_CONTROL`, and
optional `HIL_WEAVE_TIMEOUT_SECS`.
Artifacts are uploaded as `weave-prepared-host-artifacts`, including
`target/weave-hil/report.json` and `target/weave-hil/run.*`.

## Known Gaps

- Broader prepared-host Weave hardware evidence across devices and firmware
  combinations is still required.
- Current operator visibility is through daemon/RPC status, `rnstatus-rs`
  summaries, `rnstatus-rs --weave-display`, and `weaveconf-rs` display-service
  controls; broader hardware evidence is still needed before a
  production-complete Weave claim.
- I2PInterface has a separate in-progress outbound SAM peer slice.
