# External Client Interop Acceptance v1

## Scope

This contract defines the minimum proof required before the repository can claim
that a Rust-side runtime path interoperates with an external Reticulum/LXMF
client.

The release-gated acceptance target for `v1` is:

- MeshChatX
- Sideband
- Columba

## Normative Success Criteria

An external-client interoperability run passes only if all of the following are
true in one reproducible execution:

1. `reticulumd` starts on a real Reticulum transport interface.
2. The external client starts non-interactively against that interface.
3. The Rust-side runtime discovers the external client's LXMF destination.
4. A message sent from the Rust-side path is visible through the external
   client's own API or persisted state.
5. A reply sent from the external client is visible through the Rust-side
   runtime's own persisted state.
6. The run emits a machine-readable report with the exact destination hashes and
   artifact paths used for verification.

## MeshChatX v1 Acceptance

The MeshChatX proof must satisfy the generic criteria above with these concrete
checks:

1. MeshChatX runs headless via `uv run meshchat --headless`.
2. MeshChatX exposes its local LXMF destination through:
   - `GET /api/v1/config`
3. Rust-side to MeshChatX delivery is verified through:
   - `GET /api/v1/lxmf-messages/conversation/{daemon_hash}`
4. MeshChatX to Rust-side delivery is verified through the `reticulumd` SQLite
   message store.

## Sideband v1 Acceptance

Sideband compatibility requires:

1. Sideband starts headless through a scripted `SidebandCore` control shim.
2. Sideband exposes its local LXMF destination through the shim state artifact.
3. Rust-side to Sideband delivery is verified through Sideband's own SQLite
   message store or decoded message view.
4. Sideband to Rust-side delivery is verified through the `reticulumd` SQLite
   message store.

## Columba v1 Acceptance

Columba compatibility requires:

1. Columba starts headless through its real Python `ReticulumWrapper`.
2. Columba exposes its local LXMF destination and source identity through the
   control shim state artifact.
3. Rust-side to Columba delivery is verified through Columba's own
   `poll_received_messages()` path.
4. Columba to Rust-side delivery is verified through the `reticulumd` SQLite
   message store.

## Artifact Requirements

A passing run must retain:

- `reticulumd` log path
- external client log path
- machine-readable report path
- daemon delivery hash
- external client delivery hash
- exact message bodies used for both directions

## Non-Goals

This contract does not yet require:

- CI gating

That belongs to the later release-gated interop track once the local harnesses
are stable on repeated runs.
