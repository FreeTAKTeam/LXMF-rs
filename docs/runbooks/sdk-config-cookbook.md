# SDK Configuration and Policy Cookbook

This cookbook provides production-ready `SdkConfig` templates aligned with `docs/schemas/sdk/v2/config.schema.json`.

All JSON examples in this runbook map directly to validated fixtures in:

- `docs/fixtures/sdk-v2/cookbook/`

## Desktop Local Service (Default Safe Posture)

Use this for local-only service deployment with strict blocking backpressure.

- Fixture: `docs/fixtures/sdk-v2/cookbook/config.desktop_service_local.valid.json`
- Security posture:
  - `bind_mode=local_only`
  - `auth_mode=local_trusted`
  - redaction enabled

## Remote Token Gateway

Use this for authenticated remote clients where token verification and replay controls are required.

- Fixture: `docs/fixtures/sdk-v2/cookbook/config.remote_token_gateway.valid.json`
- Security posture:
  - `bind_mode=remote`
  - `auth_mode=token`
  - `token_auth` required (`issuer`, `audience`, `jti_cache_ttl_ms`, `shared_secret`)

## Remote mTLS Gateway

Use this for mutually authenticated client/server deployments.

- Fixture: `docs/fixtures/sdk-v2/cookbook/config.remote_mtls_gateway.valid.json`
- Security posture:
  - `bind_mode=remote`
  - `auth_mode=mtls`
  - CA bundle and client cert requirements explicit

## Embedded Alloc Profile

Use this for constrained hosts with reduced event/memory limits and manual tick integration.

- Fixture: `docs/fixtures/sdk-v2/cookbook/config.embedded_alloc.valid.json`
- Operational posture:
  - reduced event limits
  - short timeouts
  - redaction enabled with break-glass disabled

## Policy Anti-Pattern (Invalid Example)

Remote bind with local trusted auth is forbidden and intentionally invalid.

- Fixture: `docs/fixtures/sdk-v2/cookbook/config.remote_local_trusted.invalid.json`
- Rejected by schema and conformance gates.

## Validation Gate

Run:

```bash
cargo run -p xtask -- sdk-cookbook-check
```

The gate validates cookbook fixtures against the config schema and fails on drift.
