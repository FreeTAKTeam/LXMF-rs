# `reticulumd` RNodeMulti Interface Runbook

## Purpose

This runbook documents the in-progress `RNodeMultiInterface` slice: one shared
serial or TCP RNode parent with nested virtual-port subinterfaces. It is not a
production-complete RNodeMulti parity claim.

## Scope

- Reticulum type alias: `RNodeMultiInterface`
- Default MTU: `508`, matching Python `RNodeMultiInterface.HW_MTU`
- Physical transport: one serial RNode device or `tcp://host:port` RNode
  endpoint opened at the parent level
- Child model: nested vport subinterfaces registered as virtual unicast
  children
- Multiplexing: KISS `CMD_SEL_INT` (`0x1f`) selects the active RNode virtual
  port before per-vport configuration or packet writes
- Startup probes: baseline validation now covers hardware detect, firmware
  version `>= 1.74`, platform, MCU, `CMD_INTERFACES` discovery, and configured
  vports reported by the device
- Runtime status: selected-vport command responses update per-vport radio
  status bookkeeping in the transport runtime; the same status payload reports
  stream/probe state and the last open/probe/init/read error
- Exported status: daemon startup metadata includes an initial `radio_status`
  snapshot schema under `settings._runtime.rnode_multi`, and live daemon/RPC
  snapshots refresh from the transport-side runtime handle
- Current posture: partial, with live daemon status refresh, prepared-host
  validation, and broader production parity still pending

## Configuration Model

The slice is organized as a parent interface with explicit child
subinterfaces. Each child owns a vport number, LoRa/RNode radio parameters, and
its own outgoing flag:

```toml
interfaces = [
  {
    type = "RNodeMultiInterface",
    enabled = true,
    name = "rnode-multi",
    port = "/dev/ttyACM0", # or "tcp://192.0.2.10:8001"
    speed = 115200,
    radio0 = {
      name = "rnode-multi-v0",
      vport = 0,
      region = "US915",
      frequency = 915000000,
      bandwidth = 125000,
      spreadingfactor = 9,
      codingrate = 5,
      txpower = 17,
      outgoing = true
    },
    radio1 = {
      name = "rnode-multi-v1",
      vport = 1,
      region = "US915",
      frequency = 917000000,
      bandwidth = 125000,
      spreadingfactor = 9,
      codingrate = 5,
      txpower = 17,
      outgoing = false
    }
  }
]
```

Keep `vport` assignments stable across restarts. The runtime maps each child
virtual interface address to its configured vport; changing child order or
vport values changes direct-routing behavior.

## Startup Behavior

Daemon startup registers the shared RNode parent and child virtual
interfaces immediately, then the transport task opens the configured parent
serial port/speed or TCP endpoint asynchronously. The runtime registers one
virtual child interface per configured subinterface, applies the child's
`outgoing` policy to that virtual interface, and builds an address-to-vport map
for direct sends.

Before applying child configuration, the transport task validates that the
attached device responds to the RNode probe sequence. The baseline checks
require a detected RNode, firmware version `>= 1.74`, platform and MCU
metadata, `CMD_INTERFACES` support, and hardware-reported configured vports
that cover the requested child vports. Open/probe/init/read failures are
reflected in live `radio_status.stream_state` and `radio_status.last_error`
metadata even though the daemon startup record remains `spawned`.

For each child, startup writes:

1. KISS `CMD_SEL_INT` with the child vport.
2. The child LoRa/RNode configuration command frames.
3. Radio-state-on for that selected vport.

This gives each virtual port its own radio configuration while sharing the
same parent KISS stream.

## Packet Routing

Inbound packet handling is vport-aware:

- Plain KISS data frames are treated as vport `0`.
- Vport-specific KISS data command frames are mapped back to the child vport
  they represent.
- Packets from known vports are delivered to the matching virtual child
  interface.
- Packets from unmapped vports are ignored.

Outbound packet handling follows the Reticulum interface role:

- Direct sends to a virtual child select that child's vport with
  `CMD_SEL_INT`, then write the packet as KISS data.
- Broadcast sends fan out to every configured child whose `outgoing` flag is
  `true`.
- Broadcast sends do not fan out to receive-only children.

## Status Routing

RNodeMulti status handling tracks the currently selected virtual port from
`CMD_SEL_INT` responses. Subsequent radio command/status responses are applied
to that selected child status record, matching Python's selected-index model.
Packet data command frames continue to route by their explicit vport command
byte and do not change the selected status vport.

The daemon startup record exposes the configured RNodeMulti status schema under
`settings._runtime.rnode_multi.radio_status`. This gives RPC consumers stable
keys for `selected_vport`, known `vports`, and per-vport radio fields before
hardware has reported live values. While the daemon is running,
`daemon_status_ex` and interface listing snapshots refresh that
`radio_status` object from the transport-side runtime handle. The refreshed
object includes `stream_state` and `last_error`, so absent hardware or a failed
probe is visible to RPC consumers instead of appearing as successful telemetry.
`rnstatus-rs` human output summarizes the same runtime state with the stream
state, selected vport, vport count, and last error so operators can see failed
open/probe/read states without switching to JSON output.

## Shutdown Behavior

Shutdown iterates each child vport and writes:

1. KISS `CMD_SEL_INT` with the child vport.
2. Radio-state-off for that selected vport.
3. RNode leave-host for that selected vport.

The shutdown sequence is best-effort; the stream is flushed before the shared
serial session closes. When the parent RNodeMulti interface stops, configured
virtual vport children are also stopped and removed from the interface manager
so stale child routes do not remain after parent shutdown.

## Known Gaps

- `I2PInterface` has a separate in-progress SAM peer/connectable slice;
  prepared-host production evidence is not complete.
- `WeaveInterface` has a separate in-progress WDCL/HDLC endpoint slice; full
  display/stat and hardware parity is not complete.
- Selected-vport radio command/status bookkeeping, an initial exported
  `radio_status` schema, stream/probe failure state, and live daemon snapshot
  refresh exist, but prepared-host telemetry validation is not yet at Python
  parity.
- The startup probe baseline has validation for detect, firmware `>= 1.74`,
  platform, MCU, `CMD_INTERFACES`, and configured hardware vports, but full
  prepared-host hardware evidence across devices and firmware combinations is
  still required.
- Broader RNodeMulti production parity remains incomplete; release notes should
  not describe this family as production-complete yet.

## Verification Focus

Useful coverage for this slice should prove:

- Startup writes `CMD_SEL_INT` before each child configuration.
- Startup rejects devices that fail detect, firmware `>= 1.74`, platform, MCU,
  `CMD_INTERFACES`, or configured-vport validation.
- Direct outbound packets select exactly the target child's vport.
- Inbound vport data is delivered to the matching virtual child interface.
- Broadcast packets are written once per outgoing child and skipped for
  receive-only children.
- `CMD_SEL_INT` status responses select the child status record that subsequent
  radio command/status responses update.
- Startup metadata and live daemon/RPC snapshots expose
  `settings._runtime.rnode_multi.radio_status` with stable per-vport status
  keys.
- Shutdown sends radio-off and leave-host frames for each child vport.
