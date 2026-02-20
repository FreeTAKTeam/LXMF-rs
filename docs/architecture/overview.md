# LXMF-rs Architecture

## Core Principles
- Protocol code is isolated from operator workflows.
- Runtime behavior is explicit and testable.
- Public API surfaces are narrow and crate-scoped.
- Security controls are default-on, measurable, and contract-backed.

## Stable Public Crates
- `lxmf-core`
- `lxmf-sdk`
- `rns-core`
- `rns-transport`
- `rns-rpc`

## Layering Rules
- `crates/libs/*` must not depend on `crates/apps/*`.
- `lxmf-core` must not directly depend on `tokio`, `clap`, `ureq`, or `serde_json`.
- `rns-core` must not directly depend on `tokio` or `clap`.
- CLI/daemon concerns live in `crates/apps/*`.

## Security Architecture
- Threat model source of truth: `docs/adr/0004-sdk-v25-threat-model.md`.
- Security review checklist source of truth: `docs/runbooks/security-review-checklist.md`.
- Primary controls:
  - Local-only default RPC binding with explicit secure auth required for remote bind.
  - Token replay protection (`jti`) and rate limiting for authenticated HTTP RPC.
  - Structured redaction for event/error payloads and request traces.
  - Cursor and stream-gap semantics that fail closed on invalid/expired cursors.
  - Bounded queue capacities with overflow policy enforcement.

## Legacy Cutover Note
Legacy implementation crates are no longer part of the active workspace graph. Migration artifacts remain only where retention is still required.
