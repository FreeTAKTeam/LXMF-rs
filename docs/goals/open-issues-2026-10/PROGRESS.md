# Open-issue execution progress

Started 2026-10-17 from main `a8e500e88610210582e8dd8869df8b510e1e75fc`.
User scope: [PLAN.md](PLAN.md) items 1–6, one reviewed, CI-passing merged PR each.

| Item | State | PR / exact head / merge | Evidence |
| --- | --- | --- | --- |
| 1. Acceptance reconciliation | In progress | Not opened | Source/GitHub read-only assessment; no behavior test claims. |
| 2. Automatic CI and protection | Pending | — | — |
| 3. LXMF 1.2.0 | Pending | — | — |
| 4. Transport and RNS 1.5.5 | Pending | — | — |
| 5. Maintenance fixes | Pending | — | — |
| 6. Utilities and rngit | Pending | — | — |

## Item 1 evidence

- Live open issue query returned 22 issues; live open PR query returned none.
- Live main and the isolated implementation checkout match the recorded base.
- GitHub branch protection returned `Branch not protected` (404); rulesets `[]`.
- Repository Actions runner query returned `total_count: 0`. This is not proof
  about equipment inventory or every possible organization runner arrangement.
- The roadmap, parity matrices, exact-reference manifest, issue discussions,
  existing implementation and historical evidence were inspected. Their test
  claims are inherited evidence, not tests rerun for this documentation PR.
- PRE review of the assessment identified safeguards incorporated in PLAN.md:
  predeclared recovery runs, bounded Linux BLE scope outside items 1–6, preserved
  Benshi defaults, remote-management negative authorization, and separate real
  converter/remote-exit proof.

- Independent PRE fidelity, correctness/maintainability and POST review found no
  blocker or major finding. The reviewer independently matched all 22 live issues
  and the checkbox-based requirement counts (excluding #244's example templates).
- Local validation passed: all four package documents' relative links resolve;
  all 22 issues and 173 numbered criterion groups are present without gaps or
  duplicates; `git diff --cached --check` passed. No runtime tests were run for
  this documentation-only change.

Hosted checks and merge remain pending. Record exact terminal results in the
next item after integration so the documentation does not claim its own merge
or repeatedly invalidate the CI head being accepted.
