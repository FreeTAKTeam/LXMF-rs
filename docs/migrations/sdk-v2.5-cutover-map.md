# SDK v2.5 Cutover Map

Status: Active, validated by `cargo xtask sdk-migration-check`

## Purpose

This map classifies each current SDK/RPC/event consumer path for the v2.5 hard break.

Classification values:

- `keep`: path remains with no compatibility wrapper
- `wrap`: path remains temporarily behind SDK v2.5 compatibility wrapper
- `deprecate`: path is removed from active integration surface

## Consumer Inventory

| Surface | Current path | Owner | Classification | Replacement | Removal version | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| SDK RPC send | `rns_rpc::sdk_send_v2` | `reticulum-rs-rpc` | keep | `lxmf-sdk::send` | `n/a` | Canonical core send transport for SDK facades |
| Legacy RPC send alias | `rns_rpc::send_message_v2` | `reticulum-rs-rpc` | wrap | `rns_rpc::sdk_send_v2` | `N+1` | Retained only for compatibility clients and tooling migration window |
| Legacy RPC send v1 | `rns_rpc::send_message` | `reticulum-rs-rpc` | wrap | `rns_rpc::sdk_send_v2` | `N+1` | Legacy alias path must remain strict-canonical during wrap period |
| SDK RPC events | `rns_rpc::sdk_poll_events_v2` | `reticulum-rs-rpc` | keep | `lxmf-sdk::poll_events` | `n/a` | Cursor and stream-gap semantics are authoritative in this path |
| Legacy events queue-pop | `rns_rpc::events` | `reticulum-rs-rpc` | wrap | `rns_rpc::sdk_poll_events_v2` | `N+1` | Legacy queue-pop behind migration switch in `N` only |
| SDK RPC cancel | `rns_rpc::sdk_cancel_message_v2` | `reticulum-rs-rpc` | keep | `lxmf-sdk::cancel` | `n/a` | Deterministic `CancelResult` enum is required |
| RPC cancel legacy bridge | retired `lxmf-legacy::router::outbound::cancel_outbound` path | `reticulum-rs-rpc` | deprecate | `rns_rpc::sdk_cancel_message_v2` | `N` | Legacy crate bridge is removed; active clients must use SDK/RPC cancel |
| Runtime snapshot | runtime-specific snapshot path | runtime team | wrap | `rns_rpc::sdk_snapshot_v2` | `N+1` | Snapshot response must include watermark and capability metadata |
| Direct runtime embedding API | retired `lxmf-runtime` direct surfaces | `sdk` | deprecate | `lxmf-sdk` facade | `N` | Removed from active topology in hard-break cycle |
| Transitional router/runtime crates | retired `crates/libs/lxmf-router`, `crates/libs/lxmf-runtime` | `architecture` | deprecate | `lxmf-wire`, `lxmf-sdk`, `reticulum-rs-core`, `reticulum-rs-transport`, `reticulum-rs-rpc` | `N` | Transitional stubs are removed from the repository |
| LXMF CLI client integration | `crates/apps/lxmf-cli/src/main.rs` | `lxmf-cli` | keep | `lxmf-sdk::Client<RpcBackendClient>` | `n/a` | CLI must remain SDK-first with no direct legacy runtime embedding |
| RNS tools compatibility harness | `crates/apps/rns-tools/src/bin/rnx.rs` | `rns-tools` | wrap | `sdk_*_v2` method set for send/cancel/events paths | `N+1` | Tooling still exercises legacy methods for compatibility evidence |
| Conformance harness integration | `crates/libs/test-support/src/sdk_conformance/mod.rs` | `test-support` | keep | `lxmf-sdk` + `reticulum-rs-rpc` conformance path | `n/a` | Source of truth for migration regression coverage |

## Exit Criteria

1. All `wrap` rows include an explicit removal version and owner.
2. No `deprecate` row is referenced by release artifacts after `N`.
3. Migration CI gate validates table completeness (no empty owner/classification/replacement/removal-version cells).
