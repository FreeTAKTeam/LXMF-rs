# SDK Integration Guide

This guide is for teams embedding `lxmf-sdk` into services, desktop apps, and constrained hosts.
It complements the formal contracts under `docs/contracts/` with integration-focused guidance.

## Reading Order

1. `docs/sdk/quickstart.md`
2. `docs/sdk/configuration-profiles.md`
3. `docs/sdk/lifecycle-and-events.md`
4. `docs/sdk/advanced-embedding.md`

## Core Concepts

- `Client<RpcBackendClient>` is the primary host-facing entry point.
- Startup is contract-negotiated (`supported_contract_versions` + capabilities).
- Runtime behavior is profile-bound (`desktop-full`, `desktop-local-runtime`, `embedded-alloc`).
- Event ingestion is cursor-based (`poll_events`) and explicitly backpressured.
- Domain APIs are capability-gated and must be feature-detected after `start`.

## Source-of-Truth Contracts

- `docs/contracts/sdk-v2.md`
- `docs/contracts/sdk-v2-events.md`
- `docs/contracts/sdk-v2-errors.md`
- `docs/contracts/sdk-v2-feature-matrix.md`
