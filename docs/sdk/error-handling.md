# SDK Error Handling Guide

This guide describes how application code should react to typed `lxmf-sdk`
errors without parsing raw RPC strings.

## Error Shape

App-facing failures preserve:

- `code`: stable machine-readable SDK code
- `category`: validation, config, capability, connectivity, delivery, timeout,
  security, runtime, persistence, or internal
- `retryable`: whether the SDK/backend considers a retry useful
- `message`: operator-facing summary

Use `code`, `category`, and `retryable` for control flow. Treat `message` as
diagnostic text only.

## Retry Policy

Prefer profile defaults through `messages().send_with_profile_defaults(...)` or
`messages().send_async_with_profile_defaults(...)`.

The helper returns a `SendReport` when a send is accepted. Failed attempts record
the retry disposition, delay, original error code, and whether queue pressure was
involved.

Manual retries should stay bounded:

1. Retry only when `retryable` is true.
2. Respect profile or host-specific attempt limits.
3. Use idempotency keys for process-level retry loops.
4. Stop on validation, config, capability, and security categories unless the
   user or operator changes input/configuration.

## Idempotency

Use `SendRequest::with_idempotency_key(...)` when an operation may be retried
after process restart, daemon restart, or network uncertainty.

The backend scopes idempotency by source, destination, and key for the negotiated
TTL. A conflicting retry surfaces as a typed validation/idempotency error instead
of creating a second logical send.

## Queue Pressure

Queue pressure is a delivery category signal. Profile helpers either fail fast
or apply bounded retry according to the active profile.

Applications should not spin or create unbounded local queues. On repeated queue
pressure, surface backpressure to the user/operator and wait for a new trigger
or a runtime state change event.

## Connectivity and Runtime Failures

Connectivity errors include daemon restart, stream disconnect, and transport
loss. Runtime errors include invalid lifecycle state, shutdown races, stale
cursor handling, and degraded event streams.

Recommended handling:

- keep the event stream active and let it reconnect when supported
- handle `StreamGapDetected` with snapshot or bounded cursor recovery
- re-check `runtime().status_async()` before retrying after disconnect
- use `runtime().stop_async(...)` during shutdown and avoid background retry
  tasks after stop

## Security Failures

Security category errors are not normal retry candidates. Token expiry, replay
rejection, missing mTLS client identity, and remote-bind policy failures require
new credentials or configuration changes.

Do not log secrets, bearer tokens, private keys, ticket material, or full payload
fields while handling these errors.
