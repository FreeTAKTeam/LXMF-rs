# RNode Bluetooth SPP Interface

## Purpose

`rnode_spp` is the RNode Bluetooth Classic Serial Port Profile transport
scaffold. It carries opaque Reticulum packet bytes through the same RNode KISS
framing used by serial and TCP RNode paths, but the physical bearer is a
Bluetooth Classic/RFCOMM byte stream rather than BLE GATT characteristics.

```text
Reticulum packet bytes -> KISS data frame -> SPP byte write
SPP byte read -> KISS decoder -> Reticulum packet bytes
```

Use `rnode_spp` for RNode devices exposed as Bluetooth Classic serial ports.
Use `rnode_ble` for RNode BLE/Nordic UART devices and `ble_gatt` for generic
configured GATT adapters. The SPP path intentionally does not reuse BLE scan,
characteristic discovery, notification subscription, or ATT MTU behavior.

## Current Scope

This slice lives in `reticulum-rs-transport` and defines the backend-neutral
runtime contract for SPP-backed RNode KISS sessions:

- `RnodeSppBackend` owns platform-specific Bluetooth Classic connection,
  byte-stream writes, and byte-stream reads.
- `RnodeSppKissRuntime` applies RNode KISS startup, packet MTU checks,
  stream-frame decoding, KISS READY flow-control flushing, and shutdown frame
  writes on top of any backend that implements the trait.
- `RnodeSppSettings` records the target device identifier, optional display
  name, and lifecycle timeouts that an Android or desktop backend can consume.

No `reticulumd` configuration kind is exposed by this scaffold yet. Daemon
configuration should only be documented once a native backend and startup path
exist for the daemon. Until then, integrations should instantiate the
transport-side runtime directly or through the embedding platform layer.

## Bluetooth Boundary

The repository owns Reticulum packet handling, KISS framing, runtime state, and
the stream-oriented backend contract. It does not provision the host Bluetooth
environment. Pairing, bonding, RFCOMM socket creation, Android permissions,
desktop adapter setup, and platform trust prompts remain responsibilities of
the embedding app or host operator.

An SPP backend should connect to a prepared RNode Bluetooth serial endpoint and
present ordered byte reads and writes. The runtime treats the stream as opaque
bytes and has no BLE-specific assumptions.

## Runtime Contract

Startup follows this sequence:

1. Mark the runtime disconnected.
2. Await `backend.connect()` with `RnodeSppKissConfig::connect_timeout`.
3. Write KISS startup command frames from `KissConfig`.
4. Write any configured `initial_frames`.
5. Mark the runtime connected after startup writes succeed.

If `backend.connect()` stalls, startup returns
`RnodeSppKissError::ConnectTimeout` and no startup frames are written. If the
backend returns a connect or write error, startup returns
`RnodeSppKissError::Backend` with the failing operation label.

Outbound packets are rejected with `PacketTooLarge` before any write when the
payload length exceeds the configured KISS MTU. Otherwise the runtime encodes
the packet as a KISS data frame and writes it to the SPP stream.

Inbound bytes are pushed into `KissStreamDecoder`. Complete KISS data frames
become Reticulum packet payloads. Unknown KISS command frames are surfaced in
`RnodeSppRead::commands` for callers that need command visibility.

## KISS Flow Control

When `KissConfig::flow_control` is disabled, outbound packets write
immediately.

When flow control is enabled, outbound packets are queued until the runtime
receives a KISS `READY` command. A `READY` command marks the interface ready,
flushes one queued packet as a KISS data frame, and then returns to not-ready
state until the next `READY`.

The runtime also clears stale partial KISS frames when more stream bytes arrive
after `read_frame_timeout`, matching the behavior used by the other RNode KISS
bearers.

## Defaults

`RnodeSppKissConfig::default()` uses:

- Connect timeout: `5 s`
- KISS read-frame timeout: `1250 ms`
- KISS MTU: `508`
- Initial, deferred, and shutdown frames: empty
- KISS parameters: `KissConfig::default()`

`RnodeSppSettings::for_device_id(...)` uses the same connect and read timeout
defaults and leaves `device_name` empty unless `with_device_name(...)` is used.

## Backend Guidance

A platform backend should keep the trait methods narrow:

- `connect()` should open or attach to the Bluetooth Classic/RFCOMM stream for
  the configured device.
- `write(payload)` should write the provided KISS bytes in order and report
  write failures.
- `read()` should return the next available byte chunk, `Ok(None)` when no
  bytes are currently available, or an error when the stream fails.

The runtime does not require a one-to-one relationship between Bluetooth reads
and KISS frames. A backend can return partial frames, multiple frames, or any
stream-sized chunk; the KISS decoder handles reassembly.

## Verification

Focused software validation for this scaffold is:

```bash
cargo test -p reticulum-rs-transport --test rnode_spp
cargo clippy -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings
```

The current tests cover startup writes, stream-byte packet decoding,
READY-driven flow-control flushing, and connect-timeout enforcement.

## Known Limitations

- There is no native OS SPP backend in this repository yet.
- There is no `reticulumd` interface kind or operator config surface yet.
- Prepared-host hardware evidence is still required before making production
  RNode Bluetooth Classic support claims.
- SPP is separate from BLE. BLE adapter availability or BLE GATT UUID settings
  do not validate this path.
