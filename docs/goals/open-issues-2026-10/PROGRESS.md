# Open-issue execution progress

Started 2026-10-17 from main `a8e500e88610210582e8dd8869df8b510e1e75fc`.
User scope: [PLAN.md](PLAN.md) items 1–6, one reviewed, CI-passing merged PR each.

| Item | State | PR / exact head / merge | Evidence |
| --- | --- | --- | --- |
| 1. Acceptance reconciliation | Merged | [#671](https://github.com/FreeTAKTeam/LXMF-rs/pull/671); head `5358bf7399c3921182580ef8a4bd8a01ea6ed665`; merge `a0a72d9b9f2d53d62b9e6d2b0f61614410dc8004` | Independent review passed; 15 hosted checks passed, 2 deliberately inapplicable jobs skipped. |
| 2. Automatic CI and protection | Merged | [#672](https://github.com/FreeTAKTeam/LXMF-rs/pull/672); head `18816fa6f71c1e57e58465c8e8e6379a40767a20`; merge `23a96a4a13bba3237c93c66f8d45443cc3d119ad` | Independent review and native blocked/restored proof passed; 20 hosted checks passed, 1 deliberate skip. |
| 3. LXMF 1.2.0 | Merged | [#673](https://github.com/FreeTAKTeam/LXMF-rs/pull/673); final head `609edf12f738b1021eaaad963e828f0495544bee`; merge `9c27747753488ed950c52a2c7717ddec14478786` | Independent reviews/verifier approved; 20 hosted checks succeeded, one publication job deliberately skipped. Both local pin matrices and hosted baseline matrix pass 30/30; PR HIL passes 23/23 on first attempts. Known independent peer divergences and release-check package conflict remain explicit. |
| 4. Transport and RNS 1.5.5 | In progress; thirteen of twenty family rows locally qualified | Branch `corvo/open-issues-04-transport` from item-3 merge | [TRANSPORT.md](TRANSPORT.md), [REQUESTS.md](REQUESTS.md) and [DISCOVERY.md](DISCOVERY.md) record ownership, finite cells and validation boundaries. IFAC/publication, configured root ownership/restoration, duplicate admission, native Link routing, request receipts, common live child policy, discovery runtime/process cases and the three-run deeper recovery matrix pass bounded reviews and local checks. TCP/Backbone, UDP, Pipe, Serial/KISS, I2P, Auto, simulated Meshtastic and Weave family traces qualify locally. Seven other families and whole-item integration/hosted gates are open. No item-4 PR or merge yet. |
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

## Item 3 SDK and mixed-reference qualification

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
- The full workspace run in isolation passes **3,770 tests** across **139**
  binaries, with **288** existing opt-in ignores, including the process-level
  replay/restart regression. Log: `/tmp/lxmf-item3-workspace-tests-isolated.log`.
- Fresh release-check passes formatting, Clippy, workspace checking, **3,206**
  nextest tests (103 existing skips), its Cargo test/schema stages (**3,782**
  passing tests, 288 existing ignores), then reaches the same published-package
  UUID conflict recorded for item 2. The daemon dry run cannot combine published
  ZeroMQ's exact UUID 1.23.3 with the local 1.26.1 lock. No release-check PASS is
  claimed; later stages are unreached. Log: `/tmp/lxmf-item3-release-check.log`.
- Hosted nextest at `012a1d46` exposed an adapter input-ordering race in
  `propagated_signal_sent_pending_creator`: frame queue acceptance plus 32 yields
  could grant a decision/receipt commit before native signal decoding. The
  test-only adapter now waits for the exact decrypted native Data event, which
  follows synchronous signal observation, and awaits actual packet proof tokens
  before proof snapshots. Closed-generation frames retain their explicit no-op
  projection. PRE review approved this boundary; protocol and production owners
  are unchanged. Original CI failure:
  `/tmp/lxmf-item3-ci-012a-nextest.log`. All **eight predeclared stress iterations**
  pass (47 cases / 263 snapshots each; 57.675 seconds total). All-target/all-feature
  daemon Clippy with `-D warnings`, module-size, formatting and diff checks pass.
  Log: `/tmp/lxmf-item3-trace-input-barrier-stress.log`. Independent correctness
  POST approved the input boundary without changing production ownership/policy.

### Item 3 clean full compatibility qualification

- Clean committed candidate `5a0a30f55eac94f6bf5dcb7963f5be28771a8abd` passes
  **30/30 cases at each pin**, 2026-10-18 08:23:08–08:29:36 UTC. Each report
  contains 23 live Python/Rust cases, seven local Cargo policy cases, no missing
  required dispatch, failures, blocked cases, ignored tests or skips. Candidate
  and both exact reference repositories are clean in preflight; fault injection
  is disabled. Main's canonical reference manifest is unchanged.
- Reports are `target/interop/item3/5a0a30f5/baseline/matrix.json` (SHA-256
  `855835d887c070bd2769e71209bc2f0a560bbd1b042a8f4b78aafdf964ecef6a`) and
  `target/interop/item3/5a0a30f5/lxmf-1.2.0/matrix.json` (SHA-256
  `dad165428cf44c8419099ea3408d8ecf8d07afcb9490cc667b715f4e20d0c279`).
  Logs are `/tmp/lxmf-item3-{baseline,target}-matrix-5a0a30f5.log`; complete
  commands, per-case environment, raw logs, native events and peer content remain
  beside each report. This final documentation update does not change the tested
  runtime, fixtures or harness.
- Invoke `target/reference-prep/venv/bin/python tools/scripts/python_compat_matrix.py
  --all --reference-profile PROFILE --python "$PWD/target/reference-prep/venv/bin/python"
  --python-rns-path "$PWD/target/reference-prep/repos/rns-parity"
  --python-lxmf-path "$PWD/target/reference-prep/repos/REFERENCE"
  --output REPORT`, with `RUST_TEST_THREADS=4`. Baseline uses profile `baseline`,
  reference directory `lxmf-baseline`, shared/control/endpoint ports
  `22101/22102/22111`, Rust RPC/transport `127.0.0.1:22301/22302`; target uses
  profile/directory `lxmf-1.2.0`, ports `22201/22202/22211` and
  `127.0.0.1:22401/22402`. Set the recorded `PY_SHARED_INSTANCE_PORT`,
  `PY_INSTANCE_CONTROL_PORT`, `PY_ENDPOINT_CONTROL_PORT`, `RUST_RPC_ADDR` and
  `RUST_TRANSPORT_ADDR` variables explicitly. All ten ports were checked available
  together before these concurrent runs. Python shared instances use TCP.
- All six Direct/Opportunistic/propagated directions pass. Remote propagation
  controls and offers, both Link liveness/teardown directions, 16 KB native
  Resource, interchange and path policies also pass. Resource/propagation gates
  require exact native message/method/peer/representation/hash evidence and reject
  any later negative terminal; recipient stored content is checked independently.
- Target propagation metadata contains decoded `lxmd`/`1.2.0` plus raw wire bytes
  and peer identity in Rust; the baseline's missing fields stay absent. Both
  Python peers decode truthful `reticulumd`/`0.14.0`, with matching stamp costs.
  Both lanes explicitly configure Python's 1 MB receiving policy. The target's
  1 KB default, accumulated atomic-file/source-blackhole/autonomous-peer gaps,
  signal depth budget and stronger Rust terminal/late-proof behavior remain in
  the delta ledger. No general forward-parity or physical-network claim follows.
- Seventeen Python profile/evidence regressions pass. Hosted nextest now passes
  on `5a0a30f5`, including the corrected native input-completion barriers. Final
  documentation review and exact-final-head hosted checks remain integration
  gates; item 3 is not yet merged and items 4–6 remain pending.
- Independent verification decoded all 14 receiver payloads across six directions
  plus Resource at both pins: content, source and destination match. Native
  Resource proof binds 16,539 baseline / 16,549 target full-wire bytes; propagated
  packet custody binds the actual 302-byte envelope separately from recipient
  delivery. All raw announce metadata and ingested target peer fields agree.
  Correctness accepted the four slices without a local acceptance blocker.
  Maintainability review required reconciling the current LINK-OWNER status,
  now updated, and corrected one existing roadmap release-heading link. Historical
  checkpoint prose retains its original scope. Final-head hosted CI/merge remain
  required; no package publication or broad parity promotion is implied.

### Item 3 final hosted evidence and merge

- Final head `609edf12f738b1021eaaad963e828f0495544bee` changes only five
  documentation files from the clean locally qualified runtime/test head.
  [CI 39112075697](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/39112075697),
  [Verify 39112075772](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/39112075772)
  and [Independent interoperability 39112075773](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/39112075773)
  succeeded. All 20 applicable checks passed; the publication job was deliberately
  inapplicable. The three native required contexts remain bound to Actions app
  15368 on this exact head. Protection readback matches the approved policy.
- Hosted Verify's uploaded matrix passes all 30 cases (23 live, seven local)
  without skips/ignores at baseline LXMF `727830ce` and RNS `99de23c0`; its PR HIL
  controller passes 23 cases (12 virtual, 11 Python-reference), one attempt each.
  Target LXMF 1.2.0 qualification remains the separate local clean 30/30 run.
  Artifacts are retained under `target/interop/item3/hosted-609edf12/`.
- The hosted matrix SHA-256 is
  `0578d62c2d840dbe2bc6b79f237846a8d655b41819be707bfa6cb00de8e5cbfa`.
  The independent archive SHA-256 is
  `723e573e0eea4f3df01fe5aaca279d2353c642b992e31bd98689c2f635c7f811`,
  matching its sidecar and separately downloaded report bytes. Its existing
  compatibility gate accepts 79 PASS, three known peer-divergence FAIL and two
  dependency BLOCKED results. The signatures/owners match the unchanged explicit
  policy; this is not an 84/84 pass or expanded performance qualification.
- GitHub's tested synthetic merge `60584c368b90b0ae5c68ba16b3f97531ba2ec0b3`
  has the recorded head and base `23a96a4a` as parents. Independent verification
  approved the scoped merge after checking reports, JUnit, pins and final checks.
  No review threads remained; GitHub reported CLEAN/MERGEABLE. A normal squash
  merge, without admin bypass or remote branch deletion, completed at
  2026-10-18T09:14:46Z. Fetched main contains `9c277477`, whose parent is the
  recorded base and whose tree equals the tested merge candidate's tree.
- The broader #648 delta gaps, physical/public-network boundaries and existing
  release-check registry conflict remain open. Item 3 completes the approved
  four-slice increment, not every accumulated Python feature.

## Item 4 PRE and starting boundary

- Live #608/#609/#649 and all 22 open issues were refreshed; no issue was added or
  closed since the acceptance map. Item 4 begins only after verified item-3 merge.
- Independent PRE exploration identified root workers starting before IFAC policy
  publication, raw-record alias drift, TTL-based duplicate membership instead of
  Python generations, and disabled/attached persistence truncating the owner's
  hash file. Existing child preparation, IFAC wire codecs and lifecycle evidence
  are reused, not replaced.
- Discovery qualification already exists in the library; its daemon consumer,
  atomic name/endpoint admission and operator-detach coordination are missing.
  Request response limits/broadcast waiters do not establish pending request
  receipt ownership. Live mode/outgoing/gravity/pacing setters miss existing
  children. See TRANSPORT.md for the accepted owner boundaries and required cases.
- The current-session versus persisted manual-detach lifetime was asked as an
  optional preference while CI ran. Without a different response, the proposed
  rule is session intent, cleared by successful explicit reattach/reload or a new
  daemon owner. It is an explicit Rust extension; Python does not define a
  persistent suppression/reset contract. IFAC work does not depend on this choice.

### Item 4 first IFAC/publication checkpoint

- Three public SDK v2/canonical ZeroMQ to production UDP regressions first failed
  on plaintext admission or selection of the wrong pass phrase. One resolver now
  applies nonempty startup alias precedence and retains byte-significant legacy
  RPC credentials. Matching ingress/first egress, negative-key controls, IFAC
  provenance and exact rejection counters pass.
- Shared IFAC publication now fails before changing any state or metadata when
  a parent, inherited child or plaintext local child's state lock is poisoned.
  Virtual aliases are deduplicated. Prepared manager spawn retains legacy APIs,
  carrier defaults, control handles and worker-tree lifecycle; invalid preparation
  never constructs a worker. All four live hot-apply families use preparation,
  including the existing multicast routing owner without a lock-order inversion.
- The existing native mutation worker owns committed interface records. The
  additive bridge defaults preserve synchronous legacy behavior; real native
  commands return queued/incomplete. The RPC caller no longer overwrites a fast
  native result or emits premature completion. Complete ordered snapshots retain
  disabled, unmanaged, old-active and failed-replacement outcomes as applicable.
- Independent POST found stale inactive bindings, malformed caller runtime
  metadata panicking the worker, lost reload completion and the TCP status key.
  The actual malformed-RPC regression reproduced the panic before correction.
  Fixes discard caller runtime metadata, clear inactive bindings, retain reload
  context through the same command and use `tcp.stream_status`. Both independent
  reviewers rechecked and approved this focused checkpoint without a remaining
  concrete finding. Bootstrap and all later item-4 slices remain outstanding.
- Final local validation: all-feature daemon binary 580 passed, zero ignored;
  all-feature transport library 972 passed, nine existing ignored; all-feature
  RPC library 807 passed, eight existing ignored. The manager subset is 80
  passed/two existing ignored. All three expanded record-publication tests and
  three SDK alias tests pass, including blocked worker, ordered replacements,
  copied-live detach/failed replacement, later reload/TCP continuation and cleanup.
- Affected-package all-target/all-feature Clippy, formatting, issue-369 scanner,
  boundaries, module size and architecture checks pass. Logs are
  `/tmp/lxmf-item4-{daemon,transport}-slice1.log`,
  `/tmp/lxmf-item4-rpc-final-slice1.log`,
  `/tmp/lxmf-item4-clippy-final-slice1.log`,
  `/tmp/lxmf-item4-record-publication-final.log`, and the focused before/after logs.
  No item-4 full workspace/release/reference run, hosted check, PR or merge has
  happened yet. The next slice cuts over configured bootstrap roots.

### Item 4 configured bootstrap checkpoint

- The first checkpoint is committed as `b6524635b4a72d0c7a2eac4750ba020a33cf1ffb`.
  This second checkpoint cuts configured carrier roots over to the same fallible
  preparation owner before worker construction: selected TCP/Backbone listener,
  streams, UDP unicast/multicast, Pipe, I2P, serial/KISS, Auto, mesh/radio and
  available BLE backend paths. Existing MTU/IFAC defaults, routing registration,
  status handles and owned worker joins remain in their existing owners.
- An attached client previously could start traffic before global attached mode
  was installed. The native manager now retains an activation-gated worker;
  Transport sets mode and releases the gate in one synchronous guarded handoff.
  Preparation and activation do not overlap manager/handler guards. A real
  held-handler regression reproduced three cancellation hangs before a cancel-vs-
  lock select was added. Dropped startup and manager stop join without constructing
  the carrier. Failed handoff preserves both possible prior modes, while successful
  handoff commits mode independently of that worker's later lifetime.
- The frozen local reference distinguishes automatic shared-instance setup from
  explicitly configured Local interfaces. Only the existing trusted synthetic
  marker selects plaintext TCP/Unix children. A real explicit Unix accepted-child
  test first exposed plaintext admission despite parent IFAC; the default now
  inherits IFAC and the native automatic path explicitly opts into plaintext.
  Native wire tests cover first ingress/egress, exact negative-frame counts,
  existing/new child rotation and joined listener/child shutdown. Daemon wiring
  tests cover synthetic TCP sidecar, synthetic/explicit Unix, and both TCP/Unix
  attached modes through configured startup.
- Touched oversized startup and carrier files were extracted into bounded,
  same-namespace carrier/status/worker sections. Existing test namespaces and
  fixtures are preserved through included files under `/tests/`; public paths and
  old spawn API bounds remain unchanged. Four obsolete module-size exemptions
  were removed. The deferred API alone requires `Send + 'static` because it owns
  the unconstructed worker until activation.
- All-feature transport library: 982 passed, nine existing ignored. All-feature
  daemon binary: 589 passed, zero ignored. The daemon configured subset is 17
  passed and final interface-startup subset is 34 passed. Affected all-target/
  all-feature Clippy, formatting, boundaries and module-size checks pass. Logs:
  `/tmp/lxmf-item4-bootstrap-{transport,daemon}-tests.log`,
  `/tmp/lxmf-item4-bootstrap-startup-tests.log`,
  `/tmp/lxmf-item4-shared-daemon-policy.log`,
  `/tmp/lxmf-item4-{shared-activation-red,shared-client-ifac-red}.log`,
  `/tmp/lxmf-item4-bootstrap-clippy-final.log`.
- Independent correctness and maintainability POST both approve this checkpoint
  without a concrete remaining finding. App first-frame tests establish startup
  wiring/authentication; native held-handler/constructor tests establish ordering
  and cancellation. The issue-369 scanner and architecture checks also pass; logs
  are `/tmp/lxmf-item4-bootstrap-{quality,architecture}.log`. The full family lifecycle,
  duplicate-cache/queue, request, discovery, child policy and deeper recovery
  requirements remain open. No item-4 hosted check, PR or merge yet.

### Item 4 duplicate foundation and ordinary admission checkpoint

- Configured-startup checkpoint committed as `d7624f7e`. The next bounded change
  keeps two membership generations, TTL proof/source records and retained Link
  receipts in PacketCache with separate intent operations. Strict current-count
  greater than 500,000 rotates at the native 250 ms maintenance interval; existing
  90-second/180-second TTL cleanup remains separate. Tests exercise both real
  threshold rotations, displacement, previous-to-current remembering, lookup
  without promotion and proof-context independence.
- Regressions first reproduced TTL expiry forgetting duplicate membership,
  non-owner save truncating the owner's actual hash file, and a full queue
  inventing proof context for a pre-existing member. Current-only persistence,
  disabled/attached no-I/O returns, membership-only restore and atomic ordinary
  admission correct those failures. The final filter/enqueue/remember section uses
  current mode under handler -> cache -> synchronous queue. Cache rollback state
  was removed; path-request rollback remains. Competing candidates enqueue once
  and filter drops stay distinct from queue-overload counters.
- All three outbound registration sites retain pre-handoff packet/recipient
  bindings, including attached mode, and skip only membership in that mode.
  Prepared explicit/implicit proof tests retain wrong-known-signer/untracked
  negatives; declined prepared handoff remains side-effect-free. Existing outbound
  pre-dispatch timing is preserved, not claimed as newly exact Python behavior for
  failed/no-route sends. The private PacketCache has no external API exposure;
  obsolete combined removal was deleted and combined update remains test-only.
- A real authenticated Resource-control regression exposed wrong-interface Data
  changing a pending Resource to Rejected. Current local Link interface selection
  now precedes Resource/Data handling and forgets membership only on this failure.
  Both inbound/outbound local Link cases remain pending on the wrong interface,
  then accept identical correct-interface replay to the existing terminal result.
  The existing generic Link-map lookup was moved to its Handler owner and reused.
- The frozen `99de23c040d507e3fefca19e87b182302902725d` checkout is clean. A new
  explicitly run differential covers all 21 filter contexts and 18,144 Cartesian
  predicate cases. This is predicate evidence; production guards and valid native
  packet-shape routing are separately bounded. Original wire hops are retained as
  immutable filter input. Link destinations/LRPROOF defer to current drain state;
  no stale queued remember flag or additional reservation registry was introduced.
- Independent correctness and maintainability POST approve this foundation and
  ordinary-admission checkpoint. Native transport tests pass 996 with ten ignored
  (nine existing plus the new explicitly passed Python differential). The first
  daemon run had 588 passed/one failure: a shutdown I/O-fault fixture used a disabled
  transport, which now correctly skips hashlist I/O. The fixture now explicitly
  enables its transport owner while preserving its error and subsequent path-table
  and tunnel-write assertions; its focused regression passes. Final all-feature
  daemon suite passes 589 with no ignores. Affected all-target/all-feature Clippy,
  diagnostics scanner, formatting, boundaries, module-size and architecture checks
  pass. Touched oversized test includes retain their namespaces under `/tests/`,
  removing two more obsolete size exemptions. Logs include
  `/tmp/lxmf-item4-cache-{admission-transport,transport-final,filter-reference}.log`,
  `/tmp/lxmf-item4-cache-{queue-red,queue-green}.log`,
  `/tmp/lxmf-item4-local-link-data-{red,green}.log`,
  `/tmp/lxmf-item4-membership-{ttl-red,save-red,generation-tests,prepared-proof}.log`,
  and `/tmp/lxmf-item4-cache-{daemon-tests,shutdown-fault-final,daemon-final}.log`.
- Routing acceptance remains open: transit LRPROOF must bypass generic outbound
  remembering; local pending LRPROOF needs generation-bound expected hops and the
  remember-before-validation rule; transit direction/hops and authenticated
  rebalancing must be enforced, including KeepAlive's separate forwarding path.
  Queued Link-table insertion/removal, retained late proofs after teardown, and
  local wrong-interface ordinary/Resource Data need the remaining composition
  cases. These, the family lifecycle matrix, request/discovery/child policy and
  deeper recovery still gate item 4. No hosted check, PR or merge yet.

### Item 4 native Link routing checkpoint

- Duplicate foundation committed as `0dc92b4d`. Test-first regressions reproduced
  generic outbound remembering of the first forwarded transit LRPROOF, missing
  local matching-hop invalid-proof membership, unchanged paths after a valid
  mismatch, and KeepAlive forwarding from an unrelated interface on an unvalidated
  Link. Preserve the failing logs; the fixes stay in the existing Link, LinkTable,
  PacketCache and dispatch owners.
- Native first request dispatch binds private expected hops to the current Link
  ID and actual selected route, including unknown 128 and shared zero/one. Pending
  retries retain the binding and one-shot rebalance marker. Restart clears both;
  delayed old requests cannot bind a restarted generation. Local proof selection,
  remembering and validation hold one generation guard. Equal-hop invalid input
  is sticky; a mismatch needs exact 96/99-byte shape, matching mode and valid
  destination signature before one observed-hop rebalance. Unsigned 97/98-byte
  suffixes and a valid signature for the wrong cipher mode are negative controls.
  Only an existing path's hop field changes; the hop header itself is unsigned.
- Transit request routing records remaining hops from its path. Valid first
  proofs on the recorded next interface may rebalance; validated entries reject
  later mismatches. Identityless SharedOwnerHandoff keeps explicit unknown
  distance and first-owner-observation binding, without inventing a destination
  path or falling back after a known identity fails validation. Transit LRPROOF
  calls the common dispatch tail without originating hash remembering. Repeated
  valid proofs forward; invalid signatures/wrong interfaces do not become sticky.
- Data, ordinary proofs, Resource traffic and KeepAlive use one early validated
  interface/direction/hop decision. The old KeepAlive helper/bypass was removed,
  including its empty-payload indexing hazard. Same-interface routes accept either
  stored distance only on that interface; third-interface rejection is a stronger
  Rust guard than the frozen same-interface branch. Rejected transit Data creates
  no source/proof context. Current transport/local-client eligibility controls
  table precedence; local Data recovery remains distinct from Proof acceptance.
- Actual preprocess/enqueue/dequeue tests span table insertion, pending expiry,
  proof activation, and local restart/replacement. The retained original packet
  proof is queued before teardown, then validates after replacement and TTL
  cleanup. Native unit fixtures exercise direction/hop combinations and failed
  state immutability. Twelve synthetic Channel/delivery/MTU tests now provide
  their direct Link call's bound post-ingress hop count. Seven daemon tests shared
  another direct zero-hop fixture; that fixture now sends the real signed proof
  through native ingress and waits for its exact Link activation event.
- Transport passes 1,011 with ten unchanged ignores; daemon library passes 31
  and daemon binary passes 589 with no ignores. The initial daemon run's seven
  synthetic-fixture failures remain in its log. Pinned RNS
  `99de23c040d507e3fefca19e87b182302902725d` passes the real
  `rust_to_python_raw_resource_roundtrip` (one test, sender completion and Python
  acknowledgement), separate from native/source-grounded routing evidence.
  This is not the deeper mixed-process relay matrix or broad Python state parity.
- A main-agent follow-up red exposed the distinct public `restart()` interval:
  it intentionally preserves the old public ID before the next request exists.
  A private readiness bit now fences hop binding, pending selection and direct
  proof activation in that interval, while clearing expected hops/rebalance state.
  The old signed proof was not previously ingressed in the regression. Actual
  queued drain and delayed old dispatch cannot mutate membership/path/activation;
  the subsequently constructed and dispatched new generation activates normally.
  Both reviewers approve this correction. Full transport/daemon suites, affected
  Clippy, formatting and module-size checks pass after it. Logs are
  `/tmp/lxmf-item4-routing-explicit-restart-{red,green,native}.log` and
  `/tmp/lxmf-item4-routing-restart-{transport,daemon,clippy,size}.log`.
- Logs: `/tmp/lxmf-item4-transit-lrproof-{red,green}.log`,
  `/tmp/lxmf-item4-pending-hops-{red,green}.log`,
  `/tmp/lxmf-item4-proof-hops-shape.log`,
  `/tmp/lxmf-item4-transit-hop-{red,green}.log`,
  `/tmp/lxmf-item4-routing-transport-{first,final}.log`,
  `/tmp/lxmf-item4-routing-daemon{,-bin,-bin-final}.log`, and
  `/tmp/lxmf-item4-routing-python-resource.log`.
- Independent correctness and maintainability POST approve this bounded routing
  checkpoint without an actionable remaining finding. Affected all-target/
  all-feature Clippy, formatting (including explicit included-file checks),
  boundaries, module size, architecture and issue-369 scanner pass. The final
  focused transit-dispatch regression also passes. Gate logs are
  `/tmp/lxmf-item4-routing-{clippy-last,quality,architecture,boundaries,size-final,dispatch-last}.log`.
  The carrier lifecycle matrix, request receipts, discovery runtime, child-policy
  propagation and deeper recovery still gate item 4. No hosted check, PR or merge.

## Item 4 request-receipt checkpoint

- [REQUESTS.md](REQUESTS.md) freezes the native owner/cutover contract. One
  Link-owned book replaces `pending_response_limits`, binding exact owner,
  generation, registration identity, request ID, source hash, size policy,
  phases and accepted response hashes. Retained watch outcomes survive absent
  or lagged legacy listeners; per-generation terminal ID fences cannot reopen.
  Large response data lives only with observers. Distinct-ID fences intentionally
  grow within a generation; no unapproved expiry/cap was invented.
- Native exact-Arc dispatch registers before handoff and rechecks generation
  and pointer-equal map membership after preparation. Packet IDs use the final
  encrypted packet hash; Resource IDs use packed request bytes. Handoff time
  precedes post-send Link bookkeeping, and Resource activation stays under the
  send handler before a fast first ResourceRequest can be processed. Legacy
  explicit-ID Resource APIs delegate this owner without gaining a new deadline.
- Exact target `7f2b3b9b524c9386316379af1313b43a5e4f7a5d` changes timeout/reject
  guards to include SENT but exclude RECEIVING. Packet ACK stops its packet
  timer while RequestReceipt stays SENT; Resource completion sets DELIVERED and
  starts the response timer. Native tests separately validate signed full-hash
  ACKs; a controlled executable target probe compares six guard rows, repeated
  callbacks, PacketReceipt ACK behavior and Resource completion. This is not an
  actual Python wire ACK trace or a general Python state-equivalence claim.
- Response size uses reencoded packet body minus two, and Resource `d`
  uncompressed total across compressed/split transfers. Invalid ordinary owned
  envelopes fail InvalidResponse; metadata-bearing raw responses preserve their
  explicit contract. Oversized/malformed then valid input cannot reopen a
  terminal request. RECEIVING guards preserve the accepted transfer.
- ResourceManager retains unpublished events across lookup awaits; per-Link
  drain, native application and legacy publication do not yield. Native reset,
  replacement, teardown and shutdown drain captured-generation cleanup before
  detaching owners. The supervisor interrupts blocked workers before closing
  retained Links. Explicit cancellation can retire an already abandoned-send
  registration without rewriting its terminal watch outcome.
- Independent POST reviews reproduced two blockers and approved their fixes.
  Response correlation now commits in ResourceManager after duplicate, order,
  assembly identity and receiver validation. Relabelled ordinary/other-request
  advertisements, invalid first segments and out-of-order advertisements cannot
  donate hashes. Cleanup requires request ID and response kind as well as Link
  generation/hashes, preserving unrelated receivers and partial assemblies.
  The original admission red is retained in
  `/tmp/lxmf-item4-request-admission-red.log`; native green/control evidence is
  `/tmp/lxmf-item4-request-post-fixes-focused.log` (124 pass, one opt-in ignore).
- `cancel_request_on_link_observed` signals native cleanup after releasing
  native locks and before optional I/O. rncp/rngit use that latch and the existing
  teardown latch so a full live carrier queue cannot delay return after cleanup
  commits. The live full-queue control verifies no invented frame handoff and
  exact unrelated-transfer completion. Its first extension incorrectly expected
  a Progress event for a one-part transfer; the manager emits Complete directly.
  The corrected assertion checks actual completion bytes; both logs are retained
  as `/tmp/lxmf-item4-request-full-queue{,-green}.log`.
- rncp status and rngit request waiters use retained outcomes. rncp's separate
  ordinary file Resource keeps its waiter; rnx has no scoped outgoing request
  constructor. Actual Git list/fetch/bundle/push/release operations and rncp
  binary push/fetch/error cases pass against both clean baseline `99de23c` and
  target `7f2b3b9`. Canonical reference pins are unchanged. Final reruns are
  `/tmp/lxmf-item4-request-{rngit-baseline,rngit-target,rncp-baseline,rncp-target,reference}-verified.log`;
  all five tests pass. Each target is an isolated exact checkout, not a sibling
  repository scan or moving-reference claim.
- Final all-feature transport passes 1,037 with 11 explicit ignores. Utility
  bins pass 260 with 86 opt-in ignores across 18 binaries. Default daemon passes
  31 library and 588 binary tests; all-feature daemon passes 31 and 589. Affected
  all-target/all-feature Clippy, explicit included-file rustfmt, Cargo fmt,
  boundaries, module size and architecture checks pass. Diagnostics initially
  rejected four ignored local-latch send results; explicit trace handling fixes
  all four without exemptions, and issue-369 scanner passes. Logs are
  `/tmp/lxmf-item4-request-{transport,tools,daemon,architecture,clippy,size,boundaries,fmt}-verified.log`,
  `/tmp/lxmf-item4-request-daemon-all-features.log`, and
  `/tmp/lxmf-item4-request-diagnostics-{verified,green}.log`.
- Same-namespace test extractions preserve existing names/helpers while bringing
  the touched Resource test root below the size policy and removing its old
  allowlist entry. The rncp roundtrip test is similarly extracted, with an exact
  opt-in target selector while the canonical pin check remains the default.
  The concurrent reset regression now asserts the atomic close/removal contract;
  an already-waiting acquisition may reuse that Arc before reset linearizes.
  Earlier fixture failures remain recorded, not relabelled as production passes.
- Independent correctness and maintainability POST both approve the bounded
  slice. This is a local checkpoint, with no item-4 PR, hosted evidence or merge.
  Family lifecycle, discovery runtime, live child-policy propagation and the
  predeclared deeper mixed-process recovery matrix still gate item 4.

## Item 4 common live child-policy checkpoint

- The four existing InterfaceManager setters now use the same registered
  subtree relationship owner as stop/join. Mode, outgoing, gravity and announce
  pacing update under the existing mutable manager owner. Root registration is
  checked first: an unknown address cannot mutate even an orphan record linked
  to it. A child-address update preserves ancestors, siblings and other trees.
  No new owner, registry, worker, public option or IFAC publication path.
- Native controls cover accepted, virtual, nested and plaintext-local children,
  later-created inheritance and unchanged IFAC choices. Actual direct handoff
  respects updated outgoing; recursive path-request filtering reads updated
  mode on a nonvirtual child so broadcast exclusion cannot mask stale policy.
- A queued announce retains its original timestamp and eligibility deadline
  across setters. Updated outgoing blocks release without dropping it; reenabling
  releases under the new pacing values while preserving its original mode
  admission. Existing PathTable entries are not rewritten. Reusing one exact
  signed equal-hop/equal-emitted announcement verifies child gravity changes the
  tie result, including rereading the current existing-path policy.
- The definitive four regressions first fail against target-only setters, then
  pass with subtree propagation. The initial pacing fixture used the explicitly
  unpaced targeted-send helper; it was corrected to production general broadcast
  before the definitive red run. Logs retain both attempts:
  `/tmp/lxmf-item4-child-policy-{red,red2,green,final-focused,focused-last}.log`.
- Independent PRE approved the existing-owner scope. Correctness POST requested
  capturing queued_at before the setters as well as before blocked release;
  the tightened control does both. Correctness and maintainability POST approve
  this bounded slice. Full all-feature transport passes 1,041 with 11 explicit
  ignores; all-feature daemon passes 31 library and 589 binary tests. Affected
  all-target/all-feature Clippy, architecture, diagnostics, explicit included-file
  rustfmt, Cargo fmt, boundaries and module size pass. Explicit formatting of
  the touched included management file exposed Clippy's existing descending
  comparator lint; stable `sort_by_key(Reverse(...))` retains bitrate order and
  its existing focused priority regression passes. No lint exemption was added.
  Logs: `/tmp/lxmf-item4-child-policy-{transport,daemon,clippy,architecture,diagnostics,size,boundaries,fmt}.log`.
  Final lint/sort checks are
  `/tmp/lxmf-item4-child-policy-{clippy-green,prioritize}.log`; explicit final
  size/format logs use `size-last` and `fmt-last` suffixes.
- This is common native-owner evidence. Carrier-specific startup/reconnect/error
  composition, discovery runtime and predeclared deeper mixed-process recovery
  still gate item 4. No item-4 PR, hosted check, merge or physical claim.

## Item 4 discovery configuration and planner preparation

- PRE approved the existing-worker contract in DISCOVERY.md. This bounded slice
  extracts startup configuration loading to one immutable read, retaining
  independent daemon/runtime/rejected-IFAC fallbacks and both exhaustive public
  struct layouts. The additive private unverified option falls back independently
  on malformed input. Four tests cover default/true/false/invalid values,
  malformed interface versus valid runtime, actual file replacement during the
  only read, and absent/unreadable/invalid established configuration.
- The existing initial planner delegates to an additive policy-aware variant.
  Endpoint exclusions apply even with override, and each planned name reserves
  its collision suffix for later candidates. Three focused regressions pass;
  the definitive red run first exposes invalid-loopback admission and duplicate
  names. Existing verified-default callers remain unchanged.
- A controlled subprocess imports the exact clean target commit
  `7f2b3b9b524c9386316379af1313b43a5e4f7a5d` and compares 78 qualification cases,
  nine endpoint classes and sequential collision allocation. Its first run
  exposes valid large version components rejected by u32 parsing. Saturating
  numeric parsing preserves Python's ordering against the fixed minimum, and
  the complete probe passes. This is predicate/name evidence, not a live peer
  or general discovery callback trace. Logs preserve red/green results in
  `/tmp/lxmf-item4-discovery-{planner-red,planner-green,target-planner-red,target-planner-green}.log`.
- Correctness and maintainability POST both approve this bounded preparation.
  All-feature transport passes 1,044 with 12 explicit ignores, all-feature
  daemon binary passes 593, and affected all-target/all-feature Clippy passes.
  Cargo fmt, explicit new/changed planner/include formatting, diagnostics,
  boundaries, module size and architecture pass. The extracted bootstrap core
  retains its preexisting formatting rather than introducing an unrelated
  whole-file rewrite. Logs use `/tmp/lxmf-item4-discovery-prep-*.log`.
- The parsed private override is deliberately not yet consumed by production
  admission. Native discovery composition, monitoring, suppression, trusted
  bootstrap restoration and all three declared process cases remain open.
  Family lifecycle and deeper mixed-process recovery still gate item 4; no PR,
  hosted check, merge or broad runtime qualification is claimed here.

## Item 4 bounded native discovery and monitoring

- The existing interface mutation worker now owns initial admission, persisted
  live observations, five-second periodic refill/monitoring and prepared stream
  installation. Startup publishes its complete snapshot first. Discovery-only
  configurations create a transport; attached clients do not run admission.
  A weak sender applies bounded backpressure after persistence without extending
  worker lifetime. External Apply preserves private discovery origins; caller
  `_runtime` cannot grant them. A private configured/discovered key enum keeps
  literal names, hidden configured keys and endpoint identities distinct in the
  existing managed map. Snapshot reverse lookups are ephemeral.
- Admission rechecks source, both native blackhole identities, endpoint, name,
  slot and session suppression. Default qualification remains verified RNS
  1.5.2+, and the parsed explicit override is now wired to admission. Linux uses
  the existing private Backbone builder, other platform selection remains
  ordinary TCP. Announce defaults are parsed privately from the same immutable
  snapshot with independent fallback; public struct layouts remain unchanged.
- Native status observation owns the first-down monotonic anchor. Fractional
  timing, skipped ticks, recovery and manager contention cannot authorize early
  teardown. Periodic refill chooses one available transport candidate while
  preserving the quarter-slot reservation. Manual detach suppresses the endpoint
  before stopping; failed configured attach/reload retains it, including a failed
  operation whose rollback succeeds. Successful requested native readiness clears
  it. Every owned worker is joined/aborted before returning a stop diagnostic,
  and record absence is published even on that error path.
- Trusted bootstrap recipes cover the existing four managed kinds plus private
  Backbone clients. Exact keys support unnamed and padded configured names.
  Restored UDP/listener/pipe status uses existing refresh workers; retained TCP
  handles publish native connection state through the mutation owner. Other
  families remain open. A failed UDP bind can still spend the existing five-second
  retry sleep before cancellation; this checkpoint does not qualify that family
  error-lifecycle cell.
- The first daemon-process probe fails to connect with the pre-runtime binary
  and passes with the new native worker; logs are
  `/tmp/lxmf-item4-discovery-native-{red,green}.log`. It connects to a test-owned
  socket, not a Python peer. Native IPv4 and IPv6 tests additionally assert exact
  packet bytes and selected interface identity. The validated-event test proves
  persistence-before-admission and rejects a disallowed source. It begins after
  outer event validation and preserves honest publisher metadata `LXMF-rs`.
- Retained real regressions cover native-only/network blackholes, backdated
  downtime, erased unrelated status, incorrect unnamed bootstrap lookup, workers
  left alive after stop failure, unpublished removal, missing restored UDP
  refresh and configured/discovered key/name collisions. Logs use
  `/tmp/lxmf-item4-discovery-{owner-native-blackhole-red,review-controls-red2,stop-red,publication-red,key-red,key-partial-red,key-green}.log`.
  The definitive TCP red waits for native connected state and then reproduces
  the old publication predicate in
  `/tmp/lxmf-item4-discovery-tcp-status-definitive-red.log`.
- Two initial assertions were fixture errors: production advertises `LXMF-rs`,
  and server acceptance can precede the client's connected status. Correcting
  these assumptions does not change production identity or relax a deadline.
  The final full all-feature daemon run passes 31 library and 616 binary tests
  in `/tmp/lxmf-item4-discovery-native-daemon-green.log`. Transport passes 1,044
  with 12 explicit opt-in ignores. Affected all-target/all-feature Clippy passes
  in `/tmp/lxmf-item4-discovery-native-clippy-final.log`. Cargo fmt, explicit
  changed include-file formatting, diff, boundaries, size, architecture and
  issue-369 checks pass. Preexisting bootstrap-core formatting is retained.
- Independent correctness and maintainability POST reviews approve this bounded
  checkpoint with no source blockers. The three declared target-process traces,
  remaining family composition and deeper relay replacement evidence still gate
  item 4. No PR, hosted CI, merge or broad qualification is claimed.

## Item 4 exact-target discovery process traces

- Three fresh topologies pass against clean RNS `7f2b3b9b524c9386316379af1313b43a5e4f7a5d`
  and LXMF `c3ff2d6dc2f256daab896dadc044dd5a913ecbb7`, with exact revisions,
  actual import paths, isolated storage/ports and owned child cleanup enforced.
  The predeclared 15-second readiness and shared 30-second discovery/traffic
  bounds remain unchanged. No production source or security policy changes.
- Rust's default admission accepts the real Python publisher; Python's override
  admits the honest Rust publisher. After discovery, native UDP is detached
  before destination announcement, and the Rust route/native parent and Python
  receiving object identify the selected Backbone connection. Exact captured
  packed bytes correlate to native LXMF callbacks with NUL/UTF-8 title/content,
  correct source/destination, valid signature and opportunistic method. Sender
  delivered receipts are independent assertions.
- Python's default rejects Rust only after authenticating and persisting its
  `LXMF-rs`/`0.14.0` metadata. A separate genuine qualified Python publisher is
  discovered and receives exact control bytes. The subsequent 10.02-second
  rejection interval retains three free slots across 395 native observations,
  with 20 cumulative Rust listener observations and no accepted Rust connection
  or Python endpoint reservation in any visible state.
- `/tmp/lxmf-item4-discovery-process-first.log` records an environment path error
  before any peer ran. `/tmp/lxmf-item4-discovery-process-native-first.log` passes
  two cases and retains the Rust-default fixture envelope error. The strengthened
  `/tmp/lxmf-item4-discovery-process-provenance.log` passes both positives but
  retains RPC rate limiting caused by its 25 ms observer. Only RPC sampling is
  paced to 500 ms; native sampling, production limits and the ten-second window
  are retained. `/tmp/lxmf-item4-discovery-process-paced.log` passes all three in
  12.84 seconds. No run is relabelled or overwritten.
- Maintainability review strengthens the negative control by requiring the
  advertised Rust native listener to reach listening readiness before traffic
  and remain listening at every paced sample; a failed bind cannot masquerade
  as policy rejection. No preflight connection contaminates its zero-accept
  count. `/tmp/lxmf-item4-discovery-process-listener-ready.log` passes all three
  in 12.80 seconds; rejection observes 10.03 seconds and the same 395/20 native
  and cumulative listener samples.
- The three ignored opt-in cases are wired into normal Verify with separate
  feature checkouts, preserving canonical pins. Focused Clippy, Cargo formatting,
  Python compile, diff, module size and actionlint (without optional shellcheck
  or pyflakes) pass. Hosted execution and the
  overall item-4 family/deeper matrix remain pending.
- Correctness and maintainability POST reviews approve the bounded process
  checkpoint after native-listener readiness is added. No source or proof blocker
  remains in this three-case scope; item 4 is not yet complete.

## Item 4 deeper middle-relay replacement matrix

- The predeclared three fresh topologies each complete all eight cells: small
  Direct packet and 131,101-byte Direct Resource in both directions, before and
  after middle replacement. RNS is clean `99de23c040d507e3fefca19e87b182302902725d`;
  LXMF is clean `c3ff2d6dc2f256daab896dadc044dd5a913ecbb7`. Actual import paths and
  one immutable daemon snapshot SHA256 are recorded. The Python shared owner,
  attached Rust edge A, standalone middle B, standalone edge C and Python TCP
  client have three-hop endpoint paths in all six phase snapshots.
- Only B is terminated/joined and replaced at the same endpoints from a fresh
  empty state directory with a different native identity. Endpoint/edge PIDs,
  identities and native Link caches remain; no fixture teardown resets their
  Links. Both edge remote routes must name the current middle identity before
  traffic resumes. Fresh native route learning is distinct from restoration.
- The explicit native-LXMF fixture mode retains LXMRouter's native packet,
  Resource and keepalive behavior. Exact callback content/title/source/destination,
  signature and Direct method are checked by message hash. Independent native
  sender completion asserts packet/Resource representation, handoff Link ID and
  Resource hash/Link identity where applicable. Final inbox counts require exactly
  four unique recipients at each endpoint, eight per run and 24 distinct cells.
- `/tmp/lxmf-item4-deep-relay-first.log` passes the entire first set in 127.38
  seconds. Review tightens route readiness to require the new middle identity
  at both edges and freezes the daemon executable; that full set passes in
  `/tmp/lxmf-item4-deep-relay-fresh-paths.log` (104.93 seconds). Monotonic completion
  fences then reject late control/probe success. The final candidate passes all
  24 cells in `/tmp/lxmf-item4-deep-relay-strict-final.log` (104.92 seconds), with
  maximum individual cells 15.128, 14.864 and 14.633 seconds by run. Native
  recovery uses its normal retry; no retries-until-green or bound changes occur.
- The original 60-second readiness, 30-second path/reconnect and shared 60-second
  message qualification bounds remain. They are loopback acceptance windows,
  not universal protocol maxima. An initial Rust wrapper imported many unused
  helpers and failed Clippy; a small direct build/snapshot wrapper removes that
  import without lint exemptions. Focused Clippy, formatting, Python compile,
  boundaries, module size, actionlint and diff checks pass.
- The older two-relay default/raw-Resource fixture separately passes in 36.95
  seconds against RNS `99de23c` and canonical LXMF `727830c`, retaining default
  fixture behavior. Its log is `/tmp/lxmf-item4-deep-relay-legacy-fixture.log`.
  The new matrix is wired into normal Verify with separate feature references.
  This closes the declared 24-cell deeper recovery gate locally. It does not
  qualify remaining carrier families, hosted CI, physical or public networks.
- Independent correctness and maintainability POST reviews verify the final
  strict log's exact cell counts, unique hashes, replacement bindings, native
  three-hop paths and immutable binary digest; both approve without remaining
  blockers in this bounded slice.

## Item 4 configured dispatcher preparation

- The initial configured loop now delegates each enabled recipe to one callable
  dispatcher, preserving the original index, label, selected-listener context,
  sequential Auto builder calls and all 21 startup-batch fields. The borrowed
  context lasts only for this call; no CLI credentials become retained state.
- A mixed startup characterization passes before and after extraction: disabled
  entries, a unique successful Auto runtime, selected listener annotation,
  unknown-family and Auto failures, UDP, Pipe, TCP client and shadowed listener.
  It checks ordered records/failures/seeds, listener accounting, tunnel/status
  effects, native worker joins and Auto listener release. Two initial fixture
  errors (worker API and existing error prefix) are retained in the logs; they
  are not claimed as production defects.
- Final all-feature daemon tests pass 31 library and 617 binary tests in
  `/tmp/lxmf-item4-bootstrap-dispatch-daemon.log`. Affected all-target Clippy,
  Cargo fmt, explicit changed include-file formatting, boundaries, module size,
  architecture, diagnostics scanner and diff checks pass.
- Independent correctness and maintainability POST reviews approve the bounded
  behavior-preserving extraction. [BOOTSTRAP.md](BOOTSTRAP.md) records the
  remaining root/effect, status/control, restoration and twenty-family cutover.
  No family gate or whole item is promoted by this preparatory checkpoint.

## Item 4 configured ownership and publication cutover

- The typed startup batch now retains every actual configured root, original recipe
  index, native status and unique effects in the existing mutation owner's map.
  Inactive/rejected/validation-only records retain absence. Duplicate identities
  use private occurrence bindings; protected records survive public four-kind
  Apply, reorder and discovered-record removal. Native failed Pipe repair and
  policy updates preserve the correct runtime metadata and binding.
- One immediate/two-second Delay publication tick displaces seventeen detached
  production refresh loops. Complete RNode/Weave projections exist before the
  worker returns and clear after retirement; retained old handles lose their
  native receiver. Generic cancel-aware adapter controls are explicitly narrower
  than radio packet or family acceptance.
- Bootstrap uniquely retains the mutation worker join while the daemon keeps its
  bridge. Cancellation preserves that handle and its original failure. A reviewer
  found that admission remained open during cleanup; the strengthened regression
  fails in `/tmp/lxmf-item4-configured-owner-retiring-admission-red.log`, then passes
  in `...-retiring-admission-green.log` after closing the existing receiver before
  retirement. A colliding-root Apply test now waits for the actual completion event.
- Native retirement fences/takes the current worker tree atomically. Ordinary and
  virtual late-child controls fail before the admission fix (`...-admission-red.log`)
  and pass afterward. The held-handler fence test confirms route cleanup after
  manager removal, all captured joins and unrelated-root continuity. Error tests
  settle every uncooperative/panicking worker or auxiliary before returning failure.
- The missing native duplicate-KISS disable fails before cutover in
  `/tmp/lxmf-item4-configured-owner-native-red.log`; subsequent focused native,
  projection, control-receiver and cancellation runs pass. Earlier fixture-only
  compile/configuration errors remain logged and are not production regressions.
- Full daemon runs initially fail Auto's final exclusive-bind probes in
  `...-daemon-second.log` and `...-daemon-final.log`; a separate stale Pipe readiness
  snapshot also fails and is repaired by returning the native readiness projection.
  Three predeclared inode-diagnostic cohorts retain results: first/third pass,
  second fails two Auto probes. `...-auto-inodes-2.log` captures an original data
  inode still present after stop with no descriptor left in the parent process.
- A controlled Auto/Pipe cohort delays child exec entry by 50 ms. In
  `...-auto-preexec.log` and `.trace`, pre-exec Pipe child PID 1135735 retains retired
  Auto discovery inodes 1410573/1434705 and 1419959/1419960. The trace records its
  successful `/usr/bin/cat` exec and corresponding CLOEXEC release. This directly
  attributes that controlled failure; the natural untraced failures are consistent
  with it, rather than individually PID-attributed. A broader whole-suite strace
  diagnostic perturbs unrelated protocol timing and is stopped with exit 143
  after confirming it has no child processes; it is not an acceptance result.
- Seven Auto bootstrap tests now execute unchanged native assertions in separate
  exact-case processes with case-specific child markers, successful status and
  one passed/non-ignored test required. The same delayed-exec control passes all
  seven in `...-auto-isolated-preexec.log`, compared with four earlier failures.
  A normal isolated cohort also passes. No production sleep/retry, timing-budget
  increase or shared-bind relaxation is introduced; temporary inode code is gone.
- Final normal all-feature daemon validation passes 31 library and 630 binary tests
  in `/tmp/lxmf-item4-configured-owner-daemon-isolated-final.log` (30.49 seconds for
  binary tests). Affected transport validation passes 1,047 tests with 12 existing
  ignored cases in `...-transport.log`. Default daemon check also passes, retaining
  the existing optional RNode BLE dead-code warning. Both independent source POST
  reviews approve this bounded cutover. Fresh affected all-target/all-feature
  Clippy, Cargo fmt, changed include formatting, boundaries, module size,
  architecture, diagnostics scanner, task-document links and diff checks pass.
  The legacy included `bootstrap_parts/module_core.rs` retains its existing
  formatting; its changed wiring is reviewed without reformatting the whole file.
  Final gate logs use the `...-clippy-approved`, `...-architecture-approved`,
  `...-diagnostics-approved`, `...-boundaries-approved`, `...-size-approved`,
  `...-fmt-final`, `...-include-fmt-final` and `...-doc-links` suffixes.
- [BOOTSTRAP.md](BOOTSTRAP.md) now marks root/effect retention and publication/
  shutdown cutover locally verified. General trusted restoration, twenty-family
  composition, whole-item release/integration, one PR and hosted CI remain open.

## Item 4 restoration constructor preparation

- Checkpoint `184a895b6adc9c9112b8c8d038aec9570853a971` completed the reviewed
  configured ownership/publication cutover above. This follow-up changes no
  restoration behavior and retains the same numbered-item/PR boundary.
- The per-recipe context and lower helpers now take only the existing strict
  preflight boolean. The initial Args wrappers convert that one field; no CLI/RPC
  credential or unrelated argument becomes retained context. Selected listener
  strictness remains CLI strict OR `panic_on_interface_error`, independently.
- Existing TCP/Backbone/Local listener startup moves into its adapter module.
  The seven-argument function reuses the same builder, prepared policy and
  original recipe/record; it retains labels, counts, strict readiness, status,
  configured rejection, override behavior and native shared-client semantics.
  Its manager guard is released before waiting for readiness. Failed bind
  readiness still retains the native root/status; missing configuration for a
  selected index never chooses unconfigured spawning.
- Three native characterization cases pass before extraction in
  `/tmp/lxmf-item4-startup-context-policy-baseline-native.log` and afterward in
  `...-policy-after.log`: config-only strict occupied listener versus independent
  non-listener preflight, policy rejection at the original record index, and
  CLI override of a shadowed recipe. A bound but unlistened TCP socket keeps the
  refused client endpoint reserved without a port-reservation race. Initial
  fixture failures omitted explicit enable flags and used a forbidden enabled
  client port zero; their earlier logs remain and are not production reds.
- A fourth direct helper control verifies missing-recipe rejection and an
  unrelated native channel. Full all-feature daemon validation passes 31 library
  and 634 binary tests in `...-daemon.log` (30.32 seconds for binary tests).
  Affected all-target/all-feature Clippy, default daemon check, Cargo and changed
  include formatting, boundaries, module size, architecture, diagnostics scanner,
  task links and diff checks pass. Both independent POST reviews approve the
  structural preparation. Gate logs use the same `/tmp/lxmf-item4-startup-context-`
  prefix with `clippy`, `default-check`, `fmt`, `includes-docs`, `boundaries`,
  `size`, `architecture` and `diagnostics` suffixes.
  General trusted restoration, immutable context retention, reconnect worker
  joining and finite family composition remain subsequent work.

## Item 4 trusted configured restoration

- Checkpoint `6b4cb31a878fc77e2eecc16835d1c292365ed8ac` completed constructor
  preparation. This bounded behavior change keeps the existing mutation worker
  and one eventual item-4 PR; implementation remains with the main agent.
- Narrow owned context retains original selected-listener configuration, separate
  carrier/listener strictness, storage root, transport identity, reconnect sender
  and unique synthesizer completion. It is retained with discovery disabled too;
  no Args, RPC credential bundle, additional actor or registry is retained.
- Restoration reuses the per-recipe dispatcher and shared selected-listener
  starter at the original index. Each attempt uses its fresh server address.
  Indexed outcome conversion consumes tunnel requests before moving native
  status, Auto shutdown, auxiliary and control effects. The old four-family
  restore match is removed. Known startup failure settles any registered root
  and publishes no native presence, allowing a later eligible tick to retry.
- Actual configured KISS at recipe index 2 moves to public slot 1, closes when
  the discovered native peer becomes online, and restores after peer loss. Wrong
  keys/plaintext remain rejected; exact authenticated ingress/egress and an
  unrelated TCP root's exact traffic pass before/during/after. The pre-change
  test fails only at restored TCP accept after earlier checks pass:
  `/tmp/lxmf-item4-bootstrap-restore-kiss-reordered-red.log` (13.03 seconds).
  Initial factory green is `...-kiss-green.log` (10.03 seconds).
- Independent correctness review found missing common successful runtime fields,
  duplicate publication after public endpoint replacement, and delayed control
  projection while a following constructor waits. Additional controls also keep
  explicit degraded failure outcomes. All five strengthened assertions fail in
  `...-review-complete-red.log` (13 pass/5 fail, 10.04 seconds); all eighteen
  focused cases pass in `...-review-green.log` (10.05 seconds) after fixes.
  Successful native outcomes regain common fields without replacing explicit
  degraded state. An entry lacking a binding derives its current slot from its
  pre-overwrite owned record. Controls publish before each exposed live record.
- The endpoint replacement trace uses public Apply to reorder/change the TCP
  endpoint, discovery disable, original trusted recipe restore and one public
  occurrence with exact unrelated/native traffic. Its first fixture omitted the
  disabled sentinel's name, so Apply correctly retained that protected row;
  `...-review-red.log` retains that fixture failure and genuine metadata reds.
  The corrected named sentinel reaches the duplicate-row production red above.
- A real selected listener failure at original index 2 leaves no stale root and
  retries after the test-owned port is freed. An I2P factory case preserves its
  original default storage/key path and transport identity without SAM use.
  LoRa validation-only and pending Reticulum BLE outcomes stay truthful. A real
  Weave adapter against an absent temporary device exposes its control queue
  while a following test-owned SAM greeting holds preparation; this proves
  publication/control availability, not serial/radio or SAM interoperability.
- Real discovery-disabled TCP shutdown joins native workers and synthesizer,
  preserving an unrelated channel. Injected synthesizer panic/timeout and
  cancelled shutdown verify native-first ordering, sender retirement, abort-and-
  await, propagated errors and the original retained completion. The caught
  injected task panic in focused output is expected and asserted.
- Full all-feature daemon validation passes 31 library and 642 binary tests
  in `...-daemon-final.log` (30.30 seconds for binary tests). All-target,
  all-feature daemon Clippy passes in `...-clippy-approved.log`. Earlier Clippy
  logs retain four fixture clone suggestions and the non-test unused refresh
  parameter; fixed with borrowed slices and the conventional cfg-only parameter
  prefix, without allowances or behavior changes. Default check, architecture
  and diagnostics scanner pass in `...-default-check.log`, `...-architecture.log`
  and `...-diagnostics.log`. Existing optional RNode BLE dead-code/manifest
  warnings and the environment's missing profile helper remain baseline output.
- Both independent source POST reviews approve. Cargo/include formatting,
  boundaries, module size and task-link/diff evidence use the same
  `/tmp/lxmf-item4-bootstrap-restore-` prefix. BOOTSTRAP.md, roadmap and parity
  matrix record bounded restoration separately from the remaining twenty-family
  matrix, whole-item release/integration, one PR, hosted CI and merge. No physical,
  RF, public-network or whole-issue completion claim is made.

## Item 4 TCP client constructor alignment

- Checkpoint `3b3b6723` completed trusted restoration. Family exploration then
  found that public/named client replacement omitted existing immutable options
  and tunnel hooks. The same native mutation owner now borrows manager, Transport
  and the retained reconnect sender for Apply, named operations and rollback.
  Only adapters clone the sender; retirement still joins the original completion.
- Raw-record construction and change detection share one narrow projection into
  DaemonConfig normalization/validation and the existing startup builder. The
  authoritative kind/endpoint and five constructor options are admitted; unrelated
  settings and runtime metadata do not select a constructor. Validation precedes
  queue acknowledgement/retirement and emits value-free errors. Initial tunnel
  synthesis occurs after releasing the manager guard; native reconnect uses the
  same startup hook. No new defaults, tasks, registry or retained Args are added.
- Three actual native regressions fail before the fix in
  `/tmp/lxmf-item4-tcp-options-native-red.log`: exhausted zero-retry refusal,
  immutable option-only replacement and malformed options acknowledged as valid.
  Their first green is `...-green.log`. The strengthened final control changes all
  five options individually, requiring each fresh connection and old-peer EOF.
- The configured/named trace checks invalid Apply and invalid configuration
  preservation, real occupied-listener replacement failure and old-client
  rollback, rollback reconnect, successful client replacement and reconnect.
  All five native connections receive fresh correctly signed/authenticated tunnel
  requests with the source identity/current native full hash. Exact bidirectional
  authenticated traffic, retained old/new MTU, unrelated TCP traffic/address and
  joined shutdown pass. The final focused run passes 23 TCP-related tests in
  `...-strengthened.log` (5.13 seconds). Earlier hook-fixture compilation failures
  remain recorded; the fixture now uses public destination/identity APIs without
  widening production visibility.
- Both independent final source POST reviews approve. Full all-feature daemon
  validation passes 31 library and 646 binary tests in `...-daemon.log` (30.23
  seconds for binary tests). All-target/all-feature daemon Clippy, default check,
  architecture, diagnostics scanner, Cargo formatting, boundaries and module size
  pass with the `/tmp/lxmf-item4-tcp-options-` prefix and corresponding gate names.
  Existing manifest/optional BLE/profile-helper warnings remain baseline output.
  The two existing TCP runtime-settings tests move unchanged into a test child
  module to preserve the 1,200-line test limit. These are constructor/ownership
  controls; finite carrier composition, whole-item integration, PR, hosted CI and
  merge remain open.

## Item 4 TCP and Backbone family software traces

- Checkpoint `fcc1b32c04a52550eecda86ebfb19afa659eb3b8` completes the preceding
  TCP client constructor/hook cutover. The finite next task keeps the existing
  startup/mutation/native ownership paths, public named-kind restrictions and
  one eventual item-4 PR. BOOTSTRAP.md records all four exact test names.
- Configured TCP and Backbone clients now have a composed native trace from a
  bound-but-unlistened socket's failure/status through listening on that same
  socket, wrong-key/plaintext rejection, exact authenticated ingress/egress,
  live key rotation, EOF/reconnect and changed-key configured restart. Backbone
  listener composition includes strict occupied-bind failure, retained sanitized
  status/join, existing/new child rotation and changed-key restart. Single-root
  transitions preserve the unrelated TCP address and exact bidirectional traffic;
  whole configured restart deliberately recreates all roots.
- The named TCP listener trace commits key rotation through public Apply, rejects
  unsupported tunneled reload without retiring the root, fails a real occupied
  replacement and authenticates the restored committed key while rejecting the
  attempted key. Successful reload, detach/attach, old-child EOF/removal and
  unrelated traffic complete through actual management replies. Preferred IPv6
  connections work after rollback/reload/attach; an occupied IPv6 candidate also
  proves native IPv4 fallback. MTU and the native preference/liveness status stay
  correct. Two-second packet/EOF, eight-second reconnect, three-second publication
  and existing ten-second join bounds remain unchanged.
- Initial `/tmp/lxmf-item4-tcp-family-red.log` contains fixture failures rather
  than production regressions: local `localhost` has no IPv6 candidate, initial
  status awaited publication, and fresh managers can reuse their address hashes.
  `...-configured-fixture-corrected.log` then exposes the fixture accepting a CLI
  preflight/dummy connection. Listener-only strictness now uses existing config
  policy. All three configured traces pass in
  `...-configured-policy-corrected.log` (10.09 seconds). Fresh-manager evidence
  requires old-tree retirement, a new Transport identity and fresh authenticated
  connections, rather than different manager-local hashes.
- The fixed ordered resolver candidates are `localhost`, then
  `localhost.localdomain`; both families must be loopback-only, and an independent
  IPv6 bind is mandatory. Locally the second name supplies the dual-host wire
  trace. Exact-`localhost` preparation remains a separate control; there is no
  local dual-`localhost` wire claim or resolver configuration mutation.
- Two genuine production controls fail in `...-localhost-native-v2-red.log`:
  forced IPv4 despite preferred localhost, and missing IPv6 alias collision
  rejection. The named trace reaches rollback and fails on the missing native
  preference flag in `...-preference-native-v2-red.log`; resolver ordering still
  permits its IPv6 connection, so this is a forwarding/status defect, not a claim
  that that connection failed. The initial versions of these two logs retain a
  test import compilation failure and are not runtime evidence.
- Cohesive listener bind/MTU helpers first move without behavior changes; 36
  existing hot-apply cases pass in `...-listener-extraction.log`. The minimal
  correction forwards the existing preference option, keeps preferred localhost
  as a hostname for native fallback, and preserves its default IPv4 alias.
  Correctness POST then finds expanded IPv6 and wildcard collisions. Both new
  assertions fail in `...-review-collision-red.log` (one pass/two fail); numeric
  endpoint canonicalization and the existing both-family wildcard-port guard
  fix them. Final controls pass three cases in `...-review-collision-green.log`;
  all four native traces pass in `...-native-reviewed.log` (10.10 seconds).
- Both independent source POST reviews approve. The earlier broad suite passes
  31 library/652 binary tests in `...-daemon.log`; `...-clippy.log` retains the
  test-only device-resolver reexport warning, fixed with cfg(test). The next
  653-test suite passes, but `...-architecture.log` flags the test module's
  root-directory classification. It now lives in the existing tests directory;
  no limit or allowance changes. Final daemon validation passes 31 library and
  653 binary tests in `...-daemon-approved.log` (30.69 seconds for binary tests).
  Final all-target/all-feature Clippy and architecture pass in
  `...-clippy-approved.log` and `...-architecture-approved.log`; default check,
  diagnostics scanner, Cargo/include formatting, boundaries, module size and
  task links/diff checks pass with their corresponding suffixes. Existing
  manifest/optional BLE/profile-helper warnings remain baseline output.
- The first two software family inventory rows now have their declared lifecycle
  evidence. Other carrier rows, whole-item integration/release checks, one PR,
  exact-head hosted CI and merge remain open. No physical/RF/public-network or
  whole-issue qualification is inferred from these traces.

## Item 4 raw IFAC scalar admission

- Checkpoint `c5ee78f50a3e8f13c5a1f0e3537f6d4702558694` records TCP/Backbone
  composition. UDP exploration then finds malformed credential aliases ignored
  during raw conversion and signed-size presence lost through `as_u64`. This
  preparation keeps the existing validator before queueing and the native crypto
  owner, without claiming UDP family completion.
- Validation and conversion now share the same ordered alias lists. Every
  present, non-null top-level IFAC alias must be a string, even when a valid
  preferred alias shadows it. Size admission matches typed startup's signed
  integer contract; negative values, including i64::MIN, become present Some(0),
  selecting the carrier default with credentials and rejecting size without
  credentials. Existing crypto validation retains flooring/maximum rules and
  the existing typed value-free error. Generic numeric-setting helpers do not
  change; absent/null values, empty fallback and whitespace bytes remain intact.
- Source inspection revises an initial test assumption: arbitrary scalar/array
  settings are explicitly preserved as `configured_settings` by existing
  metadata helpers. Both PRE reviewers approve preserving that compatibility.
  Validation reads only recognized top-level object leaves, never interpreting
  opaque or nested metadata as IFAC. The initial
  `/tmp/lxmf-item4-ifac-types-red.log` has one pass/five failures, including that
  withdrawn container assumption. The corrected contract red in
  `...-contract-red.log` has two passes/four genuine failures. All six controls
  pass in `...-green.log`, including malformed lower aliases, float/string/bool
  sizes, signed overflow/boundaries, empty/null fallback, signed presence and
  positive opaque-envelope preservation.
- A real SDK v2/typed ZeroMQ trace first establishes exact authenticated UDP and
  unrelated native TCP traffic. Its malformed object replacement is incorrectly
  acknowledged before the fix in `...-sdk-red.log` (0.06 seconds). After the fix,
  object-valued credentials, an integral float size and negative size without
  credentials reject with static value-free errors. Both native addresses and
  the old key remain, exact authenticated/binary traffic still passes, and a
  following valid command completes through the actual mutation event. Owned
  shutdown joins native workers and the canonical router. The full trace passes
  in `...-sdk-green.log` (2.04 seconds); its explicit three-second bound-status
  observation retains two-second packet barriers and ten-second native cleanup.
- Both independent source POST reviews approve. Full all-feature daemon tests
  pass 31 library and 660 binary cases in `...-daemon.log` (30.56 seconds for
  binary tests). The existing typed-startup negative-size regression passes in
  `...-startup-contract.log`. All-target/all-feature daemon Clippy, default
  check, architecture, diagnostics scanner, Cargo/include formatting, boundaries,
  module size and task-link/diff checks pass with corresponding
  `/tmp/lxmf-item4-ifac-types-` suffixes. Existing baseline warnings remain.
- This qualifies raw-input preparation only. Named UDP lifecycle and multicast
  virtual-peer composition, other family rows, whole-item integration, one PR,
  exact-head hosted CI and merge remain open.

## Item 4 UDP software family traces

- Checkpoint `524aa7051b5c88ffd430043b6a1fc03b6eb0fa27` closes raw IFAC
  preparation. Both source PRE roles approve the next two declared native traces
  in TRANSPORT.md. Main-agent implementation reuses production startup/mutation,
  native IFAC/UDP and existing unrelated TCP helpers. Only an existing cfg(test)
  refresh field widens visibility for native failure observation; production
  behavior/owners/defaults remain unchanged.
- Configured unicast authenticates exact ingress/egress, then public Apply commits
  live key rotation while retaining the root. An occupied replacement reaches
  actual bind_failed and owned retirement; management stays pending while exact
  unrelated TCP traffic succeeds. Rollback rejects the attempted key and accepts
  the committed key. Changed-key reload, detach/attach, stale-hash dispatch
  rejection, sanitized diagnostics and ten-second joined cleanup complete.
- Multicast proves actual loopback group egress with exact IFAC bytes/source, then
  signed unicast announces create distinct virtual peers for two source ports.
  Existing/new peers inherit rotation. Matching traffic succeeds after wrong-key
  and plaintext rejection; direct replies remain absent from the other peer
  during full two-second observations. Changed-key replacement and configured
  restart reuse the same destination identities with fresh signed announces;
  paths bind to active children of the current root without resetting duplicates.
  Old child hashes lose dispatch authority. Single-root transitions preserve the
  unrelated root/address/traffic; whole restart intentionally recreates all roots.
- `/tmp/lxmf-item4-udp-family-initial.log` retains a fixture compilation error
  (snapshot lacks Serialize), fixed by its existing to_json. The first native
  run in `...-native-initial.log` fails both cases: a retired failed worker keeps
  its bind_failed snapshot rather than closed, and multicast peer learning times
  out. `...-diagnostic.log` proves complete unicast success and locates the latter
  at the second announce, sent about 30 ms after the first and held 360 seconds by
  the existing 3.5 Hz new-interface guard. Temporary diagnostic logging is removed.
- An explicit one-Hz announce input schedule preserves default admission policy
  and two-second path deadlines. `...-paced.log` then fails an immediate PathTable
  erasure assertion. Both reviewers distinguish raw cached membership from peer
  ownership/dispatch retirement; pinned RNS 1.5.5 Transport.py:548-551 and :957-979
  separate removal from path maintenance too. No eager cache erasure is added.
  Same-destination/current-parent rebinding replaces that assertion. Review also
  strengthens an immediate isolation drain to full two-second observations.
- Final `...-rebinding.log` passes both traces in 16.24 seconds. Both independent
  source POST roles approve. Full all-feature daemon tests pass 31 library and
  662 binary cases in `...-daemon.log` (31.25 seconds for binary tests).
  All-target/all-feature daemon Clippy, default check, architecture, diagnostics
  scanner, Cargo formatting, boundaries and module size pass with corresponding
  `/tmp/lxmf-item4-udp-family-` suffixes. Existing manifest, optional BLE and
  profile-helper warnings remain baseline output.
- The first four of twenty software family rows now have their declared evidence.
  Sixteen other families, whole-item integration/release checks, one PR, exact-head
  hosted CI and merge remain open. Group evidence is native egress only, with
  signed unicast peer ingress; no native group-ingress, hardware, RF, public-network
  or whole-issue completion is inferred.

## Item 4 Pipe software family traces

- Checkpoint `1a83960198fa03f338feecbbf18bde06814fd5d5` records the reviewed
  UDP composition. Both source PRE roles approve two finite Linux process cases
  in TRANSPORT.md. Main-agent implementation adds one 461-line test module and
  Linux-only registration, reusing production bootstrap/native mutation and
  existing unrelated TCP helpers. No production code/default/owner changes.
- Configured startup uses an absent owned executable as argv[0], reaching actual
  native spawn failure/respawning status while retaining IFAC/MTU. A fully written,
  executable staging file is atomically renamed into place; automatic retry
  recovers on the same root. The child transparently relays actual stdin/stdout
  and a loopback observation socket. Four PID bytes are consumed only from that
  socket; /proc PPid and argv verify direct owned-child identity. No helper
  crypto, descendant process or signal probe is introduced.
- Every phase counts wrong-key/plaintext rejection, then proves exact native
  HDLC/IFAC ingress/egress with the carrier's eight-byte default. Public Apply
  rotates the key on the same root/PID; closing only that child's control input
  produces real exit, native reap and a fresh PID with retained policy. Malformed
  shlex command preparation preserves PID/root/traffic. A missing-executable
  named reload fails and rolls back to the committed command/key and native MTU
  512; successful changed-key reload and detach/attach use MTU 1024. Old socket
  EOF and bounded /proc disappearance establish every observed generation's
  reaping. Unrelated TCP address/traffic persists and final owned shutdown joins
  native/reconnect workers and reaps the final child. Fixture delay 0.25 seconds
  and all declared packet/status/reply/join bounds remain unchanged.
- Both declared native traces pass on their first run in
  `/tmp/lxmf-item4-pipe-family-initial.log` (2/2, 4.03 seconds); no production
  defect or pre-fix red is claimed. Independent source correctness and
  maintainability POST reviews approve. Full daemon validation passes 31 library
  and 664 binary tests in `...-daemon.log` (31.69 seconds for binary tests).
  All-target/all-feature daemon Clippy, default check, architecture, diagnostics
  scanner, Cargo formatting, boundaries and module size pass with corresponding
  `/tmp/lxmf-item4-pipe-family-` suffixes. Existing baseline warnings remain.
- Five of twenty software family rows now have their declared evidence. Fifteen
  others, whole-item integration/release checks, one PR, exact-head hosted CI and
  merge remain open. This is Linux software process evidence; no Windows,
  hardware, RF, public-network or whole-issue completion is inferred.

## Item 4 shared KISS pending-I/O retirement checkpoint

- Supervisor checkpoint `b1d7e536` precedes this bounded shared-owner change.
  Nine native pressure cases fail their two-second joins; normal ready shutdown
  and initial-write-error/no-cleanup controls both pass before correction.
  `/tmp/lxmf-item4-lora-kiss-pressure-red.log` retains 2 passes/9 failures in
  2.01 seconds. The final KISS suite passes 15/15 in 0.04 seconds in
  `...-kiss-pressure-green.log`, including four existing controls.
- The existing I/O body runs as one scoped future. An explicit false initial
  result preserves the old error/no-shutdown return. Ordinary loop exit permits
  ready-first cleanup selected against the same stop. Outer interruption drops
  pending I/O and skips further output on the possibly partial stream; final
  status still commits outside it. Existing inner stop selection, helper bodies,
  option layouts, protocol timing and owner count remain unchanged.
- Tests notify actual Pending write/flush operations, keep capacity-one peers
  and the exact receive sentinel held through cancellation/join, then observe
  EOF and exact partial/full bytes without false success counters. Explicit
  current-thread RX controls reach the full send before yielding; the processing
  counter is a barrier, and retained queue contents separately establish no
  cancelled admission. The parent remains live until explicit retirement, and
  the stream management receiver closes. Cleanup controls preserve exact ready
  shutdown bytes/counters and the original initial-error status.
- Both source POST roles approve. Default transport passes 1,054 tests with
  twelve existing ignores; default daemon passes 31 library/678 binary tests,
  including the LoRa native controls and configured Serial/KISS regressions in
  their respective suites. Transport all-feature passes 1,083 with twelve
  existing ignores. No post-change all-feature daemon suite is claimed here.
  Logs are `/tmp/lxmf-item4-lora-kiss-{transport-default,daemon-default,transport-all-features}.log`.
- Affected Clippy, diagnostics #369, architecture, boundaries, module size,
  explicit three-file formatting and diff gates pass in
  `/tmp/lxmf-item4-lora-kiss-{clippy,diagnostics,architecture,boundaries,size,fmt,diff}.log`.
  Generic bearer and configured LoRa/RNode traces remain open. This native
  shared-owner checkpoint does not qualify that family row: current totals
  remain thirteen of twenty, seven families plus whole-item integration/one
  PR/hosted CI/merge outstanding. No physical/RF/public-network evidence.

Commands: red `cargo test -p reticulum-rs-transport --lib kiss::pressure_tests`;
focused green `cargo test -p reticulum-rs-transport --lib iface::kiss::`;
affected `cargo test -p reticulum-rs-transport --lib`, its `--all-features`
variant and `cargo test -p reticulumd --lib --bins`. Clippy uses
`-p reticulumd -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings`;
diagnostics/architecture/boundary/size owners are unchanged.

## Item 4 LoRa/RNode opening-status and supervisor stop checkpoint

- Structural checkpoint `581e9b52` precedes behavior. Two native owned missing
  serial/refused TCP status cases fail before error publication (0/2 in 2.00
  seconds), then the LoRa suite passes 20/20 in 0.16 seconds. One private helper
  publishes contextual existing `last_command_error`/offline status and logs
  it without credentials; successful startup retains its existing reset owner.
  Logs: `/tmp/lxmf-item4-lora-opening-status-{red,green}.log`.
- With failure publication as a deterministic retry barrier, serial/TCP crossed
  with independent global/interface stops fail all four two-second joins while
  the existing backoff stays sixty seconds. Two active-stream controls then
  produce one pass (global control) and one interface-stop failure. Logs:
  `/tmp/lxmf-item4-lora-retry-shutdown-red.log` (0/4, 2.01 seconds) and
  `/tmp/lxmf-item4-lora-active-stop-red.log` (1/2, 2.00 seconds).
- Existing native connect/retry waits now select both stops. The private stream
  context retains interface stop; it cancels the existing child token, then
  awaits the same KISS future and the already-owned probe monitor. There is no
  new worker/API. Active controls observe actual native initialization bytes
  as an entry barrier, peer EOF and weak interface-owner release. Their existing
  configurable startup guard is sixty seconds so monitor expiry cannot mask
  the targeted stop. It is not an independent protocol-conformance proof or a
  production timer change. Backoff cases prove actual failed attempts and
  pending retry cancellation; no stalled TCP connect syscall is claimed.
- The first post-change command fails to compile because two old private test
  contexts lack the added stop field; retained
  `/tmp/lxmf-item4-lora-supervisor-green.log` is not passing evidence. Filling
  those initializer fields leaves old assertions unchanged (with incidental
  formatting in the existing IFAC fixture). The corrected run passes 26/26 in
  0.16 seconds in `...-supervisor-corrected.log`.
- Both bounded source POST roles approve. Affected all-target/all-feature
  Clippy, diagnostics scanner #369, architecture, boundaries, module size,
  explicit six-file formatting and diff checks pass in
  `/tmp/lxmf-item4-lora-supervisor-{clippy,diagnostics,architecture,boundaries,size,fmt,diff}.log`.
  At this checkpoint, shared KISS pressure/cleanup, generic bearer and configured
  serial/TCP traces remained pending. It qualified neither pressured retirement nor
  the LoRa/RNode family row; thirteen of twenty rows remain locally qualified,
  with seven families and whole-item integration/hosted/merge gates open.

Commands: focused red filters `lora::tests::supervisor`, `retry_backoff_stops`
and `lora_active_stream_stops` with
`cargo test -p reticulum-rs-transport --lib`; final native command
`cargo test -p reticulum-rs-transport --lib iface::lora::`.
Clippy uses `-p reticulumd -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings`;
the diagnostics command is `cargo test -p reticulumd --test code_quality_issue_369`.

## Item 4 LoRa/RNode software family preparation

- Weave checkpoint `92627fa3` qualifies thirteen of twenty software rows.
  Read-only exploration maps the next LoRa/RNode row to the existing configured
  dispatcher/state guard, native serial/TCP/KISS supervisor, RNode status/control
  projection and caller-owned generic bearer. Defaults remain IFAC eight,
  ordinary MTU 220 and Python RNode MTU 508; no virtual-child case applies.
- The next TRANSPORT.md board declares mechanical preparation before behavior,
  two opening-status regressions followed by four long-backoff stop regressions
  and two active-stream controls for independent stop inputs,
  nine shared-KISS pressure cases plus normal shutdown/initial-failure controls, two configured
  serial/TCP traces and bounded fake-bearer attempts for both existing kinds.
  At the preparation baseline, opening errors only logged and retry waits/TCP
  connect could hide cancellation; the subsequent supervisor checkpoint above
  corrects those gaps. Shared KISS pressure was documented next and is qualified
  separately above; generic bearer pressure/final status remain open. Platform
  backend reads stay single-owner.
- Correctness PRE approves mechanical preparation and requires initial KISS
  write failure to retain its no-cleanup path. Independent global/interface-stop
  controls must cancel, then await the existing stream and probe monitor owner.
  Both PRE roles now approve these fences; terminal bearer offline status must
  preserve prior error/traffic and append a close failure.
- Mechanical preparation extracts the existing LoRa spawn/offline helper and
  management/status suffix into private includes, keeping its main types/methods
  at 447 lines, supervision 163 and management 137. Existing LoRa tests move
  unchanged inside the same tests-module wrapper to tests/core (459 lines),
  reducing module_extra to 171. The existing generic KISS stream moves to a
  380-line include, leaving its original file 465. Only three obsolete size
  allowances are removed; untouched prelude/radio-status exceptions stay intact.
- Rustfmt-normalized reconstructed source matches the prior HEAD exactly.
  The first comparison helper omitted two boundary blank lines; the failure's
  diagnostic diff is retained in
  `/tmp/lxmf-item4-lora-structure-comparison-initial.log`. Corrected comparison
  passes in `...-structure-comparison.log`, without any production correction.
  Before/after native suites pass LoRa18/18 (0.16 seconds), KISS4/4 (0.04),
  generic bearer5/5 (5.02/5.03) and configured Serial/KISS4/4 (6.08/6.07).
  Logs remain in `/tmp/lxmf-item4-lora-{native,kiss,bearer,pty}-{baseline,structure}.log`.
- Both structural POST roles independently reconstruct and approve unchanged
  bodies, public paths/privacy and test names. Affected all-target/all-feature
  Clippy, architecture, boundaries, module size, explicit nine-file formatting
  and diff gates pass in `/tmp/lxmf-item4-lora-structure-*.log`.
  That structural checkpoint changes no LoRa/RNode behavior or qualification;
  all seven remaining families/whole-item gates stay open. Physical/RF/public-network
  work is outside this software scope.

## Item 4 Weave configured PTY lifecycle qualification

- Two Linux cases use only the shared owned PTY observer and loopback TCP.
  Strict missing-alias preflight reports a redacted failed record with no native
  root/runtime-status handle or control binding. Permissive startup retains an offline/reconnecting root
  with prepared native sixteen-byte IFAC and default MTU 1,024/baud 3,000,000.
- The native PTY trace independently verifies DISCOVER addressing, signed
  response and CONNECT signature; connection logs and published WDCL state
  establish readiness after port open. Counted wrong-key/plaintext rejection
  precedes exact authenticated endpoint ingress and endpoint-addressed egress.
  Existing/new children inherit rotated IFAC, mode, outgoing=false, gravity
  and pacing; outgoing is deliberately restored for wire tests. A two-second
  first-byte check rejects every extra byte or EOF after each direct response.
- Alias removal before PTY closure prevents reopening a recycled slave. Old
  children disappear while the same root and unrelated TCP remain; a new owned
  PTY recovers with rotated policy under the unchanged five-second native retry.
  Remote-display RPC acknowledgements are followed by exact command bytes on
  the PTY. Joined owner shutdown closes saved control receivers/bindings and
  unrelated sockets; observers reach EOF and successful explicit child waits.
  Full restart uses a new Transport identity/final credential, explicit MTU 512,
  fresh counters/learned state and new exact traffic/control observations.
- The first configured run passes 2/2 in 32.11 seconds. Both source POST roles
  request tightening the quiet observer (no complete frame -> no byte), and the
  correctness role limits already-emitted restart discovery to two seconds;
  only missing-device recovery uses eight seconds. Both approve the final
  source. Final focused default/all-feature runs pass 2/2 in 32.10/32.12 seconds,
  retained in `/tmp/lxmf-item4-weave-configured-{initial,tight-default,tight-all-features}.log`.
- Default affected suites pass 1,035 transport tests (twelve existing ignores)
  and 31 library/678 binary daemon tests. That daemon binary precedes the quiet
  assertion tightening; the final focused default run above covers it. Separate
  all-feature suites pass 1,064 transport tests (twelve existing ignores) and
  31 library/679 binary daemon tests. Affected all-target/all-feature Clippy,
  default check, architecture, diagnostics scanner, boundaries, module size,
  explicit seven-file formatting and diff checks pass. Logs are
  `/tmp/lxmf-item4-weave-{transport,daemon}-{default,all-features}.log` and
  `/tmp/lxmf-item4-weave-{clippy,check-default,architecture,diagnostics,boundaries,size,fmt,diff}.log`.
- This qualifies the thirteenth declared software family row only. Seven
  families and whole-item integration/one PR/hosted CI/merge remain open. No
  physical Weave device, RF or public-network evidence is claimed; accepted
  but unused backoff knobs remain unchanged and are not fixture shortcuts.

Configured commands: `cargo test -p reticulumd --bin reticulumd policy_tests::weave_family`
and the same with `--all-features`. Affected commands are
`cargo test -p reticulum-rs-transport --lib`,
`cargo test -p reticulumd --lib --bins`, and separate `--all-features` variants;
the previously retained combined-package cipher-feature failure is not rerun or
reported as passing. Clippy uses the same affected packages with
`--all-targets --all-features --no-deps -- -D warnings`; other focused commands
retain their existing architecture/diagnostic/boundary/size owners.

## Item 4 Weave native policy, ownership and blocked-operation corrections

- From mechanical checkpoint `95139214`, three native policy/interrupted-owner
  cases fail before correction (0/3 in 0.00 seconds), and five blocked RX,
  discovery write/flush and packet write/flush cases fail their two-second
  joins (0/5 in 2.03 seconds). Retained logs are
  `/tmp/lxmf-item4-weave-endpoint-native-red.log` and
  `/tmp/lxmf-item4-weave-shutdown-red.log`. An earlier endpoint fixture compile
  error comparing collection types is in `...-endpoint-red.log`; it is not
  acceptance evidence.
- Remove the redundant outgoing override after native child registration.
  Select the existing stop tokens around the scoped I/O operation, keeping
  normal inner stop selection and mandatory awaited cleanup outside it. Cleanup
  reconciles the existing manager's interface tree minus its parent, catching
  registration/GC interruptions without telemetry sampling or another registry.
  Native children retire while the parent and unrelated tree remain live.
- The first corrected native run passes 29/30. Its packet-flush fixture used
  the manager's parent stop token, so stopping a child also garbage-collected
  that already-cancelled parent. The fixture now supplies an independent stream
  stop token and retains/asserts the live manager parent token. No production
  change is attributed to this fixture correction. Logs remain in
  `...-native-green.log` and `...-native-corrected.log`; the latter passes 30/30
  in 0.10 seconds. After the maintainability review selects the existing tree
  helper, `...-native-tree-helper.log` also passes 30/30 in 0.10 seconds.
- Both native POST roles approve, with that small helper change from the
  maintainer. At this native-only checkpoint, two configured Linux PTY cases
  were undergoing their declared first run/source reviews; family totals were
  still twelve of twenty. The later configured qualification is recorded above.

Native command: `cargo test -p reticulum-rs-transport --lib weave` (the focused
red runs used the `weave::tests::endpoint_lifecycle` and
`weave::tests::blocked_shutdown` filters).

## Item 4 Weave mechanical preparation and behavior board

- Meshtastic checkpoint `b1e34761` qualifies twelve of twenty software family
  rows. The next Weave board in TRANSPORT.md retains configured/native owners,
  default sixteen-byte IFAC, 1,024-byte MTU, five-second reconnect and software
  PTY-only scope. Both PRE roles approve mechanical preparation before behavior.
- Weave's existing code moves into private same-namespace includes: root 426
  lines, status 466, stream 239, frame processing 342 and tests/core 1,127.
  Public paths, exact test names and bodies stay unchanged; the IFAC include
  path adjusts to its new source directory. The obsolete root allowance is
  removed. The existing serial PTY script/owner/reaping helper moves to one
  shared private test module with narrow parent visibility.
- Before/after native Weave tests pass 22/22 (0.12/0.11 seconds); four
  Serial/KISS tests pass both times (6.07 seconds each). Output remains in
  `/tmp/lxmf-item4-weave-{native,pty}-{baseline,structure}.log`. Expanded-source
  Rustfmt normalization confirms all Weave and PTY helper bodies are unchanged;
  affected all-target/all-feature Clippy, architecture, boundaries, size,
  explicit formatting and diff checks pass in `...-structure-*.log`.
- Behavior PRE adds two ownership controls to the planned policy/five pressure
  cases: registration can yield after native creation/before private indexing,
  and GC can yield after deindexing/before native retirement. The existing
  manager's parent-child relationships must govern final cleanup of this
  uniquely owned parent, preserving unrelated children. No behavior changes
  or family promotion are part of this structural checkpoint. Two configured
  PTY cases and the eight native behavior scenarios remain future work.

Structural commands: `cargo test -p reticulum-rs-transport --lib weave`,
`cargo test -p reticulumd --bin reticulumd policy_tests::serial_family`,
`cargo clippy -p reticulumd -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings`,
`cargo run -p xtask -- architecture-checks`, the boundary/module-size scripts,
and the explicit eight-file `rustfmt --edition 2021 --config skip_children=true --check`.

## Item 4 Meshtastic software lifecycle and native queue shutdown qualification

- Auto checkpoint `162db200` qualifies eleven of twenty declared software
  family rows. The next board in TRANSPORT.md keeps Meshtastic explicitly
  simulated, using the existing configured dispatcher, native worker and
  unique mutation/auxiliary owner. Both PRE roles approve; review corrects the
  native default to sixteen-byte IFAC without changing production defaults.
- The configured failure/repair, multi-chunk native loopback, published status,
  live same-root key rotation, unrelated TCP and changed-identity/key restart
  baseline passes 1/1 in 6.05 seconds in
  `/tmp/lxmf-item4-meshtastic-configured-initial.log`.
- Two full native RX/output queue regressions retain their 0/2 pre-fix result
  in 2.01 seconds in `...-shutdown-red.log`. Manual polling drives the real
  native future to the pending handoff, then a Tokio task owns completion.
  Exact queues stay held through independent stop-token cancellation and join;
  input ownership closes without cancelled admission/output.
- Selecting existing stop tokens around receive processing and outbound send
  makes both pass alongside three existing independent bearer IFAC controls:
  5/5 in 0.02 seconds in `...-native-green.log`. Both Meshtastic source POST
  roles approve. Default affected suites pass 1,027 transport tests with twelve
  existing ignores and 31 library/676 binary daemon tests in `...-default.log`.
- The all-feature transport sequence stops at one existing Auto failed-startup
  immediate exclusive-bind assertion: 1,055 pass, one failure, twelve existing
  ignores in `...-transport-all-features.log`. Later commands did not run. The
  failed run is retained. This natural failure is untraced, consistent with the
  preceding controlled Auto/Pipe descriptor attribution rather than individually
  PID-attributed here. Local socket vectors drop on the data-bind error before
  runtime workers spawn. A bounded PRE amendment reuses existing exact-case
  subprocess isolation for all three old Auto exclusive-bind lifetime cases,
  sharing the existing private helper with the two pressure cases. Preserve all
  assertions/deadlines/bind probes and declare one subsequent normal full run.
  Both PRE roles approve. The three contiguous cases move into a 342-line
  `tests/socket_lifecycle.rs` included in the same module, retaining exact case
  names and reducing the old core test file from 1,448 to 1,105 lines without
  adding an allowance. A Rustfmt-normalized comparison confirms the three
  original bodies differ only by the isolation entry guards.
- The declared subsequent normal all-feature transport run passes 1,056 tests
  with twelve existing ignores in `...-transport-all-features-isolated.log`.
  Separate daemon all-feature validation passes 31 library/677 binary tests.
  Affected all-target/all-feature Clippy, default check, architecture,
  diagnostics, explicit formatting, boundaries and module-size gates pass.
  This locally qualifies the twelfth software family row, with eight remaining
  plus whole-item integration, one PR, hosted CI and merge. Configured loopback
  composition remains separate from independent native bearer authentication;
  no external Meshtastic device or RF evidence is claimed.

The checkpoint commands are:

| Command | Output log suffix / result |
| --- | --- |
| `cargo test -p reticulum-rs-transport --lib iface::meshtastic::blocked_shutdown -- --nocapture` | `shutdown-red.log`, zero/two pass before fix |
| `cargo test -p reticulum-rs-transport --lib meshtastic -- --nocapture` | `native-green.log`, five pass |
| `cargo test -p reticulumd --bin reticulumd configured_meshtastic_failure_rotation_and_restart_preserve_simulated_delivery -- --nocapture` | `configured-initial.log`, one pass before fix |
| `cargo test -p reticulumd -p reticulum-rs-transport --lib --bins` | `default.log`, pass |
| `cargo test -p reticulum-rs-transport --all-features --lib` | `transport-all-features.log`, one Auto lifetime failure; `transport-all-features-isolated.log`, pass after harness correction |
| `cargo test -p reticulumd --all-features --lib --bins` | `daemon-all-features.log`, pass |
| `cargo clippy -p reticulumd -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings` | `clippy.log`, pass |
| `cargo check -p reticulumd` | `default-check.log`, pass |
| `cargo run -p xtask -- architecture-checks` | `architecture.log`, pass |
| `cargo test -p reticulumd --test code_quality_issue_369` | `diagnostics.log`, pass |

Suffixes use `/tmp/lxmf-item4-meshtastic-`. The explicit eight-file
`rustfmt --edition 2021 --config skip_children=true --check` result is in
`fmt-final.log`; boundary/module-size script output is in `boundaries.log`
and `size-final.log`. Local task-document links and `git diff --check` pass.

## Item 4 Auto software lifecycle and receive-owner qualification

- `60f756a7` commits reviewed I2P and shared HDLC behavior; ten of twenty family
  rows are locally qualified. The Auto board in TRANSPORT.md declares two Linux
  configured traces using the existing manual-loopback plan hook, with empty
  candidates/adopted devices, `lo` filtering and exact-case subprocess isolation.
  Host OS discovery and physical multicast ingress remain outside this evidence.
- Correctness PRE adds two native receive-owner blocked-handoff regressions.
  Both revised PRE roles approve. A real UDP worker, valid nonempty packet and
  native queue/status barriers retain full RX/event queues through shutdown,
  join and exclusive socket rebind before manager retirement. The sole bridge
  is worker-owned. The native 1,920 ms gate remains unchanged.
- `/tmp/lxmf-item4-auto-family-receive-red.log` has two joined-shutdown failures
  in 3.94 seconds. Selecting existing watch shutdown around forwarding and both
  event sends makes both pass in `...-green.log` (1.93 seconds). Both source
  POST roles approve the focused correction; all sixty native Auto runtime
  tests pass with all features in `...-runtime.log`.
- Both configured traces are implemented. The initial compile preserves the
  private-field error; the fixture now uses the public initial-announcement
  accessor. The first native run reaches traffic and live policy but fails an
  incorrect fixture expectation of zero adopted devices. Native status seeds
  that counter from manually supplied data listeners, so it reports one `lo`
  listener while the plan's candidate/adopted lists remain empty. No production
  status behavior changes. The corrected run passes 2/2 in 8.00 seconds in
  `/tmp/lxmf-item4-auto-family-configured-corrected.log`; prior results remain
  in `...-initial.log` and `...-native.log`.
- Both full-source POST reviews approve. Default affected suites pass 1,025
  transport tests (twelve pre-existing ignores) and 31 library/675 binary daemon
  tests. The combined package all-feature run passes 1,054 transport tests with
  twelve existing ignores, but retains sixteen daemon propagation-decryption
  failures. It enables transport AES-128 independently of core AES-128; both
  manifests/cipher implementations are unchanged from `60f756a7`. No prior-head
  execution of that combined command is claimed. Independent review agrees
  with this source-grounded classification. The standalone daemon all-feature
  command then passes 31 library/676 binary tests, including all sixteen cases.
  See `/tmp/lxmf-item4-auto-family-{default,all-features,daemon-all-features}.log`.
- All-feature affected-package Clippy, default check, architecture, diagnostics,
  formatting, boundaries and module-size gates pass. Output logs remain in
  `/tmp/lxmf-item4-auto-family-{clippy,default-check,architecture,diagnostics,fmt,boundaries,size}.log`.
  At the Auto checkpoint, eleven of twenty declared software rows were locally
  qualified, with nine remaining and whole-item integration, one PR, hosted CI
  and merge still open. No OS enumeration, native multicast ingress, physical-carrier or
  public-network qualification is inferred.

The exact Cargo command scopes for this checkpoint are:

| Command | Output log suffix / result |
| --- | --- |
| `cargo test -p reticulumd -p reticulum-rs-transport --lib --bins` | `default.log`, pass |
| `cargo test -p reticulumd -p reticulum-rs-transport --all-features --lib --bins` | `all-features.log`, transport pass; daemon sixteen failures |
| `cargo test -p reticulumd --all-features --lib --bins` | `daemon-all-features.log`, pass |
| `cargo test -p reticulum-rs-transport --all-features --lib iface::auto_runtime::` | `runtime.log`, sixty pass |
| `cargo clippy -p reticulumd -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings` | `clippy.log`, pass |
| `cargo check -p reticulumd` | `default-check.log`, pass |
| `cargo run -p xtask -- architecture-checks` | `architecture.log`, pass |
| `cargo test -p reticulumd --test code_quality_issue_369` | `diagnostics.log`, pass |

All suffixes use `/tmp/lxmf-item4-auto-family-`. The explicit five-file
`rustfmt --edition 2021 --config skip_children=true --check` invocation,
`tools/scripts/check-boundaries.sh`, `tools/scripts/check-module-size.sh`,
49 task-document link checks and `git diff --check` also pass.

## Item 4 I2P lifecycle and shared stream shutdown qualification

- Structural checkpoints `8715367b` and `c01d294d` preserve the I2P and shared
  HDLC orchestration before behavior edits. Both PRE and structural POST roles
  approve. The main agent implements all changes; immutable references and
  public forwarding APIs remain unchanged.
- Six configured fake-SAM cases retain their 2-pass/4-fail baseline in
  `/tmp/lxmf-item4-i2p-family-native-red.log`, then pass 6/6 in 8.35 seconds in
  `...-green.log`. Three incoming SAM sockets survive pre-fix joined root
  shutdown, and malformed DEST REPLY exposes an owned private value. Owned
  root/accept task sets, cancellation-aware setup and value-free errors fix
  those failures. The outgoing lifecycle and actual native-queue pressure
  teardown were already passing controls.
- The incoming trace proves actual private-key file persistence, default
  sixteen-byte IFAC rejection/exact traffic, old/new accepted-child rotation
  and isolated replies, preserved active children across session recreation,
  changed-key restart/native MTU and unrelated traffic. Recovered initial
  persistence/error metadata remains a bootstrap snapshot; only the actual
  file proves recovery. Restart preparation has a new identity-bound key path.
- Correctness POST blocks qualification on established shared-stream I/O.
  Five declared RX admission, packet write/flush and keepalive write/flush
  regressions retain blocked resources through cancellation and require joined
  completion, EOF and no false successful-output event. An initial empty-packet
  RX fixture gives a false pass and is explicitly excluded from coverage.
  The valid nonempty roundtrip guard gives 0/5 passing in
  `/tmp/lxmf-item4-i2p-hdlc-valid-red.log`; cancellation-selected shared handoffs
  give 5/5 in `...-green.log`. Existing token, queue, watchdog, framing and IFAC
  behavior remains. Partial writes retire with that stream generation.
- Both final source correctness and maintainability POST roles approve. The
  first broad daemon run exposes an old raw-SAM-error expectation; its updated
  regression requires safe HELLO context and absence of an owned private marker.
  A fixture loop counter lint and shortened-error formatting are corrected;
  initial logs remain and no gate/deadline is relaxed.
- Final default suites pass 1,023 transport tests with 12 existing ignores and
  31 library/673 binary daemon tests. All-feature suites pass 1,052 transport
  tests with the same ignores and 31 library/674 binary daemon tests (33.98
  seconds for the binary suite). All-target/all-feature transport/daemon Clippy,
  default checks, diagnostics scanner, architecture, Cargo formatting,
  boundaries and module size pass. Logs are
  `/tmp/lxmf-item4-i2p-family-*-final.log` and `*-all-features.log`.
- Live GitHub still reports the same 22 open issues and no PR for this branch
  in `/tmp/lxmf-open-issues-item4-i2p.json`. Ten of twenty software family rows
  are locally qualified. Ten other families, whole-item integration/release
  checks, one PR, exact-head hosted CI and merge remain open. This is fake-SAM
  and bounded stream software evidence, with no prepared-router, public-network,
  hardware or whole-issue completion inferred.

## Item 4 shared HDLC shutdown preparation

- I2P's six configured traces pass after its initial ownership/redaction fix,
  but correctness POST finds established shared HDLC I/O can still block the
  new mandatory joins. Qualification remains withheld. Both PRE roles approve
  five additional blocked-handoff regressions and a minimum shared-owner fix.
- Main mechanically extracts the existing shared function into a private
  349-line module, preserving its crate-visible path/signature through a
  re-export. Both structural POST roles approve exact behavior preservation;
  40 existing TCP-client tests pass in 6.00 seconds in
  `/tmp/lxmf-item4-i2p-hdlc-refactor.log`. Blocked-I/O correction remains pending.

## Item 4 I2P orchestration preparation

- Both PRE roles approve the finite I2P board in TRANSPORT.md. Existing native
  I2P tests pass 21/21. New configured regressions retain two passes/four failures
  in `/tmp/lxmf-item4-i2p-family-native-red.log`: outgoing lifecycle and pressured
  shutdown pass; native incoming destination/session/accept sockets outlive joined
  root teardown, and malformed DEST REPLY leaks an owned private value to status.
  Fixture compilation and empty-filter mistakes are retained separately and are
  not runtime evidence.
- Main mechanically extracts root/accept orchestration into private 121/255-line
  modules. Public spawn and test compatibility wrappers remain unchanged. Both
  independent structural POST roles approve exact behavioral preservation.
  Existing I2P tests remain 21/21 green, default transport check, formatting,
  module size and diff checks pass. Behavior correction and both I2P inventory
  rows remain pending; no new family qualification or PR is claimed.

## Item 4 Serial and KISS software family traces

- Checkpoint `61cf99e0c79793a28abdbaec2edfc7d51fc888ca` records Pipe composition.
  Both PRE roles approve four Linux software cases for three inventory rows.
  Main-agent implementation adds one 506-line module and Linux registration,
  reusing configured bootstrap, native retry/IFAC owners and unrelated TCP helpers.
  No production behavior/default/API/dependency changes or unsafe Rust.
- Serial, KISS and AX.25 KISS begin with absent owned device aliases and real
  native open-error status. Raw owned PTYs permit same-root activation, eight-byte
  IFAC authentication and live rotation. Closing the old master requires a new
  open-error increment before publishing a fresh PTY; reopen retains the key.
  KISS TCP instead uses a bound, unlistened socket's refusal, then listens on the
  same socket and retains its root/key through EOF/reconnect. Every phase counts
  wrong-key/plaintext rejection before exact authenticated ingress/egress.
- Fixed independent KISS initialization bytes are consumed on every open,
  reconnect and restart. AX.25 uses the pinned APZRNS/N0CALL-0 sixteen-byte UI
  header around IFAC data; no address-validation qualification is claimed. Full
  changed-key configured restart proves old-tree retirement, fresh Transport
  identity, native MTU 512/1024, sanitized status and joined cleanup. Single-root
  transitions retain unrelated TCP addresses and exact traffic.
- The test-owned Python helper closes its extra slave descriptor, relays raw
  bytes with complete writes and preserves buffered greeting/wire bytes. Expected
  EIO remains responsive to stdin EOF; stdout EOF and successful Child wait prove
  helper cleanup. Alias removal is tightened to precede master closure while the
  authenticated native descriptor is open, preventing another concurrent test's
  reused slave name from being opened by a retry.
- `/tmp/lxmf-item4-serial-family-initial.log` retains a fixture reader/writer
  tuple compilation error. Accepting the existing capabilities by value handles
  both borrowed PTY streams and TCP halves; no new adapter or behavior change.
  The first native run passes 4/4 in `...-native-initial.log` (6.07 seconds), and
  final alias isolation passes 4/4 in `...-alias-isolated.log` (6.08 seconds).
  Both independent source POST reviews approve. No production defect/pre-fix
  runtime red is claimed.
- Full daemon validation passes 31 library and 668 binary cases in
  `...-daemon.log` (32.61 seconds for binary tests). All-target/all-feature daemon
  Clippy, default check, architecture, diagnostics scanner, Cargo formatting,
  boundaries and module size pass with corresponding
  `/tmp/lxmf-item4-serial-family-` suffixes. Existing baseline warnings remain.
- Eight of twenty software family rows now have their declared evidence. Twelve
  others, whole-item integration/release checks, one PR, exact-head hosted CI and
  merge remain open. This is Linux software evidence, with no Windows, hardware,
  RF, public-network or whole-issue completion inferred.
