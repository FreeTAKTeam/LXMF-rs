# Payload Contract v2 (LXMF + Reticulum RPC)

This file is the single contract source for desktop parity work across:

- repository root (`./`)
- `docs/contracts/payload-contract.md` (this repository)
- `crates/libs/*` and `crates/apps/*` protocol/runtime contracts
- `<external interop repo mirror>/docs/payload-contract.md`

The mirrored frontend copy is:

- `<external interop repo mirror>/docs/payload-contract.md`

## Version

- Contract version: `v2`
- Scope: desktop runtime only (Tauri embedded runtime, no sidecar)
- Compatibility slice:
  - `slice_id`: `payload_v2`
  - Matrix source: `docs/contracts/compatibility-matrix.md`
  - Extension registry: `docs/contracts/extension-registry.md`
  - Support windows: `N`, `N+1`, `N+2`

## Cryptographic Agility Metadata

Algorithm negotiation roadmap is governed by `docs/adr/0007-crypto-agility-roadmap.md`.

Payload-level contract additions (additive roadmap):

1. Message/session metadata may include `algorithm_set_id` (example: `rns-a1`).
2. If present, `algorithm_set_id` must match negotiated runtime value.
3. Unknown algorithm set ids must fail closed for signature/verification-sensitive flows.
4. New algorithm set ids must be additive and documented before interop rollout.

## Canonical Field Coverage

Required LXMF field coverage for parity:

| Domain | Field | Hex | JSON key form |
| --- | --- | --- | --- |
| telemetry | `FIELD_TELEMETRY` | `0x02` | `"2"` |
| attachments | `FIELD_FILE_ATTACHMENTS` | `0x05` | `attachments` (public), `"5"` (internal wire-json view) |
| commands | `FIELD_COMMANDS` | `0x09` | `"9"` |
| ticket | `FIELD_TICKET` | `0x0C` | `"12"` |
| refs | `FIELD_RNR_REFS` | `0x0E` | `"14"` |
| app extensions | extension map | `0x10` | `"16"` |

Notes:

- Integer LXMF keys must be preserved end-to-end via `_lxmf_fields_msgpack_b64`.
- JSON key forms are expected when fields are rendered back to JSON from msgpack.
- Public input policy is strict for attachments:
  - `attachments` accepted
  - `files` rejected
  - public `"5"` rejected

## Canonical Attachment Shape

Public payloads must use:

```json
{
  "attachments": [
    {
      "name": "example.bin",
      "data": [1, 2, 3]
    }
  ]
}
```

`data` accepts:

- byte arrays (`[0..255]`)
- `hex:<payload>`
- `base64:<payload>`

Unprefixed text payloads are rejected.

## Chunked Attachment Transfer (SDK v2.5)

RPC-backed SDK attachment streaming methods:

- `sdk_attachment_upload_start_v2`
- `sdk_attachment_upload_chunk_v2`
- `sdk_attachment_upload_commit_v2`
- `sdk_attachment_download_chunk_v2`

Rules:

1. Upload state is identified by `upload_id`; callers resume with explicit `offset`.
2. Upload chunks must be appended at the expected offset; mismatches return `SDK_RUNTIME_INVALID_CURSOR`.
3. Upload commit validates declared `total_size` and `checksum_sha256`; checksum mismatch returns `SDK_VALIDATION_CHECKSUM_MISMATCH`.
4. Download chunk responses include `offset`, `next_offset`, `done`, and `checksum_sha256` for resumable and integrity-aware readers.

## Schema Artifacts

- `docs/schemas/contract-v2/payload-envelope.schema.json`
- `docs/schemas/contract-v2/event-payload.schema.json`
- `docs/schemas/contract-v2/interop-golden-corpus.schema.json`

Golden fixtures:

- `docs/fixtures/interop/v1/golden-corpus.json`

## Message Envelope (v2)

Transport envelope key:

- `_lxmf_fields_msgpack_b64`: base64 msgpack map preserving integer field IDs.

App-extension conventions in field `16`:

- `reply_to: string`
- `reaction_to: string`
- `emoji: string`
- `sender?: string`

All additive payload extension keys must be listed in `docs/contracts/extension-registry.md`.

Telemetry location conventions in field `2`:

- `{ lat: number, lon: number, alt?: number, speed?: number, accuracy?: number }`

## Announce Contract (backend-backed)

`list_announces(limit?, before_ts?)` response:

```json
{
  "announces": [
    {
      "id": "announce-...",
      "peer": "hex32",
      "timestamp": 1770855315,
      "name": "Hub",
      "name_source": "pn_meta",
      "first_seen": 1770855300,
      "seen_count": 3,
      "app_data_hex": "hex",
      "capabilities": ["topic_broker", "telemetry_relay"],
      "rssi": -70.0,
      "snr": 10.5,
      "q": 0.91
    }
  ]
}
```

## RPC Additions (v2)

- `list_announces(limit?, before_ts?)`
- `get_outbound_propagation_node()`
- `set_outbound_propagation_node(peer?)`
- `list_propagation_nodes()`
- `message_delivery_trace(message_id)`

## Event Payload Additions (v2)

- `announce_received`
- `propagation_node_selected`
- `receipt`
- `outbound` (with method/error metadata)
- `runtime_started`
- `runtime_stopped`

`announce_received` payload includes:

- `peer`, `timestamp`, `name`, `name_source`, `first_seen`, `seen_count`
- `app_data_hex`, `capabilities`
- optional signal fields: `rssi`, `snr`, `q`

## Delivery Trace States

Persisted transition status strings include:

- `queued`
- `sending`
- `outbound_attempt: link`
- `sent: link`
- `retrying: opportunistic ...`
- `sent: opportunistic`
- `retrying: propagated relay ...`
- `sent: propagated relay`
- `delivered`
- `failed:*`

No outbound message should remain indefinitely in an ambiguous non-terminal state without subsequent retry/failure transition visibility.
