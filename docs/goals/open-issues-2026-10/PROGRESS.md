# Open-issue execution progress

Started 2026-10-17 from main `a8e500e88610210582e8dd8869df8b510e1e75fc`.
User scope: [PLAN.md](PLAN.md) items 1–6, one reviewed, CI-passing merged PR each.

| Item | State | PR / exact head / merge | Evidence |
| --- | --- | --- | --- |
| 1. Acceptance reconciliation | Merged | [#671](https://github.com/FreeTAKTeam/LXMF-rs/pull/671); head `5358bf7399c3921182580ef8a4bd8a01ea6ed665`; merge `a0a72d9b9f2d53d62b9e6d2b0f61614410dc8004` | Independent review passed; 15 hosted checks passed, 2 deliberately inapplicable jobs skipped. |
| 2. Automatic CI and protection | Merged | [#672](https://github.com/FreeTAKTeam/LXMF-rs/pull/672); head `18816fa6f71c1e57e58465c8e8e6379a40767a20`; merge `23a96a4a13bba3237c93c66f8d45443cc3d119ad` | Independent review and native blocked/restored proof passed; 20 hosted checks passed, 1 deliberate skip. |
| 3. LXMF 1.2.0 | Draft PR; in progress | [#673](https://github.com/FreeTAKTeam/LXMF-rs/pull/673); published checkpoint `57f3efb46c3a17dc1e9099d9a7a649693dce15da` | Integrated owner/adapter passes locally. Canonical SDK recovery now passes locally; target/baseline mixed-peer qualification is ongoing. Hosted quality found a new Clippy lint in one test, corrected locally; fresh exact-head checks remain required. No merge. |
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

### Item 3 retained identity-discovery owner checkpoint

- Split base LXMF preparation/commit from propagation-envelope preparation. An
  admitted task retains its prepared bytes, duplicate-id membership, count and
  byte reservations while an identity is missing. Missing identity now returns
  an explicit wait outcome; it no longer terminates/requeues and rebuilds the
  message. Execution permits drop before waiting, while stamp permits cover
  stamp generation only. Propagation cost discovery also precedes execution.
- Added per-owner target notification. Actual announce RPCs notify before legacy
  selected-node and durable filtering, and skip redispatch/status mutation for
  bridge-owned records. The owner registers its target before checking identity
  readiness; notification permits cover the check/wait race. Legacy unowned
  delivery remains covered by existing announce tests and an explicit control.
- Direct fallback now returns an explicit method transition. The owner switches
  once to Propagated; committed propagation state fixes that method on restore.
  Envelope publication remains after its durable commit, preserving exact bytes.
- A real signed announce-ingress regression captures one recipient packet despite
  a simultaneous deadline and duplicate notifications. It also proves another
  peer sends while the first retains its reservations and unchanged committed
  LXMF bytes. This is packet capture, not recipient-confirmed mixed-peer delivery.
- Review found an extra initial Direct/Propagated discovery attempt and shutdown
  missing from normal blocking stamp work. Fixed both: normal/slow discovery
  consumes exactly four/two request windows before terminal exhaustion; shutdown
  reaches the blocking callback, which is joined, and stamp semaphore waiters
  observe shutdown/cancellation without losing their place during normal waits.
  Running cost-255 PoW, unavailable stamp capacity, discovery shutdown and persisted
  cancellation racing notification all release reservations without requeue/send.
- Correctness and POST/maintainability re-reviews accepted this incremental
  owner slice. A production fallback-to-node-discovery-resume regression remains
  a useful follow-up beyond the existing explicit transition/storage tests. Local validation:
  all 85 bridge tests passed before the final added cancellation regression; the
  five owner integration tests then passed including that regression. All 42
  announce RPC tests, issue-369 scanner, all-feature daemon-binary/RPC-library
  Clippy, architecture/boundary/module-size checks, formatting and diff checks
  pass. Logs: `/tmp/lxmf-item3-owner-{bridge,integration,rpc-all,quality,clippy,rpc-clippy,architecture}.log`.
- Remaining item-3 boundaries are explicit: full post-send and Link-activation
  retry ownership, terminal persistence/publication fencing, Resource broadcast
  overload recovery, target/baseline mixed-peer traces and later successful SDK
  delivery after callback failure. Missing-identity waiting is now owned; this
  checkpoint does not claim the complete LXMF 1.2.0 lifecycle or hosted CI.

### Item 3 accepted-terminal persistence fence checkpoint

- PRE mapping rejected a blanket post-handoff terminal wait: the pinned router
  removes Propagated work at SENT, while this repository correctly distinguishes
  sent from delivered; packet receipt timeout/retry ownership is also unfinished.
  This smaller slice fences only a terminal receipt already accepted by the
  existing persistence worker, without changing those handoff/status semantics.
- ReceiptPublisher now exposes pending accepted terminal work and completion
  notifications. Unused reserved permits do not wait. Successful unreserved
  legacy terminal enqueue registers under the same map lock; rejected enqueue
  creates no pending entry. Duplicate callbacks share the existing worker.
- A common scheduler epilogue runs before task/membership/count/byte guards drop.
  It subscribes before checking pending state, covering commit/query/wait races.
  It performs no receipt-status read and creates no competing terminal event.
  Fatal persistence failure retains the fence until daemon shutdown; shutdown
  releases scheduler capacity but preserves the pending worker event for drain.
- A real SQLite receipt-write fault holds ownership and rejects duplicate admission
  while terminal work is queued. Recovering storage commits exactly one journal
  receipt then releases reservations. A separate shutdown variant preserves the
  scheduled broker intent and pending publication until the worker drains.
  Removing the epilogue call reproduced premature completion at the intended
  assertion (exit 101); restoring it passes. Publisher tests cover both reserved
  and legacy work, queue refusal, duplicates and completion races.
- Correctness and POST/maintainability reviews accepted the fence. Local validation: 87 bridge tests,
  12 receipt-related binary tests, four receipt publisher/library tests, four
  receipt integration tests and the issue-369 scanner pass. All-feature daemon
  library/binary Clippy, formatting, boundary, module-size and diff checks pass.
  Logs: `/tmp/lxmf-item3-terminal-{red,bridge,publisher,receipt-worker,quality,clippy}.log`.
- This does not retain all packet/Resource handoffs through delivery, implement
  reference post-send retry policy, or recover lost Resource broadcasts. Those
  require their explicit lifecycle cutover and target mixed-peer evidence before
  the single item-3 PR is ready. No hosted check or item completion is claimed.

### Item 3 opportunistic proof-registration prerequisite

- A real sender/recipient regression exposed a pre-existing prepared-send gap:
  the daemon registered its message mapping but bypassed transport PacketCache
  proof context, and its cached recipient identity was unavailable to validation.
  Before the fix, the actual recipient proof could not commit delivered (exit
  101, `/tmp/lxmf-item3-proof-red.log`); after it, the same regression passes.
- Added one validated SINGLE prepared-send API. It recomputes the identity hash
  from public/verifying keys, checks the named destination and packet class,
  rejects known identity drift, then registers the absent identity without a
  route. The observer installs message correlation before the authoritative send
  helper records proof context and hands off unchanged ciphertext. No second
  transport encryption or proof-validation path was added.
- Security tests reject forged cached hashes, destination mismatch, conflicting
  known identities, wrong known signers and untracked hashes; both implicit and
  explicit proofs remain destination-bound. The daemon test uses real RxMessage
  ingress, receiver decryption/proof, ReceiptBridge and the durable receipt worker;
  it checks exact LXMF bytes, persisted delivered and correlation cleanup.
- Review found an unintended generic public-API behavior expansion for Announce
  and Link packets. Restored that original method unchanged; only the new
  validated SINGLE API uses the authoritative send helper. Correctness and
  maintainability/POST reviews then accepted the scoped change. Extracted the
  new proof test to respect the existing module-size gate.
- Validation: 88 bridge tests, 294 transport tests (seven existing ignored), the
  issue-369 scanner and the extracted real-recipient regression pass. Boundary,
  module-size, formatting and diff checks pass. All-feature daemon and transport
  library/binary Clippy is recorded in `/tmp/lxmf-item3-proof-final-clippy.log`.
- The test completes the current one-attempt scheduler before returning the
  proof. Full retained post-send retry ownership and mixed Python/Rust traces
  remain pending; no item completion or hosted CI is claimed.

### Item 3 production-reference retry trace checkpoint

- Extended the existing exact-clean-pin generator with ten scenarios and 66
  snapshots from real `handle_outbound`, `process_outbound`, `LXMessage.send` and
  packet construction. Clock, transport measurements, packet-send effects and
  thread execution are controlled; this is reference policy evidence, not network
  delivery or a passing Rust retry implementation.
- Covers strict deadline equality, initial/pathless discovery and exhaustion,
  two sends followed by forced rediscovery, pending-request suppression,
  pending/debounce windows in both orders, a path appearing after a pathless
  send, preservation/change of attempt limits, no-receipt versus raising sends,
  late delivery and cancellation during delayed rediscovery. Actual reference
  delayed closures run at their controlled deadline rather than replacing their
  behavior with a hand-written request decision.
- The reference still dispatches its detached delayed request after delivery or
  cancellation. Preserve this fact in the artifact; Rust's owned cancellation
  must be an explicit stronger lifecycle guarantee rather than a claimed exact
  match. Snapshot times/deadlines/request timestamps include IEEE bits to avoid
  JSON parser rounding being mistaken for policy behavior. Rust timer conversion
  and strict-boundary comparisons still need explicit treatment in integration.
- Review caught that assertions inside the send seam would be swallowed by the
  real router's broad send-error handler. Byte invariants now run after the
  production call. An injected byte mismatch fails generation at the intended
  assertion (`/tmp/lxmf-item3-retry-generator-red.log`); restored generation passes
  and reproduces SHA-256
  `4c2ed93512cd771aa64268d60f7c343a0d7868af189f212f678871813ab29e10`.
  Review accepted the corrected fixture; existing generated fixtures are unchanged.
- Next: consume these traces in the existing admitted owner, with a distinct
  opportunistic packet outcome, retryable no-route dispatch, preserved counters
  and prepared bytes, forced rediscovery's separate atomic pending claim and
  owned 500ms delay, and terminal observation before another send. Link fallback
  must not enter the opportunistic resend loop. The current one-attempt handoff
  remains unchanged by this test-artifact checkpoint.

### Item 3 retained opportunistic retry owner

- Integrated the production-reference policy into the existing admitted owner for
  fitting opportunistic packets. One owner retains prepared signed LXMF bytes,
  membership and count/byte reservations through sends, no-route dispatch,
  discovery, rediscovery and terminal persistence. Each attempt encrypts afresh;
  execution permits cover actual sends, and oversized Link/Resource fallback
  stays outside this packet retry loop. Ten reference scenarios and 66 snapshots
  check decisions, counters, deadlines, request windows and attempt limits.
- The atomic forced-rediscovery claim uses pending lifetime without refreshing
  suppressed timestamps or pruning another destination's history. Its 500ms
  request belongs to the same loop, runs even while send capacity is blocked, and
  is cancelled on terminal observation/shutdown. This intentionally differs from
  the reference's detached request surviving delivery/cancellation. Early announce
  resumes the owner without duplicate sends or resetting consumed attempts.
- Real Rust sender/recipient ingress now proves retained ownership until delivered,
  including delivery by the first packet's proof after a second attempt. Both
  correlations survive until terminal cleanup; a real receipt-write fault retains
  admission until recovery commits exactly one terminal result. No-route dispatch
  remains pending until bounded reference exhaustion. Status-read faults pause
  retry without a competing terminal and resume after storage recovery.
- Correctness review found two races: a forced request could wait behind blocked
  execution capacity, and terminal acceptance could arrive while transport locks
  were awaited. Fixed both. The validated SINGLE prepared-send API has an optional
  final guard after handler/cache/interface locks; rejected handoff produces no
  message correlation, proof-cache entry or frame. It reuses InterfaceManager
  dispatch and leaves the generic prepared API unchanged. The guarantee applies to
  SINGLE broadcast handoff, not unrelated direct-send paths or remote delivery.
- POST review found oversized AwaitStatus released its durable scheduled owner,
  and path-request deadlines could use measurements/time from before an awaited
  handoff. Oversized ownership now polls nonmutating status and retains admission;
  the Link fallback entry also uses that check. Initial and ordinary discovery
  remeasure/re-anchor after handoff, and forced rediscovery does so after expiry
  while preserving the preceding attempt limit. These changes do not claim shared
  Link-stage status handling is fully converted.
- Regression evidence uses real SQLite column-read faults and staged FIFO
  transport mutex waits with a frozen clock. Initial and post-send discovery both
  retain the full fresh 86-second window after a delayed request, remain idle at
  equality, and resume after it. Counterfactual removal of each oversized-owner,
  initial-timing and ordinary-timing fix fails at its intended assertion. The
  ordinary-request regression checks the final allowed request too: debounce can
  hide an early request frame while its consumed attempt causes early exhaustion. Test-only
  transport maintenance is allowed to finish at the frozen handoff time before
  advancing the clock; no production scheduling hook or timer tolerance was added.
- Full Link/Resource post-send ownership, Link activation waits, overload recovery,
  successful SDK/ZeroMQ delivery after callback failure, and target/baseline mixed
  Python/Rust traces remain pending. This is a local checkpoint toward the single
  item-3 PR, not issue completion, hosted CI or a release claim.
- Validation: 99 daemon bridge tests, four guarded prepared-SINGLE transport
  regressions, three timing tests, the issue-369 scanner, all-feature daemon and
  transport library/binary Clippy, formatting, boundaries and architecture checks
  pass. Extracted delayed-handoff tests into their own module to satisfy the
  actual 500-line gate applied to these binary test files. Final bridge/size/diff
  checks pass after that extraction. Logs:
  `/tmp/lxmf-item3-retry-final-{bridge,clippy,quality,architecture,size}.log`,
  `/tmp/lxmf-item3-retry-guard.log`, `/tmp/lxmf-item3-retry-timing.log`, and
  `/tmp/lxmf-item3-retry-{oversized,handoff,repeat-handoff}-red.log`.
- Independent correctness and POST/maintainability reviews accept this scoped
  checkpoint and the explicit uncompleted acceptance boundaries. No remaining
  checkpoint blocker was found; neither review claims full item-3 acceptance.


### Item 3 production-reference Link and Resource traces

- Added 42 scenarios and 226 snapshots from the pinned production Python router,
  message send and registered callbacks. The generator verifies exact clean
  references before importing them. Cases cover pathless and known-path exhaustion,
  changing medium measurements, activation beyond 20 seconds, never-activated and
  activated closure, packet timeout/no-receipt/exception, late callbacks, Resource
  complete/rejected/failed, Direct/backchannel selection, oversized opportunistic
  escalation and two prepared messages sharing one Link.
- Shared propagation uses different final recipients. Captured transfers must
  match both prepared byte sequences exactly, and retries preserve signed bytes
  and propagation envelopes. Assertions run outside production exception handlers
  so swallowed send/callback errors cannot silently establish a false oracle.
- Direct Resource completion is delivered and rejection is terminal; propagation
  completion is custody sent and rejection retries. Native packet timeout returns
  OUTBOUND, while no receipt or externally closed SENDING can remain SENDING even
  after a replacement Link activates. These stalls are characterized explicitly;
  they are not evidence of successful retry or an invented recovery policy.
- Added [LINK-OWNER.md](LINK-OWNER.md) as the remaining implementation map within
  item 3. It preserves the exact retained Link context, one acquisition authority,
  continuous attempt budget, owned transfer outcomes, terminal commit fence and
  full SDK/ZeroMQ and target/baseline mixed-peer acceptance. It does not create an
  extra PR or reduce the six-item plan.
- Validation: corruption of captured packet bytes fails at the intended external
  byte-preservation assertion. Two clean generations reproduce SHA-256
  `fec1e14faeca2b7fb34a738b540e09a7a7bd0258ec30f020d376f5ab4fd06e4e`.
  Existing generated fixtures remain unchanged. Logs:
  `/tmp/lxmf-item3-link-trace-red.log` and
  `/tmp/lxmf-item3-link-trace-generator-{0,1}.log`.
- Independent correctness and POST/maintainability reviews accept this bounded
  fixture/documentation checkpoint. Removed the unused single-message binding
  identified during review. No Rust runtime changed, so this checkpoint does not
  claim Rust consumption, concurrent acquisition, native watchdog, real Resource
  cancellation, cryptographic recipient proof or interoperability acceptance.
  Controlled cancellation assigns message state; shared-Link processing is
  sequential. Rust durable terminal monotonicity remains an explicit stronger
  boundary than manually injected late Python callbacks.
- Next: implement the reviewed retained Link/acquisition boundary and consume the
  reference transitions through the admitted owner. All remaining item-3 gates
  remain open; no item-3 PR has yet been created or merged.


### Item 3 atomic outbound Link acquisition

- Fixed concurrent `Transport::link` callers publishing separate Links for one
  destination. Selection snapshots the existing map entry; publication rechecks
  pointer identity under the existing handler lock. Only the winner dispatches an
  initial request. Reuse also revalidates map membership, so a removed entry is
  not returned while explicit reset waits to close it.
- PRE review identified public restart reviving the same Closed Arc during
  candidate preparation. Publication now rechecks native Closed state with a
  nonblocking Link lock and retains that guard through insertion. If contended,
  it drops the handler before awaiting the Link and retries selection. No Link
  guard is held across handler reacquisition or packet dispatch.
- Publication and initial dispatch use the same existing handler send lock;
  explicit reset/maintenance cannot remove the candidate between these actions.
  Explicit reset retains its destructive semantics, and callers must still observe
  native lifecycle after acquisition. Cancellation does not reset a shared Link.
  No per-target mutex registry or second lifecycle owner was introduced.
- Extracted the existing method from the nearly full `close_channel.rs` section
  into the existing establishment section. The public API, routing, request sizing
  and handler send path remain the same. The trade-off against a new lock registry
  and primary Rust/Tokio references are recorded in [LINK-OWNER.md](LINK-OWNER.md).
- Four deterministic regressions failed against the original implementation:
  competing absent entries, competing Closed entries, restart during preparation
  and reuse while reset has removed the entry. All pass after the fix. A fifth
  stages contention at final Closed revalidation and proves unrelated handler
  access remains available before the same Arc is restarted and reused. Tests use
  real transport mutex barriers and explicit future polling, not scheduling sleeps.
- Independent correctness and POST/maintainability reviews accept this bounded
  acquisition checkpoint. Daemon propagation cache expiry/reset removal, retained
  Link context, continuous retry budget, native activation waits, transfer recovery,
  successful SDK/ZeroMQ delivery and target/baseline mixed-peer acceptance remain
  pending. The 42 Link/Resource reference rows are not yet consumed by that owner.
- Validation: all-feature transport library suite passes 944 tests (9 existing
  ignores); daemon bridge integration filter passes 33 tests and retained-owner
  filter passes 99. The issue-369 scanner, all-feature daemon/transport library and
  binary Clippy, formatting, boundaries, architecture and module-size checks pass.
  Logs: `/tmp/lxmf-item3-link-acquire-{red,green,transport,bridge,owner,quality,clippy,architecture,boundaries,size}.log`.
  This is local validation only; item 3 still has no PR or hosted CI result.


### Item 3 propagation cache lifecycle

- Removed the daemon cache's 25-second expiry and its unconditional transport
  reset on a miss. All non-Closed native statuses remain reusable, including
  Stale. Empty or mismatched selection hints delegate to atomic `Transport::link`
  and cannot independently close a shared propagation Link or remove its Resource
  state. Removed the displaced age field/constant rather than retaining stale
  lifecycle metadata. Explicit reset and native closure retain their authority.
- Two regressions fail against the preceding implementation at the intended
  exact-Link preservation assertions. The Resource regression establishes a real
  Link through two Rust Transport ingress paths, holds the first Resource in
  flight across both empty and mismatched cache cases, starts a second distinct
  payload, and checks actual recipient bytes plus correlated sender completion
  proofs. No replacement Link request or failed/cancelled Resource is accepted.
- The lifetime regression waits 26 real seconds because the displaced cache used
  `std::time::Instant`. The native establishment allowance is 90 seconds; the
  pending Link remains cached beyond the former grace and is replaced only after
  explicit native close. This is not an automatic watchdog-expiry test. Resource
  payloads are raw bytes, not LXMF envelopes for different final recipients.
- POST review identified that same-Arc reacquisition alone could hide expired
  cache state because Transport would reuse that Link anyway. Added a direct
  cache-retention assertion before reacquisition and an isolated counterfactual
  restoring only the old grace while leaving destructive reset removed.
- The cache is explicitly a selection hint, not a lifetime lease. A concurrent
  explicit reset may retire a returned Arc; the retained owner must observe exact
  native state before dispatch. The forthcoming Link owner, continuous attempts,
  activation waits, method-aware Resource recovery, SDK/ZeroMQ and mixed-reference
  acceptance remain in scope and incomplete.
- Validation: the isolated grace counterfactual fails at the direct cache
  assertion with destructive reset still removed. After restoring the fix, all
  101 bridge-owner tests pass, as do three legacy propagation-cache tests, the
  issue-369 scanner, all-feature daemon library/binary Clippy, formatting,
  boundaries, architecture and module-size checks. Logs:
  `/tmp/lxmf-item3-propagation-cache-{red,grace-red,green,owner,legacy,quality,clippy,architecture,boundaries,size}.log`.
- Correctness review accepts the final assertion/correlation changes. POST review
  found no production blocker and its cache-policy evidence correction is fixed
  and proven. The next retained-driver PRE boundary is recorded in LINK-OWNER.md.
  This is local validation toward item 3, with no PR/hosted CI/completion claim.


### Item 3 retained native Resource outcome

- Found a prerequisite for reliable transfer ownership: the legacy Resource
  consumer publishes propagated custody through a best-effort nonterminal receipt.
  A full receipt queue can drop that update after correlation removal. An owner
  that simply waits for the resulting stored status could retain admission forever.
- Added opt-in `send_resource_observed_with_completion`. The existing Resource
  manager registers observation atomically with tracking before packet dispatch,
  and publishes one native terminal outcome before forwarding unchanged legacy
  events. Results are retained independently of broadcast lag. Existing send APIs
  allocate no observer; a dropped observer does not cancel the transfer.
- Completion uses the logical/original hash across segments. Intermediate proofs
  do not report success; Complete, Failed, Rejected and Cancelled remain distinct.
  Terminal publication removes the producer entry even when its receiver was
  dropped. Producer destruction without an outcome is observation-unavailable.
- Extracted the existing send-method group into one transport section to preserve
  the module-size limit. No new retry policy or receipt persistence authority was
  introduced. Documented the existing send-future cancellation boundary: after
  registration, drive dispatch to completion or explicitly cancel the Resource.
- Five new tests cover registration before dispatch failure, actual failure-event
  overflow, original-hash reset/reject/cancel in pending/first/later segments,
  recipient-byte reconstruction and final proof of a two-segment transfer,
  receiver drop, repeated result reads and producer disappearance. The existing
  two-Transport cache test now retains a successful result through an unread
  broadcast overflowing with actual dispatch failures.
- A counterfactual disabling completion publication fails four intended
  regressions. The production call was restored before validation. All-feature
  transport tests pass 949 tests (9 existing ignores); all 101 daemon bridge tests
  pass. Issue-369 diagnostics scanning, all-feature daemon/transport library and
  binary Clippy, formatting, boundaries, architecture and module-size checks pass.
  Logs: `/tmp/lxmf-item3-resource-completion-{focused,native,red,transport,owner,quality,clippy,architecture,boundaries,size}.log`.
- Independent correctness and POST/maintainability reviews accepted this native
  prerequisite. Native observation is implemented; the daemon retained Link owner,
  reliable custody persistence, method-aware retries and target/baseline mixed-peer
  acceptance remain pending. No item-3 PR, hosted CI or completion is claimed.

### Item 3 atomic propagation custody storage

- Reproduced restart re-upload after the ordinary `sent: propagated resource`
  status: it left the durable operation scheduled. Added a typed native custody
  command through the shared writer that atomically updates status, journal and
  dispatch state. Local dispatch completion remains distinct from recipient
  delivery, and generic sent text cannot grant custody.
- Custody spends retained preparation capacity while preserving signed bytes and
  the separate recipient-terminal reservation. Preparation and one-way fallback
  retain the exact maximum serialized custody-event cost. Duplicate custody and
  already committed terminal outcomes spend nothing.
- Review found that refusing older insufficient reservations permanently would
  strand restored messages. Fixed this by repairing only the shortfall through
  ordinary logical/physical admission in the existing writer. Restored reads and
  identical preparation commit that repair before exposing bytes; prior-run
  acknowledgements repair atomically with custody. Full storage defers repair
  until capacity returns. Journal failure rolls back the repair too.
- Seven focused regressions cover restart, exact/full-budget accounting, later
  delivery/cancellation, duplicate ordering, fallback capacity, old reservation
  recovery and journal rollback. The original restart counterfactual failed.
  All-feature RPC tests pass 822 tests with 8 existing ignores. All-target,
  all-feature RPC Clippy, issue-369 scanning, formatting, boundaries, architecture
  and module-size checks pass. Test-helper lint findings were corrected.
  Logs: `/tmp/lxmf-item3-custody-{red,focused,rpc,clippy,quality,format,architecture,boundaries,size}.log`.
- Independent correctness and POST/maintainability reviews accepted the fix.
  The typed transaction is a storage prerequisite; daemon receipt publication,
  commit acknowledgement and retained Link ownership remain pending. No item-3
  PR, hosted CI or completion is claimed.

### Item 3 trusted daemon custody entry

- Extracted the existing receipt application block without introducing a second
  status lock or persistence authority. `record_propagation_custody` selects a
  private, non-deserializable disposition and derives ID/status from typed native
  arguments. Ordinary RPC fields cannot grant custody; identical prior sent text
  still requires the atomic dispatch-completion transaction.
- Reused metadata, trace, ticket and event handling. Custody does not mark a ticket
  delivered; later recipient delivery still does. Storage failure returns before
  success publication. Review found missing messages could resemble idempotent
  success; typed custody now returns NotFound with no event/trace, preserving the
  existing ordinary receipt contract.
- Five new daemon tests and an extended authenticated-RPC regression cover those
  boundaries, terminal ordering, nonbroker compatibility and restart. The positive
  authorization test required its fixture identity bridge to list its imported
  identity; production authorization was unchanged. All seven focused broker
  tests pass. All-feature RPC tests pass 827 tests with 8 existing ignores;
  all-target/all-feature RPC Clippy, issue-369 scanning, formatting, boundaries,
  architecture and module-size checks pass. Logs:
  `/tmp/lxmf-item3-custody-daemon-{focused,auth,rpc,clippy,quality,format,architecture,boundaries,size}.log`.
- Independent correctness and POST reviews accepted this prerequisite. Recorded
  the upcoming queue lifecycle in LINK-OWNER: paired transient permits cover the
  admitted owner and custody/terminal race, while explicit committed retirement
  releases unused permits and preserves accepted work. SQLite terminal capacity
  remains reserved; indefinite unused queue permits would block unrelated traffic.
- The receipt worker/publisher and retained Link owner are not wired to this entry
  yet. No item-3 PR, hosted CI, interoperability or completion is claimed.

### Item 3 local custody publication and commit ownership

- Connected typed custody events to the existing sole receipt worker. Atomic paired
  queue reservations protect custody and recipient-terminal races; each accepted
  disposition retains its own pending state. Custody observation reports Stored
  or superseded by a committed terminal only after persistence. Worker failure
  closes observation without inventing success. Ordinary sent text remains ordinary.
- Local propagation returns a typed custody event to the scheduler after ingestion.
  The scheduler drops execution permits before publishing/waiting, retains count,
  bytes and membership, and retries only publication on queue backpressure. It
  does not re-ingest or retransmit. Failed observation retains admission until
  coordinated shutdown. A boxed event keeps the existing attempt/error enum small.
- Committed custody retires unused transient reservations while preserving accepted
  terminal work and its existing fence. Terminal commit likewise preserves accepted
  custody. SQLite recipient-terminal capacity remains independent. Publication and
  pending registration share one lock, with owner-failure checks repeated inside it.
  Failed enqueue releases unused reservations.
- Correctness review caught shutdown between native success and first publication;
  fixed it by always attempting initial reserved publication, including during
  shutdown, before leaving the observation wait. POST review caught new stranded
  slots on remote legacy sends; paired admission now applies only to the selected
  local typed producer until the remote retained owner lands.
- Four publisher tests cover atomic rollback, saturation, duplicates, commit order,
  sequential completion beyond queue capacity, late terminal admission and shutdown.
  Three real worker tests cover SQLite retry with pending terminal work, terminal
  supersession and fatal failure. Three local-owner tests cover admission retention,
  real second-peer handoff while custody storage is blocked, shutdown after ingestion,
  worker drain and fatal-failure ownership. The second peer learns its route from a
  real signed announce. The former test-only local-send helper assertion was updated
  to inspect the returned custody disposition; production owner tests prove commit.
- Validation: 31 all-feature daemon library tests, 104 bridge tests, 6 receipt-worker
  tests and 4 legacy receipt integration tests pass. Issue-369 scanning, all-feature
  library/binary Clippy, formatting, boundaries, architecture and module-size checks
  pass. Logs: `/tmp/lxmf-item3-custody-worker-{lib,bridge,receipts,legacy,quality,clippy,format,architecture,boundaries,size}.log`;
  focused fault traces are in `/tmp/lxmf-item3-custody-{integration,owner}.log`.
- Independent correctness and POST/maintainability reviews accepted this checkpoint.
  Remote Resource-to-custody publication, the continuous retained Link owner and
  mixed-reference acceptance remain pending. No item-3 PR, hosted CI or completion
  is claimed.

### Item 3 retained-context Resource dispatch

- Changed the shared Resource send implementation to accept the caller's exact
  Link Arc. Existing public ID APIs resolve once and delegate; the delivery
  helper's oversized-payload fallback no longer reselects by ID. The narrow
  public completion variant preserves registration-before-handoff and native
  failure observation. Legacy sends allocate no completion observer; their
  Arc adapter stays crate-private.
- Added a regression that replaces the map entry after selection and verifies
  the original Link's advertisement, decryptable ciphertext, interface handoff
  and observer ordering. It failed before the fix with InvalidArgument, then
  passed. A second regression verifies registration and retained failure when
  no mapping/interface is available. The existing real two-Transport Resource
  transfer now exercises the Arc completion API through recipient proof and
  subsequent legacy broadcast overflow.
- PRE review and correctness/POST review accepted this bounded prerequisite.
  Review correctly limited removed-map evidence to initial dispatch; this does
  not establish full native routing for removed Links, same-ID Link isolation,
  a lifetime lease against closure/restart, or remote daemon ownership. The
  remaining exact-context cancellation/teardown, packet timeout, propagation
  signal binding and retry ordering are recorded in LINK-OWNER.
- All-feature transport tests pass 951 tests with 9 existing ignores; all 104
  daemon bridge tests pass. All-target/all-feature transport and daemon Clippy,
  issue-369 scanning, formatting, boundaries, architecture and module-size checks
  pass. Logs: `/tmp/lxmf-item3-bound-resource-{before,after,transport,bridge,clippy,quality,format,architecture,boundaries,size}.log`.
- Item 3 remains active. No item-3 PR, hosted CI, full mixed-peer acceptance or
  completion is claimed.

### Item 3 retained-Link cancellation and teardown

- Native Resource cancellation now rejects a Link-ID mismatch before changing
  pending, active or segmented state. The regression failed against the preceding
  implementation and now preserves the original transfer and observer until its
  matching Link cancels it. Moved cancellation into the existing cleanup module
  after correctness review caught the regular-module size limit.
- Added exact-Arc cancellation and teardown. The ID cancellation API delegates;
  native Resource mutation and completion remain in ResourceManager. Teardown
  closes/removes only the selected Arc and publishes logical failures in one
  handler/Link critical section. Busy-Link waits release handler. Lifecycle frames
  retain the captured ID/interface after releasing Link, preserving replacement
  Links and concurrent restart behavior.
- A fallible internal Link teardown keeps closure even when frame construction
  fails. Legacy callers retain their Option API with an actionable diagnostic.
  Frame handoff failure does not reverse an observed native Cancelled/Failed result.
  The APIs govern the current Arc generation; earlier-generation cleanup is not
  promised if a public restart/request changes that same Arc's ID.
- Eight new tests cover wrong-Link cancellation at three transfer stages, native
  frame-construction failure through both APIs, active inbound/outbound teardown
  and replacement Resource preservation, failed handoff, pending closure, handler
  availability under Link contention and old-interface teardown during restart.
  Maintainer review reconciled conflicting cancellation documentation. A real
  Transport regression now proves that dropping interrupted dispatch then explicitly
  cancelling its registration clears native state before handoff and after queuing
  but before confirmation. Pending cleanup can emit no cancel frame; the daemon
  owner will use drive-to-completion before cancellation instead.
- Correctness and POST reviews accepted the resolved checkpoint. The native Link
  construction-failure branch is tested; Resource publication in the corresponding
  transport build-error branch is established by inspection. Full remote delivery
  ownership, typed remote custody and mixed-peer acceptance remain pending.
- Validation passes: 959 all-feature transport tests (9 existing ignores), all
  104 daemon bridge tests, all-target/all-feature transport and daemon Clippy,
  issue-369 scanning, formatting, dependency boundaries, architecture and module
  size. Logs: `/tmp/lxmf-item3-link-lifecycle-{before,after,focused,cancel-contract,transport,bridge,quality,clippy,format,architecture,boundaries,size}.log`.
- The next integration remains one continuous remote Link driver, including packet
  timeout/proof ownership and mode-specific Resource completion; no item-3 merge
  or full parity completion is claimed.

### Item 3 draft PR and first hosted validation

Opened and attached [#673](https://github.com/FreeTAKTeam/LXMF-rs/pull/673) on
2026-10-18. GitHub confirms OPEN/DRAFT, mergeable, base
`23a96a4a13bba3237c93c66f8d45443cc3d119ad` and published head
`a6886c29bbc25bdcac55a9f02a1105d72ba90616`. The body explicitly lists remaining
remote-owner, baseline-delta, mixed-peer, callback/SDK and final validation work.

The first hosted runs on that checkpoint are:
- [CI 39100469293](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/39100469293)
- [Verify 39100469289](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/39100469289)
- [Independent interoperability 39100469295](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/39100469295)

Selection has passed and the applicable jobs are running. This records draft
validation only; terminal results and final-head review/CI/merge remain required.


### Item 3 integrated retained Link driver (local acceptance pending)

- Production completion now uses one continuous Link/Resource owner with native
  Pending activation, continuous method budgets, immutable prepared bytes and
  method-specific outcomes. Explicit Direct-to-propagation fallback stays one way.
- Removed the old 20-second sender, detached cancellation/signal monitors,
  separate generic discovery owner and outbound broadcast receipt publisher.
  Resource correlation now retires only after the retained native owner cleans it.
  SINGLE overflow returns to the same production prepared-message driver.
- Remote propagation uses paired receipt reservations and typed packet/Resource
  custody through the existing sole writer. Five real peer cases pass: Direct
  packet/Resource, oversized Opportunistic Resource and propagated packet/Resource.
  They reconstruct exact committed bytes, return native proofs, hold admission
  across blocked SQLite commitment with free execution/stamp permits, avoid
  retransmission and commit one journal receipt plus terminal/custody state.
- Native shutdown cleanup and receipt cancellation after peer closure pass.
  Another test blocks an independent RX Resource operation's handler with a full
  interface queue; coordinated shutdown still joins and retains unresolved native
  correlation. Restoring the bare timing await fails at the intended shutdown
  deadline. This is not cleanup while that independent handler stays blocked;
  transport shutdown owns the remaining native lifetime.
- Three receipt-column read faults interrupt/join real cost-255 stamp workers,
  release permits and resume build attempt one without charging retries or
  fabricating terminal failure. Unavailable status while waiting for execution
  also releases partially acquired permits.
- Review caught native errors before the packet observer being replaced by
  absence. A real handler barrier/Link closure reproduces it; propagation is fixed.
  A neighboring real SQLite fault waits without failure/fallback and resumes one
  handoff after recovery. Duplicate native token decline, producer loss and final
  weak-context release also have regressions.
- Creator-bound signals now use authenticated ordinary Link data and prioritize
  rejection over pending proof retirement. The generator validates 47 scenarios
  and 263 snapshots, including five creator-ordering/replacement cases. The native
  parser has an explicit 32-container budget. The Rust row adapter is pending.
- All-feature transport tests pass 968 tests with 9 existing ignores; the daemon
  binary suite passes 572 tests. Module-size checks pass. Logs are
  `/tmp/lxmf-item3-link-cutover-{transport,daemon,size}.log`,
  `/tmp/lxmf-item3-link-transfer.log`,
  `/tmp/lxmf-item3-link-handler-shutdown{,-before}.log` and
  `/tmp/lxmf-item3-link-preobserver-error-{before,after}.log`.
- The accumulated disposition ledger is `docs/status/lxmf-1.2.0-delta.md`.
  File-export, source-blackhole and autonomous-peer gaps remain explicit; local
  management sync must not become their claimed network proof.
- Published checkpoint `a6886c29` has 20 successful hosted checks and one deliberate
  publication skip. This integrated code is still uncommitted/local. Adapter,
  mixed-reference/announce/SDK recovery, quality gates, independent review,
  final-head hosted CI and merge remain required. Items 4-6 remain pending.

## Item 3 reference-owner acceptance checkpoint (local)

- The production `complete_delivery` adapter consumes all 47 pinned Python
  scenarios and 263 snapshots. Its case/event-only inputs control measurements,
  decision turns, receipt timeout and the labelled Error/NoReceipt producer faults.
  Links, packet proofs, Resources, recipient bytes, SQLite and admission are real.
- Assertions cover every shared message, exact Link/creator, native status and
  activation history, continuous budgets/deadlines, request/teardown decisions,
  transfer counts, native handoffs, remote peer bytes, final-recipient binding and unchanged committed
  preparation. Durable terminal/custody results are bounded to one of each kind.
- Committed cancellation preserves native Cancelled cleanup and ignores synthetic
  later callbacks; this explicit projection is not a claim of callback resurrection
  parity. The native packet observer intentionally accepts owned late proofs.
- The adapter exposed premature replacement of a Closed propagation hint for a
  new message; `/tmp/lxmf-item3-owner-trace-closed-cache-before.log` retains the
  failing attempt count. The current adapter passes after consuming that hint
  once and retiring only the pointer-equal context on closure.
- Two real blocked-SQLite creator-signal regressions failed before the fix and
  pass for Packet and Resource custody. Their accepted custody work stays with
  the same worker; a reserved terminal rejection fences owner retirement. The
  signal wins a both-ready tie until the owner observes the commit acknowledgement.
  Post-acknowledgement late-signal controls preserve completed custody.
- Independent review found stale Direct SENDING/transfer state surviving explicit
  propagation fallback. The focused state regression failed before the reset;
  prior packet proof tokens stay outside that transition and keep their meaning.
- Current focused all-feature owner suite: **44 passed** in 7.89 seconds, including
  all adapter snapshots and native storage/signal cases. Log:
  `/tmp/lxmf-item3-owner-integrated-focused.log`. Module-size and dependency-boundary
  checks pass. Full daemon/quality refresh is ongoing; no current-head hosted or
  mixed-peer claim is added by this checkpoint.
- Removed the unread identified-Link cache/consumer and obsolete Resource-map
  receipt-worker plumbing. Native verified Link state owns selection; outbound
  local-source routing and authenticated propagation-control state remain intact.

- Final local refresh for this checkpoint: daemon **573 passed** in 32.91 seconds;
  daemon library **31 passed**; native packet bridge integration **2 passed**;
  issue-369 scanner passed; formatting, both-package Clippy with all targets/features
  and `-D warnings`, dependency boundaries, module-size and architecture checks
  passed. Logs use `/tmp/lxmf-item3-link-cutover-{daemon,library,packet-bridge,369,clippy,boundaries,size,architecture}.log`.
  The default-feature architecture build emitted the pre-existing unused BLE
  runtime-status helper warning; the all-feature Clippy gate remains clean.

## Item 3 SDK and mixed-reference qualification (in progress)

- Public `Client<ZmqPipelineBackendClient>` now exercises the canonical router,
  production delivery owner, real native Transport peer and SQLite together.
  Two unknown-peer sends exhaust the unchanged protocol retry policy. External
  receipt notification returns an error, then panics; the same client subsequently
  delivers exact signed content to the intended peer. SDK status/replay, one
  persisted terminal result per operation, callback recovery, admission release
  and joined shutdown pass. Log: `/tmp/lxmf-item3-sdk-recovery.log` (28.79 seconds).
- The sink is enabled through public revision-checked `configure`: startup's
  negotiation type does not carry `event_sink`. This qualification does not
  claim startup-field propagation. Independent correctness POST approved the
  real SDK/native boundary; its requested exact exhaustion-cause assertion is
  included in the following validation refresh.
- The matrix keeps baseline `727830ce` as its default and adds explicit immutable
  `lxmf-1.2.0` selection at `c3ff2d6d`, with unchanged RNS `99de23c0`. Neither
  reference dirtiness nor candidate dirtiness/revision checks can be bypassed.
  Baseline/target mismatch and clean-worktree gates have focused regressions.
- Live smoke checks now require native Resource delivery and typed propagation
  custody, including method, peer, representation, proof hash and envelope size.
  They retain exact Python stored-content assertions. Wire announce app data and
  Rust ingestion metadata are captured separately in the two propagated lanes.
  These harness changes are not yet mixed-peer PASS evidence.
- Hosted head `57f3efb4` failed only quality/its aggregate CI gate: new Clippy
  `chunks_exact_to_as_chunks` flagged the shutdown test. Use the MSRV-supported
  `as_chunks::<4>().0.to_vec()` conversion. Independent interoperability passed;
  Verify was still running at the last check. The corrected head needs all
  applicable hosted checks again.
- Development target runs now pass `resource_transfer` (16,549 signed wire bytes,
  native `delivered` Resource receipt), `propagated_rust_to_python` (302-byte
  native packet custody and retained `lxmd`/`1.2.0` metadata) and
  `propagated_python_to_rust` (Python observes `reticulumd`/`0.14.0` on the wire).
  Reports are under `target/interop/item3/development/`. These dirty-worktree
  runs do not replace clean-commit baseline/target matrix evidence.
- The first target Resource run correctly rejected 16 KB under upstream's new
  1 KB default. Both reference lanes now explicitly configure the baseline's
  1 MB receiving policy; the delta ledger preserves this default difference.
  No production limit or reference file was changed. The unused Rust inbound
  hook is disabled for this outbound-only lane, as in other Rust-to-Python lanes.
- Read-only maintainability review found first-page-only SDK capture and route
  readiness preceding metadata ingestion. Capture now follows cursors, retains
  all pages and hard-fails on gaps/truncation/stream changes; exact announce and
  target peer metadata have a deadline-bound readiness barrier. A Bash regression
  rejects a newly observed gap even when an older aggregate already contains a
  valid completion, including invocation under `if` where errexit is disabled.
  All **16** focused Python evidence/profile tests pass.
- The complete daemon binary suite now passes **574** tests in 36.69 seconds,
  including the stricter real-exhaustion failure cause. Workspace all-target,
  all-feature Clippy with `-D warnings` passes in 21.51 seconds. Logs:
  `/tmp/lxmf-item3-sdk-daemon-full.log`,
  `/tmp/lxmf-item3-workspace-clippy.log` and
  `/tmp/lxmf-item3-reference-evidence-unit.log`. Full clean matrices, workspace
  tests and exact-head hosted checks remain required.
- Clean `73fe0ec2` baseline/target matrices were interrupted after initial
  failures: Python RNS's default shared Unix socket joined another local instance
  despite configured per-case TCP ports. Exact reference source confirms this
  selection. The harness now explicitly uses `shared_instance_type=tcp`, honoring
  its dedicated ports; target Direct delivery passes with this isolation. The
  original logs and `target/interop/item3/73fe0ec2/interrupted.json` are retained.
  A full rerun on the corrected clean commit remains required. Concurrent profiles
  must use explicit disjoint shared/control/endpoint and Rust ports, recorded in
  each case's environment, instead of relying on PID-modulo defaults.
- The workspace run stopped at the process-level proof-replay restart test's
  transport-status deadline. Isolated package, workspace-selected and complete
  workspace target-selection reruns of that exact test pass (0.15–0.18 seconds).
  This is not a full workspace pass or a confirmed production fix. Keep the
  failure log `/tmp/lxmf-item3-workspace-tests.log` and rerun the complete suite
  separately from the mixed-peer processes to qualify this boundary.
- Incremental correctness review found that the new native evidence helper
  rejected failure but could accept custody followed by `rejected`, `cancelled`
  or `expired`. The focused negative regression failed for all three before the
  correction; it now rejects every negative terminal in the captured history.
  Log: `/tmp/lxmf-item3-negative-terminal-before.log`. No owner/state behavior
  changes are involved; this tightens the successful-live-lane evidence gate.
