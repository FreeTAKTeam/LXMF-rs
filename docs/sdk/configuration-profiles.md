# SDK Configuration and Profiles

`SdkConfig` combines runtime policy, event buffering, redaction, and RPC transport controls.

## Profile Selection

Use a single profile per runtime session:

- `desktop-full`: full capability envelope, async/event-heavy workloads.
- `desktop-local-runtime`: tighter local-service default profile.
- `embedded-alloc`: constrained capability set with manual tick expectations.

Profile limits and required capabilities are contract-governed by:

- `docs/contracts/sdk-v2-feature-matrix.md`

## Security Baselines

Recommended defaults:

- `bind_mode = local_only`
- `auth_mode = local_trusted`
- `redaction.enabled = true`

Remote bind requires explicit secure auth:

- `auth_mode = token` with replay-safe `jti` controls
- or `auth_mode = mtls` with transport-bound certificate validation

Do not expose remote bind with `local_trusted`.

## Event Stream and Backpressure

Tune event buffers using:

- `max_poll_events`
- `max_event_bytes`
- `max_batch_bytes`
- `max_extension_keys`

Overflow behavior:

- `reject`: keep older entries, drop new events
- `drop_oldest`: evict head, keep newest events
- `block`: stall producer with `block_timeout_ms` bound

Operational tuning guidance:

- `docs/runbooks/queue-pressure-tuning.md`
- `docs/runbooks/sdk-config-cookbook.md`

## Mutable vs Immutable Config Fields

Mutable at runtime via `configure(expected_revision, patch)`:

- event stream limits
- redaction policy
- selected backend tuning fields

Immutable after `start`:

- `profile`
- bind/auth mode core posture

See `docs/contracts/sdk-v2.md` for revision-CAS and patch semantics.
