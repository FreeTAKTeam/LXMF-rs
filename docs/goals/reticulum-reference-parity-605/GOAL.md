# Goal: Full Reticulum Reference Parity Software Acceptance

Use Krypton Execution to execute `docs/goals/reticulum-reference-parity-605/PLAN.md` for issue #605.

Core rules:

- Treat `PLAN.md` and the frozen #606 reference contract as the source plan and truth boundary.
- Keep the public parity advisory's 1.5.2 callable fields intact and separately expose an optional `forward_behavioral` status additively, with exact target revision and applicable/verified counts; older advisory payloads must remain deserializable and re-serialize without inventing a status. Keep the OpenRPC schema and compatibility fixtures aligned, and do not claim an old strict schema accepts a new property.
- Preserve exact reference commits, ownership, cutover, and evidence lanes. Physical/platform acceptance is explicitly outside this goal and remains separately tracked.
- Extend the existing inventory, parity, `xtask`, CI, daemon, transport, and utility paths; do not create a competing status system, daemon, or protocol.
- Do not promote a callable, runtime, utility, platform, or client row without executable behavior and evidence tied to the exact candidate.
- Say `implemented but unproven`, `partial`, or `blocked/unverified` when the required evidence is missing.
- Do not use `Closes #605` from a bounded child PR. This goal can close only its software acceptance scope; it does not certify the separately tracked physical/platform gate.
