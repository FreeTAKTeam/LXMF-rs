# ADR 0008: Extension and Plugin Contract Model

- Status: Accepted
- Date: 2026-02-21

## Context

SDK v2.5 is expanding into optional domain surfaces (topics, telemetry, attachments, markers,
identity, voice, and future modules). We need controlled extensibility without destabilizing core
contract guarantees.

## Decision

Adopt a plugin negotiation model layered on top of capability negotiation:

1. Plugin descriptors declare `required_capabilities`.
2. Plugin activation is determined by explicit negotiation (`negotiate_plugins`).
3. Unknown plugin IDs are ignored for forward compatibility.
4. Plugin lifecycle is classified as `experimental`, `stable`, or `deprecated`.
5. Core SDK method contracts remain authoritative regardless of plugin activation.

## Consequences

Positive:

- Optional modules can evolve independently.
- Hosts can enable only vetted plugin sets.
- Contract drift is reduced by explicit activation rules.

Tradeoffs:

- Additional governance is required for plugin lifecycle transitions.
- Plugin version/capability compatibility must be validated in CI.

## Enforcement

- Contract reference: `docs/contracts/sdk-v2-backends.md`
- CI gate: `cargo run -p xtask -- plugin-negotiation-check`
