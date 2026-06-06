# RPC Contract (`reticulumd`) - CLI Stable Set

This document freezes the daemon RPC methods that `lxmf-sdk`, `lxmf-cli`, and
other operator-facing integrations rely on over the JSON-RPC/MessagePack daemon
surface.

Scope:
- Transport: HTTP `POST /rpc` with framed MessagePack payloads over Unix sockets, TCP, or TLS/mTLS.
- Experimental transport: feature-gated ZeroMQ pipeline RPC uses the same framed MessagePack RPC
  payload bytes inside an application envelope. HTTP remains the default and authoritative transport
  until the ZeroMQ release gates below pass.
- Event stream: HTTP `GET /events` with framed MessagePack events.
- Live event stream: HTTP `GET /events/stream` keeps the connection open and writes framed SDK
  event objects until the client disconnects. Frames use the same 4-byte big-endian length prefix
  as RPC request/response bodies, followed by a MessagePack-encoded SDK event object. `?cursor=...`
  performs catch-up before live fanout. Broadcast lag is reported as a typed `StreamGap` SDK event
  instead of silently resetting the cursor. The Rust SDK uses this native stream over Unix sockets,
  plain TCP, or TLS/mTLS, and falls back to cursor polling only for recovery/manual paths.
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

Frame payloads are capped at 16 MiB. Decoders must reject larger declared
lengths before waiting for the remaining body, and HTTP `POST /rpc` bodies must
reject content lengths larger than the 4-byte prefix plus the maximum payload.
HTTP request headers are capped at 64 KiB total, 8 KiB per line, and 128 header
fields. SDK RPC HTTP response reads are bounded to the response header cap plus
one maximum framed RPC body.

Request object:
- `id: u64`
- `method: string`
- `params: object | null` (method-specific)

Response object:
- `id: u64`
- `result: object | array | scalar | null`
- `error: { code: string, message: string } | null`

## Experimental ZeroMQ pipeline transport

Status: opt-in, feature-gated, not the default SDK transport.

Crate/API pin:

- Rust crate: `zeromq = 0.6.0`
- Enabled crate features: `tokio-runtime`, `tcp-transport`
- Initial socket pattern: paired `PUSH`/`PULL` sockets
- IPC transport is deferred for the first cross-platform pass.

Envelope:

- `protocol_version: u16`, currently `1`
- `session_id: string`
- `request_id: u64`
- `kind: request | response | event | control`
- `auth: { scheme, value } | null`
- `response_endpoint: string | null`
- `payload: bytes`, containing the existing framed MessagePack RPC request or response

Correlation is mandatory. SDK clients must accept a response only when both `session_id` and
`request_id` match the pending call. ZeroMQ socket identity, round-robin delivery, and peer ordering
are not sufficient for request/reply semantics.

Security:

- Loopback TCP endpoints may run in local-trusted mode.
- Non-local TCP endpoints fail closed unless explicit application-layer auth metadata is configured.
- The first auth mode is token/HMAC metadata equivalent to the HTTP bearer token semantics.
- mTLS is not provided by ZeroMQ and must not be inferred from the socket layer.
- IPC endpoints are deferred until the optional Unix-only transport feature is introduced.

Events:

- `sdk_poll_events_v2` cursor semantics remain authoritative.
- Pushed event envelopes are a wakeup/optimization path only until they prove identical gap, cursor,
  overflow, replay, and reset behavior.

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
- `record_receipt`
: Params keys: `message_id`, `status`.
- `message_delivery_trace`
: Params keys: `message_id`.
- `get_outbound_progress`
: Params keys: `message_id` or `lxm_hash`; returns current outbound progress or `null` when the message cannot be found.
- `get_outbound_lxm_stamp_cost`
: Params keys: `message_id` or `lxm_hash`; returns the normal stamp cost, or `null` when a ticket stamp is being used.
- `get_outbound_lxm_propagation_stamp_cost`
: Params keys: `message_id` or `lxm_hash`; returns the propagation stamp target cost when known.
- `sdk_cancel_message_v2`
: Params keys: `message_id`.

