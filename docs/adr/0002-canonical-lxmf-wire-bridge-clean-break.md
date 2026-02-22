# ADR 0002: Canonical LXMF Wire Bridge Clean-Break

- Status: Accepted
- Date: 2026-02-19
- Deciders: LXMF-rs maintainers

## Context

The workspace had accumulated multiple compatibility paths across `lxmf`, `reticulum-daemon`, and `reticulum`:

- duplicate bridge/runtime conversion logic,
- relaxed inbound decode fallback controlled by environment flags,
- mixed attachment key handling (`attachments`, `files`, and public `"5"`),
- client fallback between `send_message_v2` and `send_message`.

This made behavior harder to reason about and increased parity risk between runtime and daemon paths.

## Decision

We adopt a clean-break canonical policy for v0.3:

1. Public JSON attachment contract is `attachments` only.
2. Legacy public aliases are rejected:
   - `files`
   - public numeric key `"5"`
3. Attachment text payloads must be explicit:
   - `hex:<payload>`
   - `base64:<payload>`
4. Relaxed inbound decode env toggles are removed.
5. Inbound payload shape selection is explicit (`FullWire` vs `DestinationStripped`) at call sites.
6. Client surfaces call `send_message_v2` directly; no client fallback to `send_message`.
7. Server keeps both `send_message` and `send_message_v2`, but both flow through the same strict outbound bridge path.
8. Shared helpers live in `reticulum` for:
   - delivery/link send outcomes and link send fallback behavior,
   - destination hash parsing,
   - receipt mapping and receipt status recording.

## Consequences

- External callers must migrate to canonical `attachments`.
- Legacy payload shapes now fail fast with clear errors.
- Runtime/daemon behavior is more deterministic and testable.
- Interop fixtures and schema artifacts must represent canonical public shapes.

## Verification

The rollout is gated by:

- workspace compile, format, clippy,
- runtime/daemon parity tests,
- RPC contract tests,
- external interop harness gates per release checklist.
