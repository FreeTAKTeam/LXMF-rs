# Compatibility Matrix v1

Last updated: 2026-02-20

## Matrix Version

- `matrix_version`: `1`
- `contract_generation`: `sdk-v2.5-hard-break`
- `matrix_owner`: `docs/contracts/compatibility-matrix.md`
- `normative_references`:
  - `docs/contracts/rpc-contract.md`
  - `docs/contracts/payload-contract.md`
  - `docs/contracts/sdk-v2.md`
  - `docs/contracts/sdk-v2-events.md`
  - `docs/contracts/third-party-compatibility-kit.md`
  - `docs/contracts/extension-registry.md`

## Protocol Slice Definitions

| Slice ID | Required spec | Summary |
| --- | --- | --- |
| `rpc_v2` | `docs/contracts/rpc-contract.md` | Framed MessagePack RPC request/response contract and stable method names. |
| `payload_v2` | `docs/contracts/payload-contract.md` | Canonical payload field IDs, attachments, and envelope behavior. |
| `event_cursor_v2` | `docs/contracts/sdk-v2-events.md` | Cursor-based event polling, monotonic sequencing, and stream-gap semantics. |
| `domain_release_b` | `docs/contracts/sdk-v2-backends.md` | Topics, telemetry, attachments, markers, and identity domain endpoints. |
| `domain_release_c` | `docs/contracts/sdk-v2-backends.md` | Paper commands, remote command markers, and voice signaling domain endpoints. |
| `delivery_modes_v1` | `docs/contracts/sdk-v2.md` | Direct, opportunistic, propagated, and paper workflow semantics. |
| `auth_token_v1` | `docs/contracts/sdk-v2-errors.md` | Token auth requirements, replay rejection (`jti`), and redacted failures. |
| `auth_mtls_v1` | `docs/contracts/sdk-v2-errors.md` | Transport-bound mTLS identity checks with explicit client material policy. |

## Client Matrix (v1)

Legend: `required`, `optional`, `planned`, `n/a`.

| Client | Version window | RPC v2 | Payload v2 | Event Cursor v2 | Release B Domains | Release C Domains | Auth Token | Auth mTLS | Delivery Modes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `lxmf-sdk` (Rust) | `N, N+1, N+2` | required | required | required | required | required | required | optional | required |
| `reticulumd` (Rust daemon) | `N, N+1, N+2` | required | required | required | required | required | required | optional | required |
| `Sideband` (external client track) | `Pinned interop baseline` | required | required | required | optional | planned | required | optional | required |
| `RCH` (external client track) | `Pinned interop baseline` | required | required | required | optional | planned | required | optional | required |
| `Columba` (external client track) | `Pinned interop baseline` | required | required | required | optional | planned | required | optional | required |

## Support Windows

| Window | Meaning | Compatibility rule |
| --- | --- | --- |
| `N` | Current release | All `required` slices must pass conformance and schema gates. |
| `N+1` | Next release | Additive capability growth only; no silent behavior change in existing `required` slices. |
| `N+2` | Following release | Breaking removals require migration entry in `docs/contracts/sdk-v2-migration.md`. |

## Validation Gates

- Matrix structure lint: `cargo run -p xtask -- interop-matrix-check`
- Schema and fixture drift: `cargo run -p xtask -- interop-artifacts`
- Golden corpus replay: `cargo run -p xtask -- interop-corpus-check`
- Semantic drift classification: `cargo run -p xtask -- interop-drift-check`
- Delivery workflow E2E suite: `cargo run -p xtask -- e2e-compatibility`
- SDK conformance coverage: `cargo run -p xtask -- sdk-conformance`
- External kit dry-run gate: `cargo run -p xtask -- compat-kit-check`
- Extension registry governance: `cargo run -p xtask -- extension-registry-check`

## Change Control

- Additive slice changes require updates in this file and the referenced normative contract.
- New client rows must declare a `Version window` and all slice statuses.
- Status demotions (`required` -> `optional`/`planned`) require an ADR in `docs/adr`.