### Identity / status
- `daemon_status_ex` (no params)
: Must include `identity_hash` when available.
- `status` (no params)
: Fallback status method; must include `identity_hash` when available.

### Peers and interfaces
- `list_peers` (no params)
- `peer_sync`
: Params keys: `peer` (optional: `transfer_limit_kb`, `wanted_ids`). When
  `wanted_ids` is `true`, every offered message is transferred like Python's
  "wants all" offer response. When `wanted_ids` is `false` or an empty list,
  every offered ID is treated like a Python LXMF peer response indicating the
  peer already has those messages: they become handled but are not transferred.
  A non-empty list transfers only the supplied IDs and handles the rest. Each
  supplied wanted ID must be a 32-byte transient ID encoded as 64 hex
  characters; malformed wanted IDs are rejected before peer queue state is
  mutated.
- `peer_unpeer`
: Params keys: `peer`. Result and `peer_unpeer` event include `removed`,
  `propagation_cleared`, `propagation_cleared_bytes`, top-level aggregate
  peer counters `offered`, `outgoing`, `incoming`, and `messages` with
  `offered`, `outgoing`, `incoming`, `unhandled`, byte counts, and handled /
  unhandled propagation IDs.
- `clear_peers` (no params)
- `list_interfaces` (no params)
- `set_interfaces`
: Params keys: `interfaces`
- `reload_config` (no params)

`list_interfaces` response notes:

- `interfaces[*].settings` may include a runtime metadata envelope at `_runtime` with fields:
  `startup_status`, optional `startup_error`, and optional `iface` (runtime interface id).
- Known `startup_status` values include: `disabled`, `inactive_transport_disabled`, `failed`,
  `spawned`, and `active`.
- This metadata is additive and intended for startup/degraded-mode observability.

Startup policy notes:

- `reticulumd --strict-interface-startup` makes startup/preflight interface failures fatal.
- Strict preflight currently includes `tcp_client` connect checks (2s timeout) and serial port open checks.
- TCP RPC binds on non-loopback addresses fail at startup unless a persisted SDK runtime config
  has `bind_mode=remote` with `auth_mode=token`/`mtls`, or mTLS client authentication is configured
  with `--rpc-tls-client-ca`.
- First-run remote token auth can be configured at daemon startup with `--rpc-token-issuer`,
  `--rpc-token-audience`, and `--rpc-token-secret-env`. The secret value is read from the named
  environment variable, not from argv.

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
- `propagation_remote_sync`
: Params keys: `remote`, `peer` (optional: `identity_private_key_hex`, `timeout_secs`).
  `remote` is trimmed and must not be blank; invalid remotes are rejected
  before the bridge is called or local peer/sync state is updated.
  `propagation_status.propagation.sync_state` uses Python `LXMRouter.PR_*`
  values for remote sync lifecycle: request sent `0x04`, complete `0x07`,
  failed `0xfe`.
- `propagation_acknowledge_sync_completion`
: Optional params keys: `reset_state`, `failure_state`. Mirrors Python
  `acknowledge_sync_completion`: clears progress, resets completed states to
  idle, and preserves failure states unless `reset_state` is true.
- `propagation_remote_unpeer`
: Mirrors local `peer_unpeer` cleanup accounting for the local peer state after
  the remote unpeer call succeeds, and includes the remote bridge `result`.
: Params keys: `remote`, `peer` (optional: `identity_private_key_hex`, `timeout_secs`).
  `remote` is trimmed and must not be blank; invalid remotes are rejected
  before the bridge is called or local peer state is removed.

### Stamp / tickets
- `stamp_policy_get` (no params)
- `stamp_policy_set`
: Params keys: `target_cost`, `flexibility`, `enforce`
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
