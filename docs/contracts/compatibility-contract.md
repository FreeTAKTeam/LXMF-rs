# LXMF-rs <-> Reticulum-rs Compatibility Contract

## Version Mapping

- LXMF-rs project releases are recorded in the root `VERSION` file and are
  independent from Cargo package versions.
- Release baseline: LXMF-rs `0.5.0` ships `lxmf` `0.3.0` with
  `reticulum-rs` `0.2.0`.
- Compatibility track: LXMF-rs `0.5.x` keeps the `lxmf` `0.3.x` and
  `reticulum-rs` `0.2.x` package line unless release notes declare otherwise.
- During active refactor development, integration CI may pin exact git revisions.
- Python Reticulum compatibility is assessed against version `1.4.2` at commit
  `b48b96e61676504e0a4e527b33b9a0b4495c6872`. The version is diagnostic; the
  commit remains the reproducible compatibility identity.
- Python LXMF compatibility is validated against version `0.9.6` at commit
  `727830cefda83d9c6e3982b48675425f3f988f9c`. The version is diagnostic; the
  commit remains the reproducible compatibility identity.
- SDK negotiation exposes an optional typed `software_parity` orientation, and
  daemon `status`, `daemon_status_ex`, and the typed `rns.runtime.status`
  operation expose the same structure at `reticulum.parity`.
- The structure separately reports overall, Reticulum, and LXMF checkpoints.
  Each ratio is exact complete/applicable, where applicable is complete plus
  partial and not-applicable entries are excluded. The inventory generator
  emits and checks the grouped Rust metadata used by these API surfaces.
- `software_parity.advisory` is always `true`. This is consumer orientation,
  not a substitute for capability negotiation, runtime feature checks, or the
  separate hardware-evidence axis.

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
