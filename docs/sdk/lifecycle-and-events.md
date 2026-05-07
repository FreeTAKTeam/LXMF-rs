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

## Event-Driven Pattern

The default app-facing path is `subscribe_events(start)`, which yields typed events through the
Rust async stream surface. With the RPC backend this is backed by the daemon's native framed event
stream over `unix:/path`, TCP, or TLS/mTLS, so applications do not need a one-second polling loop.
Applications should consume that stream and handle domain events directly:

1. Start the runtime.
2. Subscribe with `Head`, `Tail`, or `Snapshot`.
3. Process typed events in order.
4. Treat `StreamGap` as an explicit data-loss signal.
5. Use snapshot/cursor recovery only when the stream reports degraded state.

The stream updates its cursor from each event sequence number. If the connection drops, the SDK
reconnects with the latest cursor and deduplicates replayed sequence numbers.
If a framed payload is malformed, the SDK treats that connection as failed,
does not emit the malformed frame, and reconnects from the latest successfully
delivered cursor.
After at least one successful connection, transient reconnect failures during
daemon restart or network recovery are retried instead of ending the
subscription.

## Cursor Polling Pattern

`poll_events(cursor, max)` returns:

- ordered event batch
- next cursor token
- dropped count for stream-gap handling

Recommended recovery loop:

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

- use `subscribe_events(start)` for app-facing consumers
- preserve the same ordering/recovery assumptions as cursor polling
- treat cursor polling as a fallback/reconciliation path, not the steady-state delivery mechanism

Capability absence is an advanced compatibility case; do not make periodic polling the default app
integration pattern.
