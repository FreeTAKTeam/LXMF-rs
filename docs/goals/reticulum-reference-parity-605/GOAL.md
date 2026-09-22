# Goal: Full Reticulum Reference Parity Software Acceptance

Use Krypton Execution to execute `docs/goals/reticulum-reference-parity-605/PLAN.md` for issue #605.

Core rules:

- Treat `PLAN.md` and the frozen #606 reference contract as the source plan and truth boundary.
- Preserve exact reference commits, ownership, cutover, and evidence lanes. Physical/platform acceptance is explicitly outside this goal and remains separately tracked.
- Extend the existing inventory, parity, `xtask`, CI, daemon, transport, and utility paths; do not create a competing status system, daemon, or protocol.
- Do not promote a callable, runtime, utility, platform, or client row without executable behavior and evidence tied to the exact candidate.
- Say `implemented but unproven`, `partial`, or `blocked/unverified` when the required evidence is missing.
- Do not use `Closes #605` from a bounded child PR. This goal can close only its software acceptance scope; it does not certify the separately tracked physical/platform gate.
