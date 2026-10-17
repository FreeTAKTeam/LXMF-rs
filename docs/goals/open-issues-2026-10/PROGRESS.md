# Open-issue execution progress

Started 2026-10-17 from main `a8e500e88610210582e8dd8869df8b510e1e75fc`.
User scope: [PLAN.md](PLAN.md) items 1–6, one reviewed, CI-passing merged PR each.

| Item | State | PR / exact head / merge | Evidence |
| --- | --- | --- | --- |
| 1. Acceptance reconciliation | Merged | [#671](https://github.com/FreeTAKTeam/LXMF-rs/pull/671); head `5358bf7399c3921182580ef8a4bd8a01ea6ed665`; merge `a0a72d9b9f2d53d62b9e6d2b0f61614410dc8004` | Independent review passed; 15 hosted checks passed, 2 deliberately inapplicable jobs skipped. |
| 2. Automatic CI and protection | Merged | [#672](https://github.com/FreeTAKTeam/LXMF-rs/pull/672); head `18816fa6f71c1e57e58465c8e8e6379a40767a20`; merge `23a96a4a13bba3237c93c66f8d45443cc3d119ad` | Independent review and native blocked/restored proof passed; 20 hosted checks passed, 1 deliberate skip. |
| 3. LXMF 1.2.0 | In progress | Branch `corvo/open-issues-03-lxmf-120` | PRE review complete; test-first implementation begins from the verified item 2 merge. |
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

Hosted CI [39064126234](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/39064126234),
Verify [39064126187](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/39064126187),
and independent interoperability [39064126272](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/39064126272)
passed on the recorded head. The label-only peer job and PR-inapplicable release
publication job were deliberately skipped. No review threads were outstanding.
GitHub confirmed the squash merge at 2026-10-17T20:22:59Z; fetched main contains it.

## Item 2 preparation and baseline

- PRE review approved existing-workflow selection, a unique `CI required` gate,
  and separate always-triggered Verify/independent required contexts. Native
  enforcement proof follows settings application/readback and precedes merge.
- Cargo metadata reports 21 workspace packages and 91 integration targets.
  Include all dependency kinds and aliases; explicitly broaden shared fixtures
  and vendored dependencies. Preserve feature-specific mobile validation.
- Existing security stage fails on four new rustls 0.23.45 advisories:
  RUSTSEC-2026-0346 through RUSTSEC-2026-0349. All specify 0.23.46 as patched.
  Existing exception policy remains authoritative; update the dependency.
- Unbounded `cargo test --workspace --tests` hit eight SDK unit failures from
  process-wide ZeroMQ connection exhaustion with 22 concurrent test threads.
  The same 255 SDK tests passed at four threads. The full workspace command
  with `-- --test-threads=4` passed: 3,669 passed, 288 explicitly ignored,
  134 test binaries. Preserve production limits and test workload assertions.
  Logs are `/tmp/lxmf-open-issues-integration-baseline.log`,
  `/tmp/lxmf-sdk-bounded-baseline.log`, and `/tmp/lxmf-open-issues-tests-bounded.log`.
- Live Actions checks report app ID 15368 (`github-actions`). Native effective
  main rules and repository/inherited rulesets were empty at preparation time;
  recheck immediately before applying protection.

## Item 2 implementation validation

- Added automatic shared security policy, Cargo-dependent integration selection,
  fail-closed `CI required`, and conflict-preserving native policy tooling.
- Updated only rustls 0.23.45 to 0.23.46 in the lockfile. The existing security
  stage now passes without changing exceptions.
- 25 CI-selection/gate/workflow tests and 11 native-protection tests pass.
  Mutation tests cover successful readback, concurrently appearing protection,
  and mismatching readback. `git diff --check` and actionlint 1.7.12 pass
  (actionlint's optional shellcheck/pyflakes integrations were not run).
- The actual broad selected integration runner passed all 91 target binaries:
  548 tests passed and 185 existing opt-in tests were explicitly ignored.
  Existing lib/bin nextest and feature-specific mobile jobs remain separate.
- Cross-package LXMD tests retain their existing explicit daemon build helper;
  selected execution does not depend on an earlier workspace build.
- Independent correctness, maintainability and POST plan review passed after
  fixing shared xtask selection, executable-document classification and broad
  integration validation. No actionable finding remains in the implementation.
- `RUST_TEST_THREADS=4 cargo xtask release-check` is running. Hosted checks,
  native apply/readback, blocked-check proof and restored eligibility remain
  pending; no live protection setting has been changed yet.

### First hosted run and release-check limitation

- PR [#672](https://github.com/FreeTAKTeam/LXMF-rs/pull/672), initial head
  `2a8c78456f728c2cee7891e547eb9c3645673c26`, started security and all applicable
  integration/contract jobs without labels. Its selection artifact contains all
  21 packages and 91 integration targets for the shared workflow/lock inputs.
- Hosted peer lifecycle validation exposed stale artifact hashes for
  `compatibility-contract.md` and `sdk-v2.md`, both changed by merged #666.
  Regenerated the existing manifest: exactly two byte-count/hash entries change;
  no contract content or check changed. Independent review verified both against
  #666 and recomputed the hashes. Interop artifacts, schema clients and SDK API
  checks pass locally after correction.
- Full local release-check passed formatting, all-feature Clippy, workspace
  check, nextest (3,120 passed; 103 opt-in skips), workspace tests and SDK schema,
  then failed the existing reticulumd publication dry run. The published
  `reticulum-rs-zeromq` 0.14.0 requires UUID 1.23.3; the unchanged local vendor and
  workspace lock require 1.26.1, also required by btleplug 0.13.5. Both local pins
  already exist in base `a0a72d9b`; the downloaded registry crate confirms the old
  pin. This is a pre-existing registry/package mismatch, not caused by rustls or
  CI selection. Later release-check stages were not reached. Do not claim a
  green release-check, downgrade incompatible BLE dependencies, change release
  versions or publish a new crate within this CI task.

### Native policy setup

- All three workflows passed on `c857253e893a92105dabf1f8bf63227d76dfffbf`:
  CI `39067993422`, independent interoperability `39067993289`, Verify
  `39067993388`. CI took 4m42s; the existing Verify suite remains substantially
  longer than the approximate 15-minute target.
- Explicit native setup succeeded after those passes. GitHub readback shows
  strict, app-bound checks, admin enforcement, zero required approvals and
  force-push/deletion disabled. The helper initially misclassified that response:
  GitHub omits `restrictions` when the submitted value is null. Normalize that
  one known API representation; unknown controls still conflict. All 12 policy
  tests, live read-only check and idempotent apply pass; independent review found
  no weakening. No second PUT was necessary.
- The normalization correction requires fresh exact-head hosted checks. A
  controlled CI cancellation will establish native blocking on that head;
  successful restoration and merge eligibility remain pending.

### Item 2 final evidence and merge

- Verify `39070636087` and independent interoperability `39070636070` passed on
  final head `18816fa6`. With only the deliberately cancelled CI gate failing,
  GitHub reported `BLOCKED`; the native-policy check remained `correct`.
- CI `39070636058` attempt 2 then passed on the same head. All three required
  contexts were successful and bound to Actions app 15368; GitHub reported
  `CLEAN`/mergeable. Final checks: 20 success, one deliberately inapplicable
  publication skip. No review threads were outstanding.
- Independent verification accepted the native failure/restoration proof.
  Actual fork and docs-only trigger runs were not exercised; local selector
  regressions cover those rules. The release-check registry limitation above
  remains unresolved and is not a claim of release readiness.
- GitHub confirmed the normal squash merge (no admin bypass) at
  2026-10-17T22:01:21Z. Fetched main contains the recorded merge commit.

## Item 3 PRE review and execution slices

- Keep exact baseline LXMF `727830cefda83d9c6e3982b48675425f3f988f9c`
  and target `c3ff2d6dc2f256daab896dadc044dd5a913ecbb7` as separate verified
  profiles. Existing six target smoke cases prove reception only; improve actual
  opportunistic-mode/no-fallback and SDK/ZeroMQ evidence on the final change.
- Size: distinguish full wire length, serialized content budget and propagated
  envelope. Test 398/399/400 wire-byte boundaries with title/fields/stamp overhead
  across core, runtime and daemon paths before correcting the shared formula.
- Metadata: emit truthful Rust name/version with existing parser/encoder owners;
  test old, new, unknown and malformed metadata plus actual announce exchange.
- Timing: use shared pure policy with existing delivery/scheduler state owners;
  cover slow/normal media, path provenance, request debounce, retry limits,
  awaiting-path-only wakeups, selected propagation node and durable dispatch.
- Peer deltas: correct the inherited zero stamp floor to saturating subtraction;
  cover empty offers and missing-path backoff. Distinguish management sync RPC
  from autonomous peer offer behavior when documenting evidence and gaps.
- Notification recovery: `EventSinkBridge::publish` is the supported Rust
  external notification boundary. Receipt state commits first; isolate sink
  failure/panic and prove later notifications/delivery without masking persistence
  failures. Preserve repeated `updated:false` events while asserting one committed
  terminal result. Bridge admission panic is a different uncertainty boundary;
  do not add speculative retry or exactly-once network-delivery guarantees.
- Independent PRE review identified these owner/evidence constraints. Runtime
  implementation remains with the main agent, sequentially within this item.

### Item 3 serialized-size slice

- Added 18 exact-target Python fixtures at wire lengths 398/399/400 and
  430/431/432, covering empty fields, title/fields and injected stamp overhead.
  The generator checks both immutable reference revisions. Injected stamps test
  serialization only, not proof-of-work validity.
- Core regression failed at 400 bytes before the MDU correction. Runtime
  regression failed because a fitting message incorrectly attempted a Link.
  Removed the extra timestamp and converted full wire length to the selector's
  serialized-content budget. The daemon now checks the same encrypted budget
  before encryption instead of relying on ciphertext overflow for fallback.
- Core fixture regression, all 16 runtime tests and the daemon's 18-case
  production-task boundary regression pass. Fitting packets decrypt to the
  expected bytes with no Link fallback; oversized cases initiate a Link. The
  daemon boundary test stops after that request; full mixed-peer completion and
  SDK/ZeroMQ acceptance remain pending for this item.
- Independent review accepted the size slice after tightening fixture provenance: clean exact-pin regeneration is byte-identical; dirty references and foreign module imports are rejected before output. No remaining checkpoint findings.
