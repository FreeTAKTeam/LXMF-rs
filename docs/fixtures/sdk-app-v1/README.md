# SDK App v1 Fixtures

These fixtures are release-gated conformance artifacts for the app-facing `sdk-app` contract.

They are consumed first by Rust contract tests in `crates/libs/test-support` and are intended to be
mirrored by the future first-party wrappers.

Current fixture set:

- `manifest.json`
- `lifecycle.start_stop_restart.json`
- `events.delivery_ordering.json`
- `timeout.poll_timeout.json`
- `delivery.queue_pressure.json`
- `connectivity.reconnect_recovery.json`
- `errors.typed_mapping.json`
- `compatibility.unknown_additive.json`
