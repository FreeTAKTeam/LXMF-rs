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
- external client source checkout path
- external client git revision metadata when the checkout is a Git repository
- generated client config/state artifact paths
- daemon delivery hash
- external client delivery hash
- exact message bodies used for both directions

## Release Gate Command

The release gate runs one concrete external-client proof through:

```bash
tools/scripts/external-client-interop-gate.sh <meshchatx|sideband|columba> [client-root]
```

The gate does not fetch external clients. The selected client must already be
available as a local checkout, either through the optional `[client-root]`
argument or the matching environment variable:

- `MESHCHATX_ROOT`
- `SIDEBAND_ROOT`
- `COLUMBA_ROOT`

The default local paths are `../MeshChatX`, `../Sideband`, and `../columba`.

The command delegates to the matching client-specific smoke harness, validates
that the machine-readable report contains the required proof fields and artifact
paths, and writes:

- `target/interop/external-client-gate/<client>/report.json`
- `target/interop/external-client-gate/<client>/gate-summary.json`

The gate summary records the external client checkout path, Git revision
metadata when available, the generated client config/state artifacts used by the
proof, logs, destination hashes, and the client-specific proof report path.

Release notes may only claim external-client interoperability for a client whose
gate summary has `status: "pass"` for the release candidate.

## Ownership And Failure Handling

The release owner is responsible for running the gate for each external client
named in release notes and retaining the generated `gate-summary.json` with the
release candidate evidence.

If the gate fails, the release owner must classify the failure before claiming
or removing interoperability support:

1. Rust-side regression in this repository.
2. External client setup or dependency drift.
3. External client behavior change.
4. Harness flake or environment failure.

The classification and the relevant report/log paths must be recorded in the
release notes or a follow-up issue. Client-specific harness fixes belong with the
matching smoke script and runbook in this repository; external client behavior
changes should be tracked against the upstream client checkout and pinned in the
next passing gate summary.

## Non-Goals

This contract does not yet require:

- CI gating

The release gate is intentionally local/manual until external client checkouts
and credentials are available in automation. A future CI version must provide
those checkouts explicitly, for example through a pinned external interop
workspace, private checkout token, or prebuilt client image.
