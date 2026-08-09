# Implementation Plan

1. Keep profile and case definitions under tests/hil, with environment
   variable names—not physical IDs, secrets, or credentials—in source control.
2. Add cargo xtask hil doctor, list, run, and report commands.
3. Execute cases through typed adapters with bounded timeouts, deterministic
   seeds, one retry only for lab failures, and result classes that preserve
   protocol failures over assertion, device, lab, and blocked outcomes.
4. Serialize a run lock and make reset/power control explicit through a
   command hook or uhubctl.
5. Publish PR, nightly, lab-health, and release workflows with concurrency,
   TTL-scoped evidence, and retained artifacts.
6. Validate the virtual matrix locally, keep pinned Python references
   reproducible, and document the two-RNode live acceptance gate.
