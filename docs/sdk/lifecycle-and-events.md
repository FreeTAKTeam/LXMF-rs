# SDK Lifecycle and Event Flow

This document defines practical lifecycle usage around the v2.5 state model.

## Lifecycle State Machine

Runtime states:

- `New`
- `Starting`
- `Running`
- `Draining`
- `Stopped`
- `Failed`

Call legality is enforced per state by `LxmfSdk` and daemon contract logic.
Illegal transitions return typed `SdkError` values (`SDK_RUNTIME_INVALID_STATE` family).

## Cursor Polling Pattern

`poll_events(cursor, max)` returns:

- ordered event batch
- next cursor token
- dropped count for stream-gap handling

Recommended polling loop:

1. Start with `cursor = None`.
2. Process events in order.
3. Persist `batch.next_cursor`.
4. Resume from persisted cursor on restart.
5. Treat invalid/expired cursor as explicit recovery path, not silent reset.

## Event Handling Guidance

- Keep handlers idempotent; delivery updates can be at-least-once.
- Preserve correlation fields (`trace_ref`, `correlation_id`) in host logs.
- Respect redaction defaults and avoid logging full payloads in hot paths.
- Handle `StreamGap` semantics as data-loss indicators and trigger resync/snapshot.

## Snapshot and Reconciliation

Use `snapshot()` periodically and during recovery:

- verify runtime state and watermarks
- reconcile missed delivery states after cursor invalidation
- detect queue pressure or degraded event streaming state

## Async Subscriptions

When `sdk-async` is enabled and negotiated:

- use `subscribe_events(start)` for stream-style consumers
- preserve the same ordering/recovery assumptions as cursor polling

Capability absence must gracefully fall back to `poll_events`.
