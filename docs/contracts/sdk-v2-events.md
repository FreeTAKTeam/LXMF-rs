# SDK Contract v2.5 (Events and Streams)

Status: Draft, implementation target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Event-First Model

The SDK is event-first:

- commands initiate state changes
- events represent authoritative progression
- `status()` is a point-in-time projection

## Event Envelope

All events must include:

- `event_id`
- `runtime_id`
- `stream_id`
- `seq_no`
- `contract_version`
- `ts_ms`
- `event_type`
- `severity`
- `source_component`
- `operation_id` optional
- `message_id` optional
- `peer_id` optional
- `correlation_id` optional
- `trace_id` optional
- `extensions` optional map

Version mapping rule:

1. `event.contract_version` must equal negotiated `active_contract_version` for the runtime session.

## Event Types

Defined event types:

- `RuntimeStateChanged`
- `DeliveryStateTransition`
- `DeliveryRetryScheduled`
- `DeliveryTrace`
- `InboundMessageReceived`
- `ReceiptObserved`
- `InterfaceStateChanged`
- `PolicyChanged`
- `HealthSnapshot`
- `ErrorRaised`
- `StreamGap`
- `ExtensionEvent`

## Ordering and Deduplication

Rules:

1. `seq_no` is strictly monotonic per `{runtime_id, stream_id}`.
2. Per `message_id`, delivery transitions are monotonic.
3. Event delivery is at-least-once per cursor/subscription.
4. Duplicate events preserve `event_id`.
5. Consumer dedupe key is `{runtime_id, stream_id, event_id}`.

## Cursor Contract

`poll_events(cursor, max)` returns:

- `events`
- `next_cursor`
- `dropped_count`

Rules:

1. Cursor is opaque.
2. Cursor validity scope is `{runtime_id, stream_id, schema_namespace}`.
3. Out-of-scope cursor fails with `SDK_RUNTIME_INVALID_CURSOR`.
4. Expired cursor fails with `SDK_RUNTIME_CURSOR_EXPIRED`.
5. Cursor must never silently reset to head or tail.

## Snapshot Boundary Contract

For snapshot-start subscriptions:

1. First frame includes `snapshot_high_watermark_seq_no`.
2. Snapshot events are `<= watermark`.
3. Live events are `> watermark`.
4. Boundary interleaving is forbidden.

## Stream Gap Semantics

On detected loss, emit `StreamGap` with:

- `expected_seq_no`
- `observed_seq_no`
- `dropped_count`

If gap recovery is impossible, stream is considered degraded and must require explicit consumer action.

Degraded-stream rules:

1. The next `poll_events` call after non-recoverable gap must return `SDK_RUNTIME_STREAM_DEGRADED`.
2. Recovery action is explicit stream reset: `poll_events(cursor=None, max=...)`.
3. After reset, returned events start from current retained head and include a `StreamGap` event in the first batch.
4. Implementations must not silently heal by rewinding or skipping without `StreamGap`.

## Marker Sync Conflict Semantics

For multi-client marker writers:

1. Marker records carry a monotonic `revision`.
2. `marker_update_position` and `marker_delete` must include `expected_revision`.
3. Stale writes fail with `SDK_RUNTIME_CONFLICT` and conflict details:
- `domain=marker`
- `marker_id`
- `expected_revision`
- `observed_revision`
4. Clients must refresh from `marker_list` before retrying a conflicted marker write.

## Size and Rate Limits

Required caps:

- `max_poll_events`
- `max_event_bytes`
- `max_batch_bytes`
- `max_extension_keys`

Validation/error bindings:

1. `poll_events(max=...)` with `max > effective_limits.max_poll_events` must fail with `SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED`.
2. Any emitted event payload larger than `effective_limits.max_event_bytes` must fail emission and surface `SDK_VALIDATION_EVENT_TOO_LARGE`.
3. Any returned poll batch larger than `effective_limits.max_batch_bytes` must fail with `SDK_VALIDATION_BATCH_TOO_LARGE`.
4. Event `extensions` key count above `effective_limits.max_extension_keys` must fail with `SDK_VALIDATION_MAX_EXTENSION_KEYS_EXCEEDED`.

Suggested defaults:

- `max_poll_events`: `256` desktop-full, `64` desktop-local-runtime, `32` embedded-alloc
- `max_event_bytes`: `65_536` desktop-full, `32_768` desktop-local-runtime, `8_192` embedded-alloc
- `max_batch_bytes`: `1_048_576`
- `max_extension_keys`: `32`

## Privacy and Redaction

Event fields are classified:

- `public`
- `sensitive`
- `secret`

Rules:

1. `secret` fields must never be emitted.
2. `sensitive` fields must be transformed (hash/truncate/redact) by policy.
3. Raw payload diagnostics require break-glass policy with audit trail and TTL.

Deterministic baseline mapping:

1. `message_payload`, `auth_token`, `private_key_material`, `session_secret` are `secret`.
2. `peer_id`, `destination_hash`, `correlation_id`, `trace_id` are `sensitive`.
3. `event_id`, `runtime_id`, `stream_id`, `seq_no`, `event_type`, `severity`, `source_component`, `ts_ms` are `public`.

## Extension Events

`ExtensionEvent` rules:

1. Use namespaced event types (`vendor.domain.event_name`).
2. Names beginning with `sdk.` are reserved.
3. Unknown extension events must not break consumer parsing.
