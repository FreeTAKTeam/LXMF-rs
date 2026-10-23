# Retained Link and Resource owner cutover

This is the implementation map for the remaining Link portion of item 3 in
[PLAN.md](PLAN.md), not a new goal or a smaller acceptance scope. It follows the
retained opportunistic owner at `785f97eb`. All work remains in the single item-3
PR; no additional PR is authorized by this document.

**Intent:** Complete direct/propagated delivery through the existing admitted
message owner, using the pinned Python contract and explicit Rust extensions.
**Current behavior:** The scheduler retains the admitted message, immutable bytes,
exact Link and native packet/Resource observations through retry, transfer and
persistence. Pending work releases execution capacity. The old activation sender,
detached monitors and duplicate outbound Resource publisher have been removed.
**Expected outcome:** One prepared payload, continuous attempt budget, exact Link
context and authoritative transfer outcome remain owned through activation,
transfer, recovery and durable completion. Other peers can use execution capacity
while this owner waits.
**Truth owners:** Scheduler admission/preparation; Transport Link/Resource state;
existing receipt publisher and receipt worker for accepted durable completion.
**Contract boundary:** Typed Link/transfer observations return to the admitted
owner. They do not independently create fallback work or a second receipt writer.
**Cutover:** Replace the fixed activation wait, repeated Link selection, detached
per-transfer monitors and handoff-as-completion in the delivery path. Keep shared
transport APIs used by unrelated clients intact.
**Evidence:** Exact reference traces, real Transport ingress and recipient proof,
SDK/ZeroMQ durable status, storage faults, cancellation and bounded overload.
**Plan review gate:** PRE review the implementation boundary before edits;
correctness and POST/maintainability reviews before integration. The reference
artifact checkpoint has passed its own bounded PRE/POST review.

## Reference evidence and limits

`crates/libs/test-support/fixtures/lxmf_1_2_0_link_traces.json` records 47 scenarios
and 263 snapshots from the pinned production `handle_outbound`, `process_outbound`,
`LXMessage.send` and registered callback methods. The parent generator verifies
clean exact reference commits and import roots. Link/Resource I/O, measurements,
clock and thread scheduling are controlled; these traces are not native watchdog,
cryptographic proof, Resource cancellation, or mixed-peer delivery evidence.

The oracle covers normal/slow pathless exhaustion, known-path slow closure,
changing medium timing, activation beyond 20 seconds, never-activated versus
activated closure, timeout/retry, late proof, active outbound/backchannel priority,
Resource outcomes, oversized Opportunistic escalation and sequential multi-message
reuse of one Link. Propagated shared-Link messages have different final recipients.
Cancellation inputs set message state; they do not invoke `cancel_outbound`.

The Rust adapter completes native inputs before the next controlled decision:
proof rows await the real retained packet token, and live signal rows await the
exact Link/context/plaintext Data event emitted after native signal decoding.
Enqueueing a frame or yielding a fixed number of times is not input readiness.
The old Closed generation remains an explicit no-op projection after its observer
and keys have been retired. These barriers do not select expected validity or
inject delivery outcomes.

Preserve these observed distinctions in the implementation and its tests:

- Attempts count no-Link creation/discovery decisions. Pending/active Links do not
  consume another attempt merely because a scheduling deadline passes. With a
  stable limit of five, only attempts one through four create/request; the fifth
  fails. Slow limit three permits two such attempts.
- Pending waits use native Active/Closed state. Active + SENDING does not resend.
  An existing direct Link precedes an inbound backchannel, even when direct is
  still pending. An existing active Link can send without consuming an attempt.
- Direct closure requests a path each time an activated Link closes, but only
  once per message for never-activated Links. Propagation closure does not issue
  that Direct-specific request. Both reschedule only when attempts + 1 < limit.
- Direct Resource complete means DELIVERED; rejected means REJECTED; other failure
  tears down and returns OUTBOUND. Propagation complete means SENT/custody and
  removes the message from pending; rejection/failure return OUTBOUND.
