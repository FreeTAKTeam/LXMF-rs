# SDK Advanced Embedding

This guide covers host-side patterns for long-lived services and constrained environments.

## Capability-Negotiated Feature Use

Always branch on effective capabilities returned by `start`:

1. Request the capabilities you want.
2. Inspect `ClientHandle.effective_capabilities`.
3. Enable feature paths only when capability IDs are present.
4. Treat missing optional capabilities as supported degradation, not hard failure.

For required profile capabilities, startup must fail fast.

## Idempotency and Cancellation

Use `idempotency_key` for retry-safe sends across process retries.

Rules to preserve:

- Same `(source, destination, key)` + same payload within TTL returns original message id.
- Same tuple + different payload must be treated as conflict.
- Cancellation outcomes are explicit (`Accepted`, `AlreadyTerminal`, `NotFound`, `TooLateToCancel`).

Persist outbound intent and message IDs in host storage for deterministic recovery.

## Embedded and Manual Tick Integration

For `embedded-alloc`:

- budget work with `tick(TickBudget)`
- keep loop deterministic with bounded work items/duration
- use periodic `snapshot` for health checks and reconciliation

Pattern:

1. call `tick` with strict budget
2. poll events with small bounded batches
3. persist cursor and critical state
4. sleep/yield based on `TickResult.next_recommended_delay_ms`

## Error Taxonomy and Recovery Strategy

Group SDK failures into:

- validation/configuration errors (caller/actionable)
- runtime state errors (ordering/lifecycle bug or drift)
- transport/auth errors (connectivity/security posture)

Use `machine_code`, `category`, and retryability hints to drive host policy.

## Upgrade and Compatibility Discipline

- Pin supported contract versions in code.
- Validate negotiation across N/N+1/N+2 with conformance fixtures.
- Keep schema and fixture drift gates enabled in CI.
- Roll forward only with migration docs and baseline updates.
