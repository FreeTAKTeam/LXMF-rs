# Payload Contract v2 (LXMF + Reticulum RPC)

This file is the single contract source for desktop parity work across:

- `/Users/tommy/Documents/TAK/LXMF-rs`
- `/Users/tommy/Documents/TAK/Reticulum-rs`
- `/Users/tommy/Documents/TAK/Weft-Web`

The mirrored frontend copy is:

- `/Users/tommy/Documents/TAK/Weft-Web/docs/payload-contract.md`

## Version

- Contract version: `v2`
- Scope: desktop runtime only (Tauri embedded runtime, no sidecar)

## Canonical Field Coverage

Required LXMF field coverage for parity:

| Domain | Field | Hex | JSON key form |
| --- | --- | --- | --- |
| telemetry | `FIELD_TELEMETRY` | `0x02` | `"2"` |
| attachments | `FIELD_FILE_ATTACHMENTS` | `0x05` | `"5"` |
| commands | `FIELD_COMMANDS` | `0x09` | `"9"` |
| ticket | `FIELD_TICKET` | `0x0C` | `"12"` |
| refs | `FIELD_RNR_REFS` | `0x0E` | `"14"` |
| app extensions | extension map | `0x10` | `"16"` |

Notes:

- Integer LXMF keys must be preserved end-to-end via `_lxmf_fields_msgpack_b64`.
- JSON key forms are expected when fields are rendered back to JSON from msgpack.

## Schema Artifacts

- `/Users/tommy/Documents/TAK/LXMF-rs/docs/schemas/contract-v2/payload-envelope.schema.json`
- `/Users/tommy/Documents/TAK/LXMF-rs/docs/schemas/contract-v2/event-payload.schema.json`

## Message Envelope (v2)

Transport envelope key:

- `_lxmf_fields_msgpack_b64`: base64 msgpack map preserving integer field IDs.

App-extension conventions in field `16`:

- `reply_to: string`
- `reaction_to: string`
- `emoji: string`
- `sender?: string`

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
