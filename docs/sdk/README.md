# SDK Integration Guide

This guide is for teams embedding `lxmf-sdk` into services, desktop apps, and constrained hosts.
It complements the formal contracts under `docs/contracts/` with integration-focused guidance.

## Reading Order

1. `docs/sdk/quickstart.md`
2. `docs/sdk/migration-to-easy.md`
3. `wrappers/kotlin-mobile/README.md`
4. `docs/sdk/configuration-profiles.md`
5. `docs/sdk/lifecycle-and-events.md`
6. `docs/sdk/polling-to-events-migration.md`
7. `docs/sdk/remote-mtls.md`
8. `docs/sdk/delivery-states.md`
9. `docs/sdk/error-handling.md`
10. `docs/sdk/advanced-embedding.md`

## Core Concepts

- `Client<RpcBackendClient>` is the primary host-facing entry point.
- Startup is contract-negotiated (`supported_contract_versions` + capabilities).
- Runtime behavior is profile-bound (`desktop-full`, `desktop-local-runtime`, `embedded-alloc`).
- RPC-backed `runtime().start_async(...)`, `messages().send_async(...)`,
  `messages().status_async(...)`, and `runtime().stop_async(...)` use Tokio-native request/response
  transport rather than blocking the executor.
- App-facing event ingestion is stream-first through typed SDK events. The RPC backend opens the
  daemon's native framed event stream over `unix:/path`, TCP, or TLS/mTLS when configured.
- Cursor polling remains available for recovery, embedded/manual integrations, and low-level diagnostics.
- Public app workflows are grouped behind domain handles: `runtime()`, `messages()`,
  `events()`, `identity()`, and `attachments()`.
- Domain APIs are capability-gated and must be feature-detected after `start`.

## Source-of-Truth Contracts

- `docs/contracts/sdk-v2.md`
- `docs/contracts/sdk-v2-events.md`
- `docs/contracts/sdk-v2-errors.md`
- `docs/contracts/sdk-v2-feature-matrix.md`
- `docs/architecture/json-lxmf-fields.md` for JSON-to-wire field mapping details

App API roadmap contracts:

- `docs/contracts/sdk-app-api-v1.md`
- `docs/contracts/sdk-app-events-v1.md`
- `docs/contracts/sdk-app-errors-v1.md`
- `docs/contracts/sdk-app-profiles-v1.md`