- Packet timeout resets OUTBOUND and tears down; a send exception fails the
  message. Returning no receipt instead closes the Link without resetting
  SENDING in this pin. External closure without a timeout/resource callback also
  leaves SENDING after replacement. Do not silently treat these distinct cases
  as identical or claim an invented recovery policy is reference behavior.
- Invoking proof callbacks directly after cancellation or completion can invoke
  callbacks/change Python state again. These injected callbacks do not prove real
  Transport delivers them. Rust's existing durable terminal monotonicity and one
  accepted terminal result remain stronger required boundaries.
- Explicit Direct-to-Propagated fallback and local propagation custody are Rust
  policies. Preserve the configured fallback gate, committed envelope and local
  bypass; do not label these as Python router policy.

## Architecture and implementation sequence

1. **Retained Link context and acquisition authority.** Add a small context owned
   beside the existing prepared payload: exact `Arc<Mutex<Link>>`, effective
   method, actual target and outbound-identification requirement. Selection and
   creation have one authority. `AwaitLink` may be a disposition but cannot be
   an ID-only reason to rerun selection on every wake. Reconcile current daemon
   backchannel preference with Transport's direct-first selection explicitly.
   `Transport::link` and propagation cache lookup/reset/create currently span
   separate awaits; narrowly serialize acquisition by actual Link target. Never
   hold that acquisition guard through activation. A propagation-node Link is
   shared across different final recipients; resetting it also removes Resource
   state and must not race another owner's live transfer.
2. **Continuous retry owner.** Replace separate identity-discovery counters for
   this path with one retained retry state covering route/identity availability,
   Link creation, activation and closure. Compute method-specific target timing,
   including changes between attempts. Move pending waits outside execution and
   stamp permits. Use native closure and exact retained Link state, not a new
   20-second timer. Subscribe before state checks and reconcile after lag rather
   than treating broadcast notification alone as truth. Terminal/read-error and
   shutdown handling must retain the existing commit fence.
3. **Active dispatch and method-aware completion.** Reacquire execution capacity
   and recheck terminal state plus exact Link before dispatch. Direct packet proof
   means delivered; propagated proof means custody sent. Select propagated packet
   versus Resource using the target representation boundary. Preserve registration
   before transport handoff. An observed Resource hash remains a one-way transfer
   ownership boundary even if dispatch returns an error; its authoritative event
   must return to this owner before retry/fallback decisions.
4. **Resource and packet recovery.** Route mode-specific completion/rejection/
   failure and packet timeout into the same owner. Replace detached cancellation
   and propagation-signal monitors with owned work. Reconcile terminal outcomes
   after Resource broadcast lag so an admitted message cannot be stranded. Keep
   packet correlations from prior attempts until the terminal winner commits;
   a late proof must not create competing send/failure or premature release.
5. **Acceptance and cutover.** Update roadmap/matrix with behavior and evidence.
   Delete the displaced delivery-only activation path and duplicate lifecycle
   ownership. Preserve standalone transport helpers for their other consumers.
   Do not close item 3 until target/baseline mixed Python-Rust traces and successful
   SDK/ZeroMQ delivery after callback failure also pass.

Current files are `bridge_delivery_completion.rs`, `bridge_delivery_link_owner.rs`,
its context/acquisition/dispatch helpers, `bridge_propagation.rs`,
`outbound_resources.rs` and `receipt_bridge.rs`. The old `bridge_link_dispatch.rs`,
Direct/Propagated task send sections and generic discovery loop are deleted.
Shared inbound Resource and transport APIs remain with their existing consumers.
The implementation sequence above is the approved cutover map; the prerequisite
checkpoints below are historical evidence, not the current outstanding task list.

## Acquisition checkpoint

The transport prerequisite now uses optimistic selection with atomic map
publication in `Transport::link`. It rechecks pointer identity before reuse and
replacement, and revalidates Closed under a nonblocking Link lock before replacing
an existing entry. A public restart can therefore preserve the same Arc. Contended
Link waits occur outside the handler lock; only the published candidate dispatches
its first request, under the existing handler send lock. Explicit reset still
invalidates a returned handle; acquisition does not grant a lifetime lease.

