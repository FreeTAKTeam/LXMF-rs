# Payload Contract (LXMF + Daemon RPC)

This is the implementation-facing payload contract for Rust LXMF (`crates/lxmf`) and daemon RPC (`reticulumd`).

Scope:
- Message payload fields (wire/paper/propagation payload body).
- Announce payload data used for peers and propagation nodes.
- Daemon RPC request/response/event payloads used by desktop clients (Tauri/UI).

Reference tests:
- `crates/lxmf/tests/constants_parity.rs`
- `crates/lxmf/tests/python_client_interop_gate.rs`
- `crates/lxmf/tests/python_client_replay_gate.rs`
- `crates/lxmf/tests/rpc_contract_methods.rs`

## Message Field IDs

Core field IDs:

| Name | Hex | Purpose |
| --- | --- | --- |
| `FIELD_EMBEDDED_LXMS` | `0x01` | Nested/embedded LXMF payloads |
| `FIELD_TELEMETRY` | `0x02` | Packed telemetry payload |
| `FIELD_TELEMETRY_STREAM` | `0x03` | Batched telemetry stream entries |
| `FIELD_ICON_APPEARANCE` | `0x04` | Icon appearance tuple |
| `FIELD_FILE_ATTACHMENTS` | `0x05` | File attachments |
| `FIELD_IMAGE` | `0x06` | Image payload tuple |
| `FIELD_AUDIO` | `0x07` | Audio payload tuple |
| `FIELD_THREAD` | `0x08` | Thread identifier |
| `FIELD_COMMANDS` | `0x09` | Command list |
| `FIELD_RESULTS` | `0x0A` | Command/result payloads |
| `FIELD_GROUP` | `0x0B` | Group identifier |
| `FIELD_TICKET` | `0x0C` | Stamp ticket |
| `FIELD_EVENT` | `0x0D` | Event payload |
| `FIELD_RNR_REFS` | `0x0E` | References (reply/relay/ref) |
| `FIELD_RENDERER` | `0x0F` | Preferred renderer |

Extension/debug field IDs:

| Name | Hex | Purpose |
| --- | --- | --- |
| `FIELD_CUSTOM_TYPE` | `0xFB` | Custom content type identifier |
| `FIELD_CUSTOM_DATA` | `0xFC` | Custom payload body |
| `FIELD_CUSTOM_META` | `0xFD` | Custom payload metadata |
| `FIELD_NON_SPECIFIC` | `0xFE` | Non-specific payload |
| `FIELD_DEBUG` | `0xFF` | Debug/development payload |

## Renderer IDs

| Name | Hex |
| --- | --- |
| `RENDERER_PLAIN` | `0x00` |
| `RENDERER_MICRON` | `0x01` |
| `RENDERER_MARKDOWN` | `0x02` |
| `RENDERER_BBCODE` | `0x03` |

## Audio Mode IDs

| Family | Values |
| --- | --- |
| Codec2 | `0x01..0x09` (`AM_CODEC2_450PWB` .. `AM_CODEC2_3200`) |
| Opus | `0x10..0x19` (`AM_OPUS_OGG` .. `AM_OPUS_LOSSLESS`) |
| Custom | `0xFF` (`AM_CUSTOM`) |

## Canonical Payload Shapes

These shapes are interoperability targets used in Python fixture gates:

1. Attachments:
`FIELD_FILE_ATTACHMENTS: [[name, data], ...]`

2. Image:
`FIELD_IMAGE: [media_type, data]`

3. Audio:
`FIELD_AUDIO: [audio_mode, data]`

4. Commands:
`FIELD_COMMANDS: [{command_id: command_payload}, ...]`

5. Telemetry stream:
`FIELD_TELEMETRY_STREAM: [[peer_hash, unix_ts, packed_payload, appearance?], ...]`

6. Paper payload representation:
- Delivered as paper-packed LXMF bytes (wire body encrypted for destination, prefixed by destination hash).
- Verified in `python_client_interop_gate` and `python_client_replay_gate`.

## Announce Payloads

### Delivery announce app-data

`display_name_from_app_data` and `stamp_cost_from_app_data` compatibility follows Python LXMF behavior:
- Legacy string app-data supported.
- v0.5.0+ msgpack list layout supported.

### Propagation node announce app-data

Validated by `pn_announce_data_is_valid` parity and fixtures.
Metadata keys:
- `PN_META_VERSION = 0x00`
- `PN_META_NAME = 0x01`
- `PN_META_SYNC_STRATUM = 0x02`
- `PN_META_SYNC_THROTTLE = 0x03`
- `PN_META_AUTH_BAND = 0x04`
- `PN_META_UTIL_PRESSURE = 0x05`
- `PN_META_CUSTOM = 0xFF`

## Daemon RPC Payload Families

### Messaging
- `list_messages` -> `{ "messages": [MessageRecord] }`
- `send_message_v2` params:
  - required: `id`, `source`, `destination`, `title`, `content`
  - optional: `fields`, `method`, `stamp_cost`, `include_ticket`
- `send_message` legacy params:
  - required: `id`, `source`, `destination`, `title`, `content`
  - optional: `fields`

`MessageRecord` includes:
- `id`, `source`, `destination`, `title`, `content`, `timestamp`, `direction`
- `fields` (optional JSON object)
- `receipt_status` (optional)

### Peers
- `list_peers` -> `{ "peers": [PeerRecord] }`
- `peer_sync` params: `peer`
- `peer_unpeer` params: `peer`
- `clear_peers`

### Interfaces
- `list_interfaces` -> `{ "interfaces": [InterfaceRecord] }`
- `set_interfaces` params: `{ "interfaces": [...] }`
- `reload_config`

### Announce/Propagation
- `announce_now`
- `propagation_status`
- `propagation_enable` params: `enabled`, `store_root`, `target_cost`
- `propagation_ingest` params: `transient_id`, `payload_hex`
- `propagation_fetch` params: `transient_id`

### Stamp/Tickets
- `stamp_policy_get`
- `stamp_policy_set` params: `target_cost`, `flexibility`
- `ticket_generate` params: `destination`, `ttl_secs`
