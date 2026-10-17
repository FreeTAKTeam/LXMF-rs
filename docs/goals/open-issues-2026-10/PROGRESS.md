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

### Item 3 metadata slice

- Added the exact `PN_META_IMPL_NAME` key. The production encoder advertises
  `reticulumd` and its compiled package version, preserving binary display names.
  The new assertion failed before the change; all nine daemon announce-name
  integration tests and eight Python-target metadata cases now pass.
- The renamed shared fixture generator also invokes the pinned Python production
  metadata encoder without starting network workers, then evaluates legacy,
  unknown and malformed variants with the reference parsers. Optional malformed
  version/implementation values remain tolerated like Python; non-map metadata
  is rejected. No unused metadata parser API was added.
- Independent review found no behavior/provenance defect; extracted existing
  encoder unit tests to keep production below 500 lines and removed its old
  exception. Actual announce exchange remains part of final interop acceptance.

### Item 3 notification recovery slice

- Reproduced an external `EventSinkBridge::publish` panic stopping the single
  notification worker: the healthy sink observed only one of four expected
  receipts. Isolated only that callback, after receipt commit and outside daemon
  locks; errors and unwinding panics increment existing metrics and log kind,
  sequence and event type. Notification side effects are not retried.
- Five sink tests and the issue-369 diagnostics scanner pass. The new persistent
  store regression proves an error, a panic, later callback success, healthy-sink
  continuation and immutable failed status despite a late delivered receipt.
  It injects network receipt results; later real SDK/ZeroMQ delivery and admission
  reservation acceptance remain pending.
