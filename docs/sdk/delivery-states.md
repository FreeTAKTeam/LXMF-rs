# SDK Delivery State Guide

This guide explains how application code should interpret message delivery
state without depending on daemon-internal status strings.

## State Model

App-facing delivery status is exposed as `DeliveryStatus`:

- `message_id`: stable handle returned by send
- `state`: typed `DeliveryState`
- `terminal`: whether no further state changes are expected
- `last_updated_ms`: backend update time
- `attempts`: observed send attempts
- `reason_code`: optional machine-readable reason

Use `DeliveryStatus.state` and `DeliveryStatus.terminal` for control flow. Do
not parse raw receipt text, trace messages, or RPC error strings.

## State Meanings

- `Queued`: accepted by the SDK/daemon, not yet dispatched
- `Dispatching`: actively being prepared, linked, propagated, or transferred
- `Sent`: handed to the selected transport or propagation path
- `Delivered`: delivery receipt observed
- `Failed`: terminal failure
- `Cancelled`: terminal cancellation
- `Expired`: terminal TTL/store expiry
- `Rejected`: terminal policy or validation rejection
- `Unknown`: compatibility fallback when the backend returns an unrecognized
  state

The app facade collapses lower-level `in_flight` into `Dispatching`.

## Terminality

Exactly one terminal state should win for a message. Terminal transitions are
storage-protected in the daemon and later conflicting transitions are rejected.

Terminal success depends on the negotiated capability:

- without receipt terminality, `Sent` can be terminal success
- with receipt terminality, `Sent` is non-terminal and `Delivered` is terminal
  success

Always trust the `terminal` flag returned with `DeliveryStatus` instead of
hard-coding terminal states in application code.

## Send Acceptance Versus Delivery

`messages().send_async(...)` returns send acceptance, not final delivery.

After send acceptance:

1. Subscribe to events for delivery transitions.
2. Use `messages().status_async(message_id)` for reconciliation.
3. Treat `Sent` as transport handoff unless `terminal` is true.
4. Treat `Delivered`, `Failed`, `Cancelled`, `Expired`, and `Rejected` with
   `terminal=true` as final outcomes.

## Event Ordering

Delivery events are at-least-once. Replayed events may appear after reconnect or
cursor recovery. Handlers should be idempotent and keep the newest
`last_updated_ms`/sequence for each message.

Expected causal order is progress before terminal. If a `StreamGapDetected`
event appears, reconcile with `status_async` or bounded cursor recovery before
showing final UI state.

## Recovery and Reconciliation

Use status reconciliation when:

- the event stream reconnects after a disconnect
- a cursor is stale, invalid, or expired
- a daemon restart happened mid-send
- queue pressure or backpressure delayed event delivery
- the app process restarted after send acceptance

Persist application-level correlation IDs and idempotency keys when the user
experience needs to survive process restart.
