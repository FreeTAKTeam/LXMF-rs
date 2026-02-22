# Protocol Extension Registry

Status: Active  
Registry version: `1`

## Purpose
This registry defines controlled extension identifiers and governance for protocol growth without destabilizing required compatibility slices.

## Namespace Rules
1. Extension IDs must use dotted namespaces: `<scope>.<domain>.<name>.v<major>`.
2. Allowed scope prefixes:
   - `rpc.`
   - `payload.`
   - `event.`
   - `domain.`
3. Additive changes within the same major extension must remain backward-compatible.
4. Breaking changes require a new major extension ID (`vN -> vN+1`) and migration notes.

## Registry Entries

| Extension ID | Scope | Status | Owner | Introduced in | Notes |
| --- | --- | --- | --- | --- | --- |
| `payload.app.reply.v1` | `payload` | active | `FreeTAKTeam` | `v2.5` | maps to app extension key `reply_to` under payload field `16` |
| `payload.app.reaction.v1` | `payload` | active | `FreeTAKTeam` | `v2.5` | maps to `reaction_to`, `emoji`, optional `sender` |
| `payload.telemetry.stream.v1` | `payload` | active | `FreeTAKTeam` | `v2.5` | telemetry stream conventions for field `2` |
| `rpc.auth.shared_instance.v1` | `rpc` | active | `FreeTAKTeam` | `v2.5` | shared-instance auth handshake behavior |
| `domain.topics.release_b.v1` | `domain` | active | `FreeTAKTeam` | `v2.5` | topic domain method family |
| `domain.attachments.release_b.v1` | `domain` | active | `FreeTAKTeam` | `v2.5` | attachment domain method family |
| `domain.voice.release_c.v1` | `domain` | active | `FreeTAKTeam` | `v2.5` | voice signaling domain method family |
| `event.stream_gap.v1` | `event` | active | `FreeTAKTeam` | `v2.5` | stream gap metadata and degraded recovery semantics |

## Change Process
1. Add or modify registry entries in this file.
2. Update affected contract docs (`rpc`, `payload`, `sdk-v2*`) and migration notes.
3. Add/adjust conformance tests before merging.
4. Pass `cargo run -p xtask -- extension-registry-check`.

## Deprecation Process
1. Mark entry status as `deprecated` with replacement extension ID.
2. Keep deprecated extension readable for at least one full support window.
3. Record removal plan in `docs/contracts/sdk-v2-migration.md`.
