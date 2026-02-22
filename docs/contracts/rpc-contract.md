# RPC Contract (`reticulumd`) - CLI Stable Set

This document freezes the daemon RPC methods that `lxmf` relies on.

Scope:
- Transport: HTTP `POST /rpc` with framed MessagePack payloads.
- Event stream: HTTP `GET /events` with framed MessagePack events.
- Stability target: this method set and parameter shapes are considered stable for `0.1.x`.
- Message field-level payload IDs and structures are documented in `docs/contracts/payload-contract.md`.

Compatibility slice:
- `slice_id`: `rpc_v2`
- Matrix source: `docs/contracts/compatibility-matrix.md`
- Extension registry: `docs/contracts/extension-registry.md`
- Support windows: `N`, `N+1`, `N+2` with additive-only method evolution.

Reference tests:
- In-repo contract coverage: `cargo xtask release-check` and `cargo test -p rns-tools`.
- Golden corpus replay: `cargo run -p xtask -- interop-corpus-check`.
- Deterministic RPC replay trace: `cargo run -p rns-tools --bin rnx -- replay --trace docs/fixtures/sdk-v2/rpc/replay_known_send_cancel.v1.json`.
- External interoperability contract checks are executed from the dedicated interop repository.

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

### Interface mutation policy (`set_interfaces` and `reload_config`)

The following contract is mandatory in v1:

1. `set_interfaces` accepts only legacy hot-apply kinds (`tcp_client`, `tcp_server`).
2. If any startup-only kind is present (`serial`, `ble_gatt`, `lora`, or unknown future kinds),
   the request is rejected atomically with:
   - `error.code = "CONFIG_RESTART_REQUIRED"`
   - `error.machine_code = "UNSUPPORTED_MUTATION_KIND_REQUIRES_RESTART"`
   - details include operation and affected interface identifiers.
3. No partial apply is allowed when rejection occurs.
4. `reload_config` without params preserves legacy behavior and emits `config_reloaded`.
5. `reload_config` with `interfaces` params hot-applies only when interface list length/order/kinds
   remain legacy TCP-only; otherwise it returns the same restart-required error contract.

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
- Additive extension behavior must be tracked in `docs/contracts/extension-registry.md` with versioned IDs.
- CLI/runtime clients must call `send_message_v2` directly (no client fallback to `send_message`).
- Server must keep `send_message` for compatibility and apply the same strict canonical field validation path as `send_message_v2`.
- At least one of `daemon_status_ex` or `status` must provide `identity_hash` for source auto-resolution.
- Embedded link adapters (serial/BLE/LoRa) must preserve this RPC method/field contract when bridged through transport runtimes.

## Cryptographic Agility Policy

Algorithm negotiation roadmap is governed by `docs/adr/0007-crypto-agility-roadmap.md`.

Versioned algorithm-set ids:

| algorithm_set_id | Status | Baseline intent |
| --- | --- | --- |
| `rns-a1` | active | current baseline interoperability profile |
| `rns-a2` | planned | strengthened signature/cipher suite profile |
| `rns-a3` | reserved | post-quantum transition profile placeholder |

Negotiation contract (additive roadmap for `sdk_negotiate_v2` extension fields):

1. Client advertises ordered `supported_algorithm_sets`.
2. Server returns one `selected_algorithm_set`.
3. Server selection must be within client-offered set.
4. If no overlap exists, negotiation fails with contract-incompatible semantics.
5. Selected algorithm set must be emitted in runtime/session metadata for auditability.

Downgrade and upgrade rules:

1. Downgrade from client-preferred set must be explicit in negotiation response.
2. Silent fallback to unknown/undeclared sets is forbidden.
3. New set ids must be additive and documented before runtime enablement.
