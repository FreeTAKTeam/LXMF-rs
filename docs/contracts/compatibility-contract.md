# LXMF-rs <-> Reticulum-rs Compatibility Contract

## Version Mapping

- The root `VERSION` identifies the project release; Cargo metadata identifies
  each package version. v0.14.0 aligns all 19 public packages, including the
  separately published `reticulum-rs-zeromq` transport. Internal packages are
  not part of that version promise.
- The current SDK contract release is `v2.6`, the schema namespace is `v2`,
  and the negotiated protocol version is `2`. v2.5 request payloads remain
  accepted. These identifiers are independent of the Cargo package version.
- Pin an exact tag or commit for reproducible integration. See the
  [v0.14.0 migration guide](../migrations/v0.14.0-zmq.md) for Rust 1.88,
  envelope construction, and optional durable-custody requirements.
- Python Reticulum compatibility is assessed against version `1.5.2` at commit
  `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`. The version is diagnostic; the
  commit remains the reproducible compatibility identity.
- Python LXMF compatibility is validated against version `0.9.6` at commit
  `727830cefda83d9c6e3982b48675425f3f988f9c`. The version is diagnostic; the
  commit remains the reproducible compatibility identity.
- SDK negotiation exposes an optional typed `software_parity` orientation, and
  daemon `status`, `daemon_status_ex`, and the typed `rns.runtime.status`
  operation expose the same structure at `reticulum.parity`.
- The structure reports active-baseline overall, Reticulum, and LXMF callable
  inventory checkpoints, plus an optional `forward_behavioral` checkpoint
  when the frozen forward contract is available. `overall` remains the RNS
  1.5.2/LXMF callable inventory and is not evidence of behavioral parity.
  Ratios are exact complete/applicable, with not-applicable entries excluded.
  The inventory generator emits and checks the grouped Rust metadata used by
  these API surfaces.
- `forward_behavioral` carries the contract coverage status, requirement
  inventory, verified-evidence count, and exact forward reference revision.
  Older payloads without this member deserialize as absent and re-serialize
  without a fabricated checkpoint. The current OpenRPC schema permits the
  optional member; clients using a strict older schema must update to the
  schema matching the negotiated contract release.
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

- Typed SDK client paths use `sdk_send_v2` and negotiated capabilities.
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
6. Pinned Python reference interoperability passes for the released commit.
7. Independent rns-rs interoperability passes every required mutually
   supported row, and Reticulum-Go passes its required supported subset with
   only explicitly capability-scoped unsupported rows.
8. The same-runner release performance workflow passes its five-sample
   dispersion and regression budgets and emits checksummed JSON, HTML, and raw
   evidence.

Reference interoperability and independent interoperability are separate
evidence axes. Python Reticulum/LXMF remains normative when an independently
implemented peer disagrees with LXMF-rs; peer divergences require explicit
Python-reference evidence and must not change core semantics merely to satisfy
the peer.
