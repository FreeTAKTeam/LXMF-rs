# SDK Configuration and Profiles

`SdkConfig` combines runtime policy, event buffering, redaction, and RPC transport controls.
The app-facing layer in `lxmf_sdk::app` adds profile-derived policy helpers on top of that lower-level config surface.

## App Profiles

For most apps, start from the app-facing presets instead of constructing `SdkConfig` manually:

- `app::Config::from_profile(app::Profile::MobileDefault)`
- `app::Config::from_profile(app::Profile::DesktopDefault)`
- `app::Config::from_profile(app::Profile::EmbeddedDefault)`
- `app::Config::from_profile(app::Profile::TestingDefault)`

The app layer also exposes:

- `Config::delivery_plan()`
- `Client::send_with_profile_defaults(request)`
- `Client::send_with_options(request, options)`

These helpers apply the profile’s bounded retry and queue-pressure policy so callers do not need their own default retry loops.

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
- `docs/sdk/remote-mtls.md`

## Mutable vs Immutable Config Fields

Mutable at runtime via `configure(expected_revision, patch)`:

- event stream limits
- redaction policy
- selected backend tuning fields

Immutable after `start`:

- `profile`
- bind/auth mode core posture

See `docs/contracts/sdk-v2.md` for revision-CAS and patch semantics.
