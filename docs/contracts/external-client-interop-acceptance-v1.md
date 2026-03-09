# External Client Interop Acceptance v1

## Scope

This contract defines the minimum proof required before the repository can claim
that a Rust-side runtime path interoperates with an external Reticulum/LXMF
client.

The required peers for this contract are MeshChatX and Sideband.

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

The Sideband proof must satisfy the generic criteria above with these concrete
checks:

1. Sideband runs non-interactively through `SidebandCore` in daemon mode.
2. Sideband exposes its local LXMF destination through a machine-readable state
   artifact emitted by the harness control shim.
3. Rust-side to Sideband delivery is verified through Sideband's own persisted
   message store, decoded through the control shim.
4. Sideband to Rust-side delivery is verified through the `reticulumd` SQLite
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

- Columba proof
- CI gating

Those belong to the later release-gated interop track once the external client
proof set is stable.
