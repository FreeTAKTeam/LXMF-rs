# RPC Contract (`reticulumd`) - CLI Stable Set

This document freezes the daemon RPC methods that `lxmf` relies on.

Scope:
- Transport: HTTP `POST /rpc` with framed MessagePack payloads.
- Event stream: HTTP `GET /events` with framed MessagePack events.
- Stability target: this method set and parameter shapes are considered stable for `0.1.x`.
- Message field-level payload IDs and structures are documented in `docs/payload-contract.md`.

Reference tests:
- `tests/rpc_contract_methods.rs`
- `tests/lxmf_rpc_client.rs`

## Wire framing

RPC request/response bodies are framed as:
- First 4 bytes: big-endian payload length (`u32`)
- Remaining bytes: MessagePack-encoded object

Request object:
- `id: u64`
- `method: string`
- `params: object | null` (method-specific)

Response object:
- `id: u64`
- `result: object | array | scalar | null`
- `error: { code: string, message: string } | null`

## Stable method set

All methods below are required for full CLI feature coverage.

### Messaging
- `list_messages` (no params)
: Returns message list or `{ messages: [...] }`.
- `clear_messages` (no params)
- `announce_now` (no params)
- `send_message_v2`
: Params keys: `id`, `source`, `destination`, `title`, `content` (optional: `fields`, `method`, `stamp_cost`, `include_ticket`, `try_propagation_on_fail`, `source_private_key`).
- `send_message`
: Compatibility server method with params keys: `id`, `source`, `destination`, `title`, `content` (optional: `fields`, `source_private_key`).

### Identity / status
- `daemon_status_ex` (no params)
: Must include `identity_hash` when available.
- `status` (no params)
: Fallback status method; must include `identity_hash` when available.

### Peers and interfaces
- `list_peers` (no params)
- `peer_sync`
: Params keys: `peer`
- `peer_unpeer`
: Params keys: `peer`
- `clear_peers` (no params)
- `list_interfaces` (no params)
- `set_interfaces`
: Params keys: `interfaces`
- `reload_config` (no params)

### Propagation
- `propagation_status` (no params)
- `propagation_enable`
: Params keys: `enabled`, `store_root`, `target_cost`
- `propagation_ingest`
: Params keys: `transient_id`, `payload_hex`
- `propagation_fetch`
: Params keys: `transient_id`

### Stamp / tickets
- `stamp_policy_get` (no params)
- `stamp_policy_set`
: Params keys: `target_cost`, `flexibility`
- `ticket_generate`
: Params keys: `destination`, `ttl_secs`

## Compatibility policy

- New methods may be added without breaking this contract.
- Existing method names in this document must not be renamed or removed in `0.1.x`.
- Existing required parameter keys must remain accepted.
- CLI/runtime clients must call `send_message_v2` directly (no client fallback to `send_message`).
- Server must keep `send_message` for compatibility and apply the same strict canonical field validation path as `send_message_v2`.
- At least one of `daemon_status_ex` or `status` must provide `identity_hash` for source auto-resolution.
