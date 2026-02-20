# ADR 0004: SDK v2.5 Threat Model and Security Mitigation Map

- Status: accepted
- Date: 2026-02-20
- Owners: LXMF-rs maintainers
- Scope: `lxmf-sdk`, `rns-rpc`, `reticulumd`, contract/security gates

## Context

SDK v2.5 introduced hard-break API and runtime boundaries. Security controls already exist in code (auth mode validation, replay rejection, redaction, rate limits), but a formal threat model was missing as a review and release artifact.

## Decision

Adopt a STRIDE-style threat inventory with explicit mitigation mapping to code and tests. Treat this ADR and `docs/runbooks/security-review-checklist.md` as release-gated security architecture artifacts.

## STRIDE Threat Inventory

| STRIDE | Threat | Primary target | Severity |
| --- | --- | --- | --- |
| Spoofing | remote caller impersonates trusted local client | RPC authn/authz boundary | High |
| Tampering | RPC frame/body mutation and capability/config patch abuse | RPC transport and request validation | High |
| Repudiation | missing or unverifiable operation traceability | operator audit and incident forensics | Medium |
| Information Disclosure | secrets leak in events/errors/logs | telemetry, diagnostics, event stream | High |
| Denial of Service | queue flood, oversized payloads, auth brute force | daemon process availability | High |
| Elevation of Privilege | unauthorized remote bind or disabled auth mode bypass | runtime configuration control plane | Critical |

## Mitigation Map

| Threat | Mitigation | Code surface | Validation evidence |
| --- | --- | --- | --- |
| Spoofing | `local_only` default, explicit secure auth required for remote binds, token signature validation, mTLS transport-context checks | `crates/libs/rns-rpc/src/rpc/daemon/sdk_auth_http.rs`, `crates/libs/lxmf-sdk/src/types.rs` | `sdk_security_authorize_http_request_*` tests |
| Tampering | strict request decoding and schema-aligned field validation, unknown-key rejection, CAS config revision checks | `crates/libs/rns-rpc/src/rpc/daemon/dispatch.rs`, `crates/libs/rns-rpc/src/rpc/daemon/sdk_helpers.rs` | `sdk_dispatch_maps_*`, `sdk_configure_v2_*` tests |
| Repudiation | correlation IDs and lifecycle trace events for send/cancel/config/shutdown | `crates/libs/rns-rpc/src/rpc/daemon/dispatch.rs`, `crates/apps/reticulumd/src/bin/reticulumd/rpc_loop.rs` | `sdk_lifecycle_traces_include_correlation_fields` |
| Information Disclosure | field redaction transforms (`hash|truncate|redact`) and sensitive-key traversal | `crates/libs/rns-rpc/src/rpc/daemon/events.rs` | `sdk_security_events_redact_sensitive_fields_by_default` |
| DoS | bounded event queues, overflow policy enforcement, max event/batch limits, rate limits | `crates/libs/rns-rpc/src/rpc/daemon/events.rs`, `crates/libs/rns-rpc/src/rpc/daemon/sdk_negotiate_poll.rs` | `sdk_event_queues_remain_bounded_under_sustained_load`, `sdk_poll_events_v2_rejects_*`, `sdk_security_authorize_http_request_enforces_rate_limits_and_emits_event` |
| EoP | auth mode constraints and profile restrictions (e.g., embedded/mTLS constraints), capability gating | `crates/libs/lxmf-sdk/src/types.rs`, `crates/libs/rns-rpc/src/rpc/daemon/sdk_negotiate_poll.rs` | `config_rejects_remote_bind_without_token_or_mtls`, `sdk_negotiate_v2_rejects_mtls_for_embedded_alloc_profile` |

## Residual Risks

- Token secret management remains deployment-owned unless external keystore/HSM backends are used.
- mTLS trust-store lifecycle (rotation/revocation) requires operator procedures in deployment runbooks.
- Bounded queues prevent unbounded memory growth but can still drop data under severe sustained pressure; clients must handle `dropped_count`/`StreamGap`.

## Consequences

- Security architecture now has explicit release artifacts and checklist-driven verification.
- Security review and release readiness can fail fast when threat-model/checklist artifacts drift.
