# JSON, LXMF Fields, and MessagePack

This note explains how application JSON moves through the current `lxmf-sdk`
and `lxmf-core` layers.

It supplements `docs/architecture/overview.md` with the data-path details that
matter when callers need predictable LXMF field encoding.

## Short Version

- `lxmf-sdk` exposes a JSON API boundary.
- LXMF wire payloads are MessagePack-based.
- The active implementation uses `rmp-serde` for MessagePack serialization and
  `rmpv::Value` as the intermediate representation for flexible LXMF fields.
- The SDK does not expose a first-class public MessagePack API.
- If you need exact LXMF field control, use `lxmf-core`.

## Layer Responsibilities

### `lxmf-sdk`

The public send API accepts `SendRequest.payload: serde_json::Value`.

In the RPC backend implementation:

- `payload["title"]` is copied into the LXMF title field if it is a string.
- `payload["content"]` is copied into the LXMF content field if it is a string.
- The full JSON payload object is also forwarded as LXMF `fields`.
- SDK metadata such as idempotency and correlation values is added under `_sdk`
  inside the forwarded fields object.

This behavior is implemented in
`crates/libs/lxmf-sdk/src/backend/rpc/core_impl.rs`.

### `lxmf-core`

`lxmf-core` owns the LXMF payload and wire encoding rules.

- `Payload` stores `content`, `title`, `fields`, and `stamp`.
- `Payload::to_msgpack()` serializes the payload tuple with `rmp_serde::to_vec`.
- `Payload::from_msgpack()` decodes with `rmp_serde::from_slice`.
- JSON field maps are converted into `rmpv::Value` before wire serialization.

This behavior is implemented in:

- `crates/libs/lxmf-core/src/message/payload.rs`
- `crates/libs/lxmf-core/src/wire_fields.rs`

## What "JSON Becomes MessagePack" Means Here

For outbound LXMF messages, the wire format is MessagePack.

The current path is:

1. Application code builds `SendRequest.payload` as JSON.
2. `lxmf-sdk` derives `title` and `content` from that JSON.
3. `lxmf-sdk` forwards the JSON object as LXMF `fields`.
4. `lxmf-core` converts JSON field values into `rmpv::Value`.
5. `lxmf-core` serializes the LXMF payload as MessagePack.

This means JSON is the host-facing representation, but MessagePack is the
transport representation.

## Important Nuance: `content` and `title`

`title` and `content` are not treated as pure field entries.

The SDK extracts them for the dedicated LXMF payload slots and also leaves the
original JSON object in `fields`. In practice, callers should assume:

- `title` may exist as both structured application data and as the dedicated
  LXMF title bytes.
- `content` may exist as both structured application data and as the dedicated
  LXMF content bytes.

If `payload["content"]` is not a string, the SDK falls back to
`payload.to_string()` for the LXMF content slot.

## Field Key Conversion

When JSON is converted into LXMF field data:

- canonical numeric JSON keys such as `"9"` are converted to integer msgpack
  keys
- non-numeric keys remain string keys

That behavior comes from `json_key_to_rmpv()` in
`crates/libs/lxmf-core/src/wire_fields.rs`.

This matters for canonical LXMF fields such as:

- commands: field `0x09`, JSON key form `"9"`
- telemetry: field `0x02`, JSON key form `"2"`
- ticket: field `0x0C`, JSON key form `"12"`

The canonical field mappings are defined in
`docs/contracts/payload-contract.md`.

## Commands Field Example

`FIELD_COMMANDS` is a real LXMF field in `lxmf-core`, not a free-form JSON
label. Its numeric field id is `0x09`.

If callers need explicit command-field control, the canonical API is:

- `payload_fields::WireFields`
- `payload_fields::CommandEntry`

These APIs live in
`crates/libs/lxmf-core/src/payload_fields.rs`.

## Maintenance Notes

- Keep repository paths relative and portable.
- If the JSON-to-wire mapping changes, update this note alongside:
  - `docs/contracts/payload-contract.md`
  - `docs/sdk/advanced-embedding.md`

## What the SDK Does Not Expose

The current `lxmf-sdk` public API does not expose:

- raw `rmp-serde` encode/decode helpers
- direct `rmpv::Value` field construction
- a stable public API for exact integer-key LXMF field maps
- a public transport helper for `_lxmf_fields_msgpack_b64`

Those concerns are currently internal to the lower-level implementation or
belong to `lxmf-core`.

## Practical Guidance

Use `lxmf-sdk` when:

- the application wants a JSON-first API
- default title/content/field mapping is acceptable
- the host only needs JSON event payloads back from the runtime

Use `lxmf-core` when:

- the caller must control exact LXMF field ids
- the caller must inject binary command payloads
- the caller must reason about the MessagePack shape on the wire
- interoperability depends on canonical integer LXMF field keys