- Design basis: Rust's [catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
  and [AssertUnwindSafe](https://doc.rust-lang.org/std/panic/struct.AssertUnwindSafe.html).
  The boundary protects daemon state from external notification code; it is not
  general recovery for storage faults or aborting panics. No dependency or second
  notification worker was introduced.

### Item 3 peer stamp floor slice

- Corrected the accumulated old-reference stamp floor from unconditional zero
  to `cost.saturating_sub(flexibility)`, matching target `LXMPeer.sync`.
  Filtering now precedes sync/transfer budgets; a second red regression showed
  the old relevance gate left below-floor entries pending when the sync budget
  was only 24 bytes.
- Six production management-sync scenarios cover below/at/above the floor,
  flexibility above cost, all-filtered entries with missing peering metadata,
  and transfer/sync limits. Rejected entries leave that peer's pending queue
  without deleting the shared message or marking it handled. All 118 focused
  peer-sync tests pass. This does not yet establish autonomous `/offer` network
  behavior; reference/mixed-peer acceptance remains open.
- Notification recovery checkpoint passed independent review and all-feature
  RPC Clippy. Default Rust panic hooks remain responsible for panic-hook output.
- Independent review caught explicit selections being validated against the
  unfiltered offer. A regression reproduced acceptance of a below-floor ID and
  exposed incorrect budget consumption. Build filtered candidates, validate
  the selected IDs, then change queue marks; invalid selections preserve both
  pending entries. Python checks peering readiness before its filter: the Rust
  missing-key/cost tests exercise management cleanup, not Python gate ordering.
- Independent re-review accepted the selection fix. Final focused suite and
  all-feature RPC Clippy pass; module-size and diff checks pass.
- Broader RPC library validation after all four item-3 checkpoints: 798 passed,
  zero failed, eight existing opt-in tests ignored (21.57 seconds).

### Item 3 next acceptance work

- Medium-aware path wait/debounce/retry scheduling and awaiting-path announce
  wakeups remain unimplemented. Keep existing scheduler and persistence owners,
  explicit timeout overrides and exact Python round/timing semantics.
- Verify remaining accumulated peer and propagated-envelope representation
  deltas, then add explicit target-reference profiles without replacing baseline.
- Complete real announce exchange and typed SDK/ZeroMQ delivery after callback
  failure, including recipient proof and capacity release. Local helper/receipt
  tests above do not substitute for those boundaries.
- Final matrices/roadmap, full item review, workspace/release gates and exact-head
  hosted CI/merge are pending. No item-3 PR has been opened yet.

### Item 3 scheduler preparation extraction

- Moved payload/stamp preparation and persisted-wire codecs to the scheduler's
  `bridge_delivery_preparation.rs` child. The existing parent exports retain
  their scope; scheduler queue, capacity, concurrency and shutdown ownership
  stay in place. Removed its obsolete module-size exception (463-line parent,
  283-line child).
- Independent review compared helper bodies with the preceding commit and found
  no behavior change. Three focused scheduler tests, module-size checks and
  all-feature daemon-binary Clippy pass.
- PRE confirms that replacing fixed waits is not full retry parity: the admitted
  scheduler must own attempts, prepared bytes and wakeups; durable `scheduled`
  claims must not be requeued merely because a path is pending.

### Item 3 reference timing policy checkpoint

- Added clock-independent `DeliveryTiming`/`DeliverySchedule` in the existing
  wire library. The caller owns attempt counts and terminal outcomes. Path wait,
  slow-interface debounce, destination round trip, strict deadlines and recovered
  path wakeups match 15 pinned target-router measurement scenarios (30 schedules,
  150 due checks). No network worker or reference path request runs in generation.
- Review exposed a real rounding bug in the first implementation: converting
  `50 / 4.5` seconds to nanoseconds changed the limit from Python's four attempts
  to five. A public-API fixture reproduced that failure before the fix. Preserve
  validated raw seconds for retry rounding, then convert waits to durations;
  adapters must not quantize the measurements first. Invalid measurements return
  an explicit error. The earlier missing-API compile failure was scaffolding,
  not a behavioral regression.
- Full `cargo test -p lxmf-wire --lib --tests`: 60 passed, 0 failed,
  0 ignored. All-target/all-feature wire Clippy, formatting, module-size and
  diff checks pass. Independent re-review accepted the precision fix. Existing
  size and metadata fixtures remain byte-identical after regeneration.
- Design uses explicit measurement inputs and one small scheduling value, keeping
  clocks, I/O, retry ownership and shared request history in production owners.
  Rust's [ties-to-even rounding](https://doc.rust-lang.org/std/primitive.f64.html#method.round_ties_even)
  supplies Python rounding without a new dependency.
- This is staged policy, not production scheduling completion. Generated request
  boundary cells await coalescer integration; admitted scheduler ownership,
  raw-precision transport adapters, durable claims, announce wakeups, send/receipt
  outcomes and end-to-end exhaustion/recovery remain the next acceptance work.

### Item 3 atomic admission and deferred completion

- Reproduced duplicate admission failing with `WouldBlock` while the original
  message occupied the whole count/byte budget. ID-aware admission now claims
  membership before bridge preparation; an already admitted ID is a successful
  no-op even at capacity. Eight simultaneous callers create one reservation.
- Ownership follows the reservation through enqueue, preparation and the current
  delivery task. Queue rejection and early return release task state, count,
  bytes and membership in that order. This is not yet ownership through all
  packet proofs/resource completions; that remains part of the attempt cutover.
- Four inner durable deferral writes now request scheduler completion. The
  scheduler releases task state and permits before changing `scheduled` to
  `queued`, preventing a broker reclaim from being acknowledged by a disappearing
  old owner. Requeue write failures retry; conditional storage transitions cannot
  resurrect a terminal message. Shutdown leaves failed requeues scheduled for
  the existing startup recovery path.
- Live completion futures remain bounded by queue capacity even after they
  release payload permits. Production scheduler tests exercise this bound during
  a storage fault. SQLite-trigger regressions cover real preparation failure,
  ordered release/requeue, storage recovery, terminal arrival during requeue and
  closed-queue cleanup. They do not prove network retry or SDK delivery.
- Independent review accepted the slice after correcting task drop order and a
  test that mistook the completion counter for reservation-release synchronization.
  All 65 bridge tests, the issue-369 scanner, all-feature daemon Clippy, formatting,
  module-size and diff checks pass. The corrected synchronization regression was
  rerun and passed.
- Next cutover must borrow task/prepared bytes and distinguish pre-send retry,
  packet awaiting proof, registered Resource ownership and terminal outcomes.
  A Resource dispatch error can already have queued an asynchronous failure;
  never treat every `io::Error` as safe to resend. Keep announce/deadline claims
  with this owner, and preserve raw timing precision from transport measurements.

### Item 3 Link dispatch outcome checkpoint

- A real activated Link with a closed output interface reproduced a Resource
  `ConnectionError` escaping into the caller's retry/fallback path even though
  transport had already registered the Resource and published `OutboundFailed`.
  Link helpers now distinguish packet dispatch, Resource lifecycle ownership and
  stopped delivery. Registered Resource errors log context and leave terminal
  processing to the existing consumer, with no fabricated sent/sending receipt.
- Proven dropped packets remove only their own message/hash correlation; unrelated
  entries remain. Pre-registration errors retain the error path. Removed an
  unreachable propagation-packet signal branch after the forced Resource return;
  active Resource rejection/cancellation monitors remain in place.
- The production-consumer regression exposed lost events before its first poll.
  Independent review also found recovered delivery could run before bootstrap
  called the worker. Bootstrap now subscribes before daemon construction/enabling
  the bridge and transfers that receiver to the consumer. Tests cover both immediate
  worker registration and dispatch before worker creation, with direct fallback
  enabled, one Resource failure receipt, persisted failure and cleared tracking.
- Independent re-review accepted the corrected slice. All 70 bridge tests and
  five production Resource-consumer tests pass; all-feature daemon Clippy,
  issue-369 diagnostics, formatting, diff and module-size checks pass. The new Resource and consumer regressions each failed
  behaviorally before their corresponding fix.
- This is not the complete admitted attempt lifecycle. Bounded broadcast lag can
  still lose Resource events; ownership through packet proofs, overload recovery,
  target retry integration and end-to-end target/SDK acceptance remain pending.

### Item 3 measured discovery timing checkpoint

- A paused-clock production regression reproduced the fixed 12-second identity
  cutoff on a 100-bit/s interface. The shared asynchronous resolver now uses the
  reference policy's measured path window (86 seconds there; seven-second floor
  on a normal medium). Tests assert both pending state before the deadline and
  completion at the deadline without allowing `await` to advance the test clock.