This uses the existing map authority instead of a per-target mutex registry and
its additional retention/cleanup rules. The choice follows the standard
[`Arc::ptr_eq`](https://doc.rust-lang.org/std/sync/struct.Arc.html#method.ptr_eq)
identity contract and Tokio's [shared-state lock-scope guidance](https://tokio.rs/tokio/tutorial/shared-state).
Five deterministic transport regressions cover absent/Closed concurrent creators,
restart during preparation, removed-entry reuse, and contention during restart.
Four regressions failed against the preceding implementation before the fix.

The daemon propagation cache now retains every non-Closed Link, including Stale,
and delegates misses to atomic Transport acquisition without resetting existing
Links. Its age timestamp and 25-second grace are removed. The cache is a selection
hint, not a lease; explicit reset may still close a returned handle. Two real Rust
Transports preserve a pending Resource across empty/mismatched hints, then finish
two distinct raw Resource payloads with recipient bytes and sender proof events.
These are transport payloads, not final-recipient LXMF envelope acceptance.

A real 26-second test directly checks cache retention beyond the former grace
while the Link's native establishment allowance is 90 seconds, then verifies
replacement after explicit close. This does not prove automatic watchdog expiry.
The retained message Link context, continuous retry budget and transfer owner
remain pending.

## Next retained-driver boundary

PRE review selects one retained Link driver for the generic non-SINGLE branch of
`bridge_delivery_completion.rs`. Do not add an ID-only `AwaitLink` that re-enters
`run_direct`/`run_propagated`: those methods reselect Links, and the existing
`wait_for_identity` loop resets its own retry state. Replace those scheduler-owned
paths together. Keep an exact Arc, target, effective mode and outbound-identify
requirement beside the prepared payload, then borrow its mode-specific bytes.

Reuse `Transport::delivery_link` before requiring Direct identity resolution;
its outbound-first ordering also governs pending outbound versus inbound reuse.
Local propagation custody bypasses Link acquisition. Active dispatch receives the
retained context, performs no selection or 20-second wait, and returns typed packet
or Resource identity to the same owner. Pending activation and transfer handoff
must retain admission outside execution permits; neither may fall through the
existing immediate-release branch. Preserve registration before dispatch and the
observed Resource ownership boundary while replacing detached transfer monitors.

### Native Resource completion prerequisite

The existing inbound Resource consumer removes its correlation before publishing
`sent: propagated resource` through a best-effort nonterminal receipt. A full
receipt queue can drop that notification. Retaining admission while merely waiting
for that persisted status would strand the owner. The cutover must instead consume
the authoritative native result and reliably commit method-specific completion.

`Transport::send_resource_observed_with_completion` now registers a narrow
`ResourceCompletion` under the same handler lock as `track_prepared`, before packet
dispatch. `ResourceManager::drain_events` publishes Complete/Failed/Rejected/Cancelled
to the optional observer before returning unchanged legacy events. The map uses
the logical/original hash, and removes its entry on the first terminal event.
Intermediate segment proofs do not terminate observation. Sends through existing
APIs allocate no observer; dropping observation does not cancel the transfer.

The receiver retains its final outcome after the producer is removed. A producer
that disappears without a terminal outcome returns observation-unavailable, not
an invented transport result. This uses the existing Tokio dependency's
[`watch::Sender::send_replace`](https://docs.rs/tokio/latest/tokio/sync/watch/struct.Sender.html#method.send_replace)
and [`Receiver::changed`](https://docs.rs/tokio/latest/tokio/sync/watch/struct.Receiver.html#method.changed)
instead of a second receipt writer, an unbounded result history, or a broadcast
replay mechanism. Native completion is not application persistence or custody.

The send future retains its existing cancellation limitation after registration:
the owner must drive it to completion, or drop the send future then explicitly
cancel the registered Resource before abandoning it. Do not keep driving the send
concurrently with cancellation. Pending registration can already have queued its
advertisement; cancellation guarantees local native cleanup and may emit no cancel
frame to that peer. Merely dropping a pending dispatch leaves native state
without the dispatch confirmation that activates its retry lifecycle. The driver
must preserve this boundary during cancellation and shutdown.

Five new native regressions cover registration followed by dispatch failure,
event lag, original-hash reset/reject/cancel at three transfer stages, final proof
of two real segments, dropped receivers, and producer loss. The existing two-Rust-
Transport cache test now also preserves an unread successful result through actual
failure-event overflow. These checks prove native observation, not the forthcoming
daemon owner, durable custody, mixed-language interoperability or hardware behavior.

## Retained-context Resource dispatch

Resource preparation and initial dispatch now share an Arc-taking implementation.
Existing ID APIs resolve once and delegate; the delivery helper's oversized-payload
fallback keeps its selected Arc rather than looking it up again by ID. The new
`send_resource_on_link_observed_with_completion` exposes the same registration,
failure and cancellation contract to the retained driver. Legacy sends still do
not allocate a completion observer. No new send or persistence owner is added.

A regression replaces the transport map entry after selection, then verifies the
original Link's Resource advertisement, bound interface, decryptable ciphertext
and observation-before-handoff. It failed with `InvalidArgument` before the fix.
This proves initial dispatch context, not lifecycle support for arbitrary removed
Links or two different Links sharing one ID. Native Resource routing remains keyed
by Link ID. Explicit closure/restart is not prevented by retaining an Arc.

Transport lifecycle primitives now preserve this context as well:

- `cancel_resource_on_link` validates the Resource's native Link ID before packet
  construction or mutation, including pending and later segmented transfers.
  Mismatch returns InvalidArgument without publishing a result. Native Cancelled
  observation remains authoritative when cancel-frame handoff fails; the return
  boolean describes frame handoff, including false when no frame is necessary.
- `teardown_link` closes the current Arc, removes only pointer-equal map entries,
  and publishes native Resource failures under one handler/Link critical section.
  Both operations use handler then Link try-lock, releasing handler before waiting
  on a busy Link. Captured lifecycle packets use their original interface after
  the Link lock is released. New frame-construction failure propagation still
  finalizes local closure; the existing Option-returning Link API logs failure.
- Regressions cover wrong-Link cancellation at three transfer stages, active
  inbound/outbound teardown with an unaffected replacement Resource, failed frame
  handoff, native pending closure, handler availability during Link contention and
  restart/request during blocked teardown dispatch. The latter decrypts the old-ID
  frame at the original peer. A separate native Link test checks construction
  failure through the fallible and legacy APIs; transport-level Resource publication
  in that construction-error branch is established by code inspection.

These APIs govern the Arc's current generation when their critical section starts.
Public restart/request can change the same Arc's ID. They do not clean arbitrary
earlier generations or grant a lifetime lease; the retained driver must avoid
restarting an in-flight transfer. The integrated daemon drops an interrupted send future before explicitly cancelling
its observed registration. Native completion governs cleanup; an optional blocked
frame may be abandoned after local completion, with the documented limitation.
Unresolved cleanup retains correlation through coordinated transport shutdown.

The PRE review identified these driver requirements:

- Link packet receipt timeout is `max(rtt * 6, 0.005 seconds)` in the pinned RNS
  `Packet.py:430`; expiry is strict `sent_at + timeout < now`. Expose native timing
  narrowly and test the proof race; retry deadlines and Link watchdogs differ.
- Owned propagation signal handling must replace the detached Link-ID-only
  monitor. Python binds one `link.for_lxmessage`; the current monitor can reject
  multiple messages sharing a PN Link. Preserve that signal ownership boundary.
- Retry decisions schedule delivery before testing the newly measured attempt
  limit, but closure checks `attempts + 1 < old_limit` before rescheduling. Native
  Pending ignores retry deadlines. Known-path/missing-identity readiness needs a
  guard so unrelated wakes do not burn attempts; this is a Rust adaptation.

## Storage custody prerequisite

Ordinary `sent: propagated resource` receipt text leaves a durable operation
scheduled, so restart recovery can upload it again. The trusted native path must
use `PropagationCustody::{Packet,Resource}` through the existing shared writer.
Its transaction records receipt status and journal event, changes dispatch state
to `custody`, and spends unused preparation reservation. It preserves signed bytes
and `broker_completions` capacity for a later recipient terminal outcome. Custody
is local dispatch completion, not recipient delivery or a global terminal status.
Duplicate custody and a recipient terminal already recorded are no-ops.

Preparation retains the exact maximum serialized custody-event cost, including
escaped identifiers. Restored older operations repair only a missing reservation
shortfall through ordinary logical/physical admission before exposing prepared
bytes. Identical preparation commits the same repair. Prior-run acknowledgements
repair their actual event cost atomically with custody. Full storage defers repair;
retry succeeds after capacity returns. Journal failure rolls back repair and all
custody changes together. No migration scan or second writer is introduced.

Seven focused storage tests exercise restart, exact/full-budget accounting,
duplicate/terminal ordering, fallback capacity, repair and rollback. Generic
sent-status text explicitly cannot establish trusted custody.

`RpcDaemon::record_propagation_custody` now provides the trusted native entry into
this transaction. A private, non-deserializable disposition shares the ordinary
receipt status lock and trace/ticket/event publication. Explicit ID and canonical
typed status override metadata; ordinary RPC fields cannot select custody. An
identical previously stored sent string still needs the custody transaction.
Missing messages fail before publication; storage failure emits no success trace
or event. Legacy nonbroker messages keep their ordinary receipt handling, and
later recipient terminal outcomes retain precedence. Existing post-lock event
publication does not establish a new concurrent event-ordering guarantee.

The sole receipt worker now routes typed custody to this entry and acknowledges
its committed disposition through retained observation: Stored or superseded by
a committed terminal. A failed owner closes the observation without inventing
success. Paired custody/terminal admission is atomic, and duplicate events share
pending observation. Queue admission is not commitment. Terminal commit releases
unused custody capacity while preserving any already accepted custody event.

Local propagation now returns its successful native action as a typed event to
the scheduler. After dropping execution permits, the same scheduler owner retries
only event publication and waits for the worker commit, retaining count, bytes and
membership. It never re-ingests or retransmits because publication is delayed.
Owner failure retains admission until coordinated shutdown. Shutdown after native
success still attempts reserved publication before leaving the observation wait,
so the worker can drain it. Failed scheduler enqueue releases unused reservations.

Paired queue admission is currently enabled only for a selected local propagation
target, including explicit Direct fallback. Remote Resource sends still publish
legacy statuses; reserving a new custody slot there before their cutover would
leak it after success. Extend paired admission when the retained remote owner uses
typed completion. Remote native Resource-to-custody integration remains pending.

After explicit custody commit, dispatch-owner retirement releases unused queue
permits under the publisher lock, preserving every accepted pending event and its
acknowledgement. The existing terminal fence still waits for an already accepted
terminal. Unlike transient queue permits, SQLite recipient-terminal capacity stays
reserved for a possible later proof. Keeping unused queue permits indefinitely
would eventually block unrelated sends after successful propagation. A terminal
racing retirement either consumes its reserved slot or follows existing unreserved
admission afterward; no guarantee of future queue acceptance is claimed. Acceptance
now exercises more sequential custody completions than queue capacity without any
later recipient proof, terminal events on both sides of retirement, failed terminal
persistence, shutdown drain and retained SQLite terminal capacity. A real SQLite
fault test keeps local custody pending while another peer sends over a learned
path; execution permits are available and admission remains retained until commit.

## Integrated driver status (local acceptance qualified)

The scheduler now uses the retained Link/Resource owner. The displaced sender,
detached monitors, generic discovery loop and outbound broadcast receipt publisher
are removed. Native packet meaning and Resource results stay with the owner;
remote custody uses paired reservations and the sole writer. Coordinated shutdown
can interrupt another blocked handler while preserving unresolved correlation for
transport shutdown.

The last full transport suite passed 968 tests (9 existing ignores). The current
focused owner suite passes 44 tests, including the 47-case/263-row production-owner
adapter, real packet/Resource proofs with blocked SQLite commits, creator rejection
while commit is pending, and rejection after acknowledged custody. The refreshed
full daemon suite passes 574 tests. Clippy passes for daemon and
transport across all targets/features; daemon library tests (31), native bridge
integration (2), the issue-369 scanner, boundaries, module size and architecture
checks pass.

The adapter uses declared case/event inputs, controlled measurements and decision
turns, per-token receipt expiry inputs, and explicit Error/NoReceipt producer
faults. The production retry policy, native handshakes/proofs/Resource outcomes,
admission and receipt transactions remain authoritative. It checks every shared
message, exact Link/creator selection, attempts/deadlines, request/teardown counts,
actual native handoffs, recipient bytes and immutable persisted preparation.
Propagated inputs use the oracle's unstamped envelope; native receipt at the
propagation peer does not prove stamp acceptance or final-recipient delivery.
Committed cancellation is monotonic: after cancellation, a synthetic reference
Resource callback is projected onto actual native Cancelled cleanup and its
immutable result. No second native result is fabricated. Propagated's last
attempt binding is preserved independently of the currently selected context.

The adapter reproduced a new-message Closed-cache retry before the reference
wait; selection now consumes that Closed hint once and closure retires only the
pointer-equal hint. Review also reproduced a stale Direct SENDING disposition
stalling explicit fallback; the method transition now retires the current transfer
and resets sending while prior packet proof tokens retain their original meaning.
A creator signal wins when both it and the commit acknowledgement are ready before
the owner observes them; observing the commit retires that callback. Packet and
Resource blocked-write regressions and late-signal controls cover this boundary.

Clean candidate `5a0a30f5` passes both complete target/baseline compatibility
matrices (30/30 each, no skips/ignores), including live propagation metadata and
native Resource/custody evidence. The same public SDK/ZeroMQ client passes two
real missing-peer failures, external callback error/panic and subsequent signed
native delivery, with coherent replay/status and one durable terminal per message.
The receiving-size configuration and broader accumulated gaps remain explicit in
the delta ledger. Final head `609edf12` passed independent final review/verifier
approval and all required hosted gates (20 successful checks; one deliberate
publication skip). Hosted baseline compatibility passed 30/30, PR HIL passed
23/23 on first attempts, and the independent peer gate retained its explicit
79-pass/three-known-peer-failure/two-dependency-blocked boundary. PR #673 merged
at `9c27747753488ed950c52a2c7717ddec14478786`; fetched main and the tested merge
tree were independently verified. The four approved slices are complete.
Historical prerequisite notes above describe their checkpoints;
[PROGRESS.md](PROGRESS.md) records the current integration.

## Required acceptance

- Consume every covered reference row against the production owner decisions;
  document intentional monotonic-terminal differences separately.
- Pending activation retains count/bytes/membership while another peer sends;
  shutdown/cancellation stops the owner without a leaked task or permit.
- Real Link request/proof activation after the former 20-second cutoff; native
  closure before activation; activation/closure while waiting for execution.
- Mixed identity/path/closure exhaustion without resetting budget; prepared bytes
  and propagation envelopes remain unchanged through every retry and restart.
- Concurrent same-node acquisitions yield one shared Link without destructive
  reset of an active Resource; distinct final recipients retain distinct bytes.
- One consumer for registered Resource failure; Direct delivered/rejected and
  propagated custody/retry statuses are correct and durable. Include lost/lagged
  notification recovery, accepted terminal write failure and storage recovery.
- Real recipient proof and Resource completion, late callbacks, no-receipt and
  exception distinctions, explicit fallback enabled/disabled and local custody
  with no Link request. Characterize reference SENDING stalls explicitly before
  changing their policy; successful later delivery cannot be inferred from them.
- Focused daemon/transport tests, issue-369 scanner, formatting, Clippy, boundary
  and architecture gates. Then full item-3 baseline/target interoperability and
  exact-head hosted CI/review/merge required by the main plan.
