# Remaining Link and Resource owner cutover

This is the implementation map for the remaining Link portion of item 3 in
[PLAN.md](PLAN.md), not a new goal or a smaller acceptance scope. It follows the
retained opportunistic owner at `785f97eb`. All work remains in the single item-3
PR; no additional PR is authorized by this document.

**Intent:** Complete direct/propagated delivery through the existing admitted
message owner, using the pinned Python contract and explicit Rust extensions.
**Current behavior:** The daemon waits up to 20 seconds for Link activation while
holding execution permits, releases admission on packet/Resource handoff, and
terminalizes several outcomes that the target retries. Propagation always uses a
Resource and packet proofs currently mean delivered without a method distinction.
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

`crates/libs/test-support/fixtures/lxmf_1_2_0_link_traces.json` records 42 scenarios
and 226 snapshots from the pinned production `handle_outbound`, `process_outbound`,
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

Primary files are `bridge_delivery_completion.rs`, `bridge_link_send.rs`,
`bridge_link_dispatch.rs`, `bridge_propagation.rs`, the Direct/Propagated task
sections, `outbound_resources.rs`, `receipt_bridge.rs`, the existing inbound
Resource consumer and the narrow transport Link acquisition/state APIs. Extract
helpers where the active module-size gate requires it. Keep unrelated CLI,
hardware, release and sibling repositories out of this cutover.

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
