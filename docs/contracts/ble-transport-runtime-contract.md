# BLE Transport Runtime Contract

Status: draft
Owners: `reticulumd` maintainers

## Scope

Defines daemon-side BLE transport worker behavior for long-lived operation.

## Boundary Rules

- OS-native BLE dependencies stay in `reticulumd` adapter layer.
- `rns-transport` remains runtime-agnostic and must not depend on OS BLE crates.

## Worker Model

- Worker is spawned through `InterfaceManager`.
- Queue capacity: `64` frames.
- Enqueue timeout: `200ms`.

Lifecycle phases:

- `scan`
- `connect`
- `discover`
- `subscribe`
- `run`
- `degraded`
- `reconnect`

## State Transition Rules

Enter `degraded` when any of:

- BLE disconnect event.
- 3 consecutive BLE I/O failures.
- Connect timeout.

Reconnect behavior:

- Initial backoff: `500ms`.
- Exponential factor: `x2`.
- Max backoff: `10s`.
- Reset backoff after `60s` continuous healthy `run`.

Failure handling:

- Strict mode: terminate process after 20 consecutive reconnect failures.
- Best-effort mode: after 20 failures, continue reconnect attempts every 30s indefinitely.

## Frame Drop and Backpressure Policy

Non-droppable frames:

- `CAPTURE_REQ`
- `DONE`
- `ERROR`

Droppable frames:

- `HEARTBEAT` only.

`CHUNK` policy:

- Never silently dropped.
- On enqueue timeout, increment timeout counter and enter retry path.
- Retry budget exhaustion maps to `SDK_RUNTIME_BACKPRESSURE_TIMEOUT`.

## Shutdown Contract

Shutdown sequence is mandatory and ordered:

1. cancel token
2. unsubscribe
3. disconnect
4. stop scan
5. drain channels

## Runtime Status Surface

BLE interface runtime settings must always expose:

- `startup_status`
- `runtime_status`
- `startup_error` (optional)
- `runtime_error` (optional)
- `reconnect_attempts`
