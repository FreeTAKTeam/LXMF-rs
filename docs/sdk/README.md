# SDK Integration Guide

This guide is for teams embedding `lxmf-sdk` into services, desktop apps, and constrained hosts.
It complements the formal contracts under `docs/contracts/` with integration-focused guidance.

The current package release is 0.16.0 and requires Rust 1.88 or newer. Its core
SDK contract is v2.6, with schema namespace `v2` and negotiated protocol version
`2`; v2.5 requests remain accepted. Domain contracts that retain a v2.5 heading
still define those domains. Package versions and contract versions are separate.

## Reading Order

1. [Quickstart](quickstart.md)
2. [Checked examples](../examples.md)
3. [Migration to easy mode](migration-to-easy.md)
4. [Kotlin wrapper](../../wrappers/kotlin-mobile/README.md)
5. [Configuration and profiles](configuration-profiles.md)
6. [Lifecycle and events](lifecycle-and-events.md)
7. [Polling migration](polling-to-events-migration.md)
8. [Remote mTLS](remote-mtls.md)
9. [Delivery states](delivery-states.md)
10. [Error handling](error-handling.md)
11. [Advanced embedding](advanced-embedding.md)

## Core Concepts

- Start app integrations with `lxmf_sdk::app::Client` and typed domain handles.
  `Client<ZmqPipelineBackendClient>` is the preferred desktop/service path;
  `Client<RpcBackendClient>` supports Unix, HTTP, and TLS compatibility flows.
- Startup is contract-negotiated (`supported_contract_versions` + capabilities).
- Runtime behavior is profile-bound (`desktop-full`, `desktop-local-runtime`, `embedded-alloc`).
- RPC-backed `runtime().start_async(...)`, `messages().send_async(...)`,
  `messages().status_async(...)`, and `runtime().stop_async(...)` use Tokio-native request/response
  transport rather than blocking the executor.
- App-facing event ingestion is stream-first through typed SDK events. The RPC backend opens the
  daemon's native framed event stream over `unix:/path`, TCP, or TLS/mTLS when configured.
- The ZeroMQ backend owns event wakeups and cursor catch-up internally.
  Applications consume its typed stream; ordinary cursor replay is distinct
  from the opt-in durable broker journal.
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

App-facing contracts:

- `docs/contracts/sdk-app-api-v1.md`
- `docs/contracts/sdk-app-events-v1.md`
- `docs/contracts/sdk-app-errors-v1.md`
- `docs/contracts/sdk-app-profiles-v1.md`

## Upgrade and durable custody

Use the [v0.14.0 migration guide](../migrations/v0.14.0-zmq.md) when upgrading
the SDK and daemon together. Durable custody is opt-in and requires
`--zmq-durable-broker`, persistent storage, and explicit negotiation of
`sdk.capability.durable_broker_v1`. A stored acknowledgement records custody,
not delivery to a peer. The [operations runbook](../goals/zmq-rch-durable-broker/OPERATIONS.md)
covers consumer checkpoints, capacity pressure, restore, and downgrade.
