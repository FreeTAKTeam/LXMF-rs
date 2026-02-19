# SDK v2.5 Cutover Map

Status: Draft, required before Phase 1 implementation

## Purpose

This map classifies each current SDK/RPC/event consumer path for the v2.5 hard break.

Classification values:

- `keep`: path remains with no compatibility wrapper
- `wrap`: path remains temporarily behind SDK v2.5 compatibility wrapper
- `deprecate`: path is removed from active integration surface

## Consumer Inventory

| Surface | Current path | Owner | Classification | Replacement | Removal version | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| RPC send | `rns-rpc::send_message_v2` | `rns-rpc` | keep | `lxmf-sdk::send` (adapter call-through) | n/a | Keep method for transition, SDK becomes primary contract |
| RPC events queue-pop | `rns-rpc::events` | `rns-rpc` | wrap | `rns-rpc::sdk_poll_events_v2` | `N+1` | Legacy queue-pop behind migration switch in `N` only |
| RPC cancel | `lxmf-legacy::router::outbound::cancel_outbound` | `rns-rpc` | wrap | `rns-rpc::sdk_cancel_message_v2` | `N+1` | Deterministic cancel result enum required |
| Runtime snapshot | runtime-specific snapshot path | runtime team | wrap | `rns-rpc::sdk_snapshot_v2` | `N+1` | Must include watermark and capability metadata |
| Direct runtime embedding API | `lxmf-runtime` direct surfaces | SDK team | deprecate | `lxmf-sdk` facade | `N` | Removed from active topology in hard-break cycle |

## Exit Criteria

1. All `wrap` rows include an explicit removal version and owner.
2. No `deprecate` row is referenced by release artifacts after `N`.
3. Migration CI gate validates table completeness (no empty owner/classification/replacement/removal-version cells).