- Removed propagation lookup's preceding blocking 12-second resolver. The common
  path first checks the transport's learned identity, then requests/waits only if
  missing. Explicit timeout callers outside this delivery path remain unchanged.
- Transport exposes a neutral raw path/interface snapshot. It captures path
  selection and interface measurements with separate short locks, preserving
  raw seconds rather than rounding through durations. The daemon applies fixed
  RNS MTU 500 and the pinned operation order; known zero-bitrate next hops use
  Python's zero-RTT measurement-error fallback. No transport-to-LXMF dependency.
- Twelve additional fixtures execute pinned RNS measurement and LXMF scheduling
  methods with raw bitrates/path presence injected. Exact IEEE bits avoid a
  demonstrated one-bit decimal JSON decoding difference. Existing sizing,
  metadata and timing fixtures remain unchanged. A real signed announce-ingress
  test verifies selected versus slowest interface, offline/stopped exclusion and
  zero bitrate; this is software timing evidence, not a physical slow-link run.
- Independent review accepted arithmetic, locking and resolver cutover. Complete
  retry ownership is still pending: discovery currently waits inside the task;
  execution permits, prepared bytes, path requests, announce wakeups and terminal
  state must move into the admitted attempt loop before item 3 is complete.
- Validation: 74 bridge tests, all 60 wire library/integration tests and the
  issue-369 scanner pass. All-feature wire/transport library and daemon-binary
  Clippy, architecture/boundary/module-size checks, formatting and diff checks
  pass. These are local checkpoint results; item-3 hosted PR checks remain pending.

### Item 3 borrowed attempt boundary checkpoint

- The scheduler now borrows a retained task and prepared payload for one attempt.
  Explicit outcomes distinguish path waiting, packet proof, Resource ownership,
  local propagation storage, terminal publication, cancellation and storage deferral.
  This is still one attempt; no outcome implicitly authorizes a resend.
- Propagation fallback extends the existing durable LXMF representation only after
  its envelope commits. Failed commits return storage deferral without sending or
  installing a reusable cache. Recovery checks unchanged LXMF bytes and reuses the
  stored node, cost, ciphertext and transient ID rather than rebuilding stamps.
- A real activated Link with a closed interface triggers direct-to-propagation
  fallback. SQLite fault injection verifies commit failure leaves the durable
  LXMF unchanged, sends/stores no propagation payload and releases scheduler
  membership/count/byte reservations before requeue. After recovery, a closed stamp
  worker proves repeated borrowed attempts and restored state reuse exact bytes.
  The broker adds a wire hash to stored JSON; tests compare validated decoded wire
  representations rather than assuming the encoded input equals the storage row.
- Both independent reviews accepted this checkpoint. Before repeated attempts are
  enabled, the owner must explicitly retain the effective propagation method;
  currently a Direct task still re-enters Direct before fallback. The tests above
  prove envelope reuse, not absence of another direct attempt. Discovery waits,
  coalescing, announce wakeups and terminal ownership remain the next cutover.
- Local validation: 76 bridge tests and the issue-369 scanner pass; all-feature
  daemon Clippy, formatting, module-size, boundary and diff checks pass. No new PR,
  hosted checks, mixed-peer target acceptance or item-3 completion is claimed.

### Item 3 shared path-request coalescing checkpoint

- Delivery tasks now share scheduler-owned LXMF discovery history. Atomic claim
  checks suppress duplicate destination requests before transport I/O without
  holding the history lock across awaits. RNS routing and explicit path RPC policy
  remain separate. Suppression neither refreshes timestamps nor prunes history;
  pending reads retain debounce history. An admitted request prunes using the
  current measured window, as in the pinned router. Retention is time-window based,
  not an entry-count bound.
- A production identity-resolver regression failed with duplicate path frames and
  passes after cutover. Fifty request cells from the unchanged exact-pin fixture
  cover normal/slow boundaries, pending expiration before debounce, and an
  86-second pending window permitting another request at the 60-second debounce.
  Fractional comparisons preserve raw medium seconds instead of rounding the
  threshold through Duration; timer waits retain Duration. This does not claim
  identical wall-clock floating-point representation at every sub-nanosecond edge.
- Removed admission's eager blocking propagation-node lookup; cached/persisted
  identities remain available and unresolved discovery belongs to the admitted
  asynchronous task. Both recipient discovery and propagation-cost discovery use
  the shared history. Cost discovery uses the measured path wait instead of 12s.
- Review identified that the longer cost wait lacked cancellation checks. Fixed
  before discovery and during polling, returning Stopped. Paused-clock tests prove
  a 100-bit/s cost lookup waits 86 seconds and a persisted cancellation ends it
  within the next 250ms poll without issuing another request. Re-review accepted.
- Local validation: 81 bridge tests, the wire library/integration suites and
  issue-369 scanner pass; all-feature daemon Clippy, formatting, boundary, module
  size and diff checks pass. This is still not the full retry owner: discovery
  waits remain inside attempts/preparation, and execution-permit release, effective
  method, announce wakeups and packet/Resource terminal coordination are pending.
