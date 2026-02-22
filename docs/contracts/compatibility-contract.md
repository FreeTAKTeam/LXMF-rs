# LXMF-rs <-> Reticulum-rs Compatibility Contract

## Version Mapping

- Release baseline: `lxmf` `0.3.0` supports `reticulum-rs` `0.1.3`.
- Compatibility track: `lxmf` `0.3.x` supports `reticulum-rs` `0.1.x`.
- During active refactor development, integration CI may pin exact git revisions.

## Canonical Payload Policy (v0.3 clean-break)

- Public attachment key is `attachments`.
- Public `files` is rejected.
- Public numeric key `"5"` is rejected.
- Wire field id `0x05` remains internal msgpack representation.
- Attachment text data must be explicit:
  - `hex:<payload>`
  - `base64:<payload>`
- Ambiguous unprefixed text attachment data is rejected.

## Decode/Bridge Policy

- Relaxed decode environment toggles are not supported.
- Inbound decode shape is explicit at call sites:
  - `FullWire`
  - `DestinationStripped`
- Runtime and daemon decode paths share the same inbound decode core.

## RPC Policy

- Client paths use `send_message_v2` only.
- Server keeps `send_message` and `send_message_v2` for compatibility.
- Both methods are subject to the same strict canonical outbound validation path.

## Runtime/Daemon Shared Semantics

- Delivery/send outcome mapping is shared.
- Link send behavior uses common helper semantics (packet send with resource fallback path).
- Destination hash parsing is shared.
- Receipt mapping and receipt recording core behavior is shared.

## Release Gate

A release is valid only if:

1. Workspace compile, format, and clippy gates pass.
2. Runtime/daemon parity tests pass.
3. RPC contract tests pass.
4. API surface and architecture boundary checks pass.
5. Compatibility matrix and migration notes are updated.
