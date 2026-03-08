# SDK App Events v1

Status: Draft, implementation target  
Contract family: `sdk-app`  
Contract release: `v1`

## Purpose

This document defines the typed app-level event model for app-api consumers.

The default event surface must hide raw transport and polling details. Advanced consumers may opt into lower-level streams separately.

## Event Model

Each event includes a stable envelope:

- `event_id`
- `runtime_id`
- `seq_no`
- `occurred_at_ms`
- `event_type`
- `severity`
- `operation_id` optional
- `message_id` optional
- `correlation_id` optional
- `profile_id`
- `extensions` optional

Rules:

1. `seq_no` is strictly monotonic per runtime session.
2. Duplicate delivery may occur only if the broader SDK contract allows at-least-once replay; duplicates must preserve `event_id`.
3. Unknown future additive fields must be safely ignored.

## Typed App Event Set

Required app-level event types:

- `RuntimeStarted`
- `RuntimeStopped`
- `RuntimeDegraded`
- `RuntimeRecovered`
- `MessageQueued`
- `MessageDispatching`
- `MessageSent`
- `MessageDelivered`
- `MessageFailed`
- `MessageCancelled`
- `InboundMessageReceived`
- `QueuePressureRaised`
- `RetryScheduled`
- `ReconnectScheduled`
- `StreamGapDetected`
- `SecurityActionRequired`
- `FatalErrorRaised`

Wrappers should present these as typed domain events instead of exposing raw event-type strings by default.

## Event Ordering Rules

1. Runtime lifecycle events must preserve causal order.
2. Delivery progression for a single `message_id` must be monotonic.
3. A terminal delivery event must be the final delivery-state event for that message.
4. `StreamGapDetected` must never be hidden or auto-healed silently.
5. Reconnect and retry scheduling events must occur before the follow-up delivery attempt they describe.

## Delivery-State Event Semantics

Delivery progression states exposed to apps:

- `queued`
- `dispatching`
- `sent`
- `delivered`
- `failed`
- `cancelled`

Rules:

1. `send()` acceptance should produce `MessageQueued` or a documented equivalent initial state.
2. `MessageSent` is not necessarily terminal.
3. `MessageDelivered` is terminal success.
4. `MessageFailed` and `MessageCancelled` are terminal failure states.
5. If a profile cannot observe `delivered`, that limitation must be explicit in profile semantics.

## Queue Pressure and Retry Events

App API policy must be visible to the app in typed form.

Required semantics:

- `QueuePressureRaised`
  - emitted when queue admission or pipeline pressure affects behavior
- `RetryScheduled`
  - emitted when the SDK schedules a retry
- `ReconnectScheduled`
  - emitted when the SDK schedules reconnection or session recovery

Rules:

1. These events are informational, not low-level transport trivia.
2. They must include enough context for UI/telemetry without leaking secrets.
3. The app must not need to infer retry behavior from generic error strings.

## Stream Gap and Recovery Events

App API must not hide data-loss indicators.

Required event:

- `StreamGapDetected`

Required fields:

- `expected_seq_no`
- `observed_seq_no`
- `dropped_count`
- `recovery_required`

Rules:

1. Silent gap healing is forbidden.
2. If explicit consumer action is required, `recovery_required` must be true.
3. Gap semantics must be identical across wrappers.

## Callback and Subscription Semantics

Bindings may use callbacks, async streams, observers, or channels, but semantics are fixed:

1. Default subscriptions receive typed app-level events.
2. Subscription closure is explicit.
3. Blocking waits or async awaits must resolve deterministically on stop, restart, close, or fatal failure.
4. The same event order must be preserved regardless of callback or async facade style.

## Extension Behavior

App API default consumers should rarely need extension events.

Rules:

1. Unknown extension events must not break parsing.
2. Extension events are advanced-only unless promoted into the typed app-api set.
3. Bindings should expose raw extension details separately from the default app event surface.
