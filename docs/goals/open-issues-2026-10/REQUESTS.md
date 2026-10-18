# Item 4 request-receipt contract

This refines [PLAN.md](PLAN.md) and [TRANSPORT.md](TRANSPORT.md) within item 4.
The routing checkpoint is `5e2321f6`; it is not another PR. Exact target RNS
`7f2b3b9b524c9386316379af1313b43a5e4f7a5d` is checked out clean in the isolated
`target/reference-prep/repos/rns-1.5.5`; canonical reference pins are unchanged.

The whole `RNS/Link.py` change from frozen `99de23c` to this target consists of
the timeout and rejection guards accepting SENT as well as DELIVERED. Packet
requests remain SENT after PacketReceipt ACK. An ACK also stops that packet's
timeout; it does not start an unconditional response timer. Request Resource
completion sets DELIVERED and starts its response timeout. The two changed guards
exclude RECEIVING. Size admission still rejects an oversized input during RECEIVING
without failing the accepted transfer. Native explicit cancellation/lifecycle,
transfer and InvalidResponse failures and immutable first terminal outcomes are
separate stronger guarantees. A conclusively completed, authenticated owned
ordinary Resource with a malformed/mismatched envelope fails InvalidResponse;
unknown legacy malformed traffic cannot conclude an owned receipt.

## Owner and cutover

- Replace Link's `pending_response_limits` decision map with one private request
  book. Each record binds the exact retained Link owner/generation and request ID,
  size policy, phase, outbound packet/original Resource identity and accepted
  response Resource identity. No global request registry or event-listener task.
- Add a native exact-Arc request send with registration before handoff and a
  generation recheck after preparation and pointer-equal native-map membership
  at registration and handoff. Select packet/Resource by packed request
  versus negotiated MDU. Packet IDs come from final encrypted packet hash;
  Resource IDs come from packed request hash. Legacy explicit-ID Resource APIs and
  packet/limit constructors retain their public signatures and delegate relevant
  state decisions to this book. Existing explicit-ID Resource APIs gain no new
  automatic response deadline; the new retained API takes an explicit timeout.
  Ordinary owned Data/None proof APIs keep their
  existing guard; request proof acknowledgement stays on the native Link owner.
- Retain request outcome through watch snapshots independently of lossy legacy
  broadcasts. A terminal record keeps only the small identity/disposition needed
  to reject reuse/late responses within that generation; large response data lives
  with retained observers. ID fences grow with distinct request IDs within one
  Link generation; no silent TTL/LRU/cap policy is introduced. Close/restart/generation replacement settles pending
  observers and clears records. Native notifications do not invoke application
  callbacks under handler, Link, cache or Resource locks.
- Authenticate/decrypt and match exact IDs before response transitions. Preserve
  the response limit across Resource advertisements/segments until completion or
  failure. Oversized-then-valid input cannot reopen a rejected request. Unknown
  legacy traffic is not authority to complete an owned request.
- ResourceManager commits correlation only after duplicate, segment order,
  assembly identity and receiver validation, before inserting the accepted
  receiver or sending its first request. Size policy runs before allocation;
  rejected or relabelled advertisements cannot donate hashes to a request.
  Progress/failure events have no request ID. Track the original Resource hash and
  each accepted active segment hash within that record, together with the Link
  generation. Apply native state changes before legacy publication. ResourceManager
  retains unpublished events across lookup awaits; per-Link drain, native application
  and legacy broadcast do not yield. Wire completion commits before reply I/O.
  Packet delivery is inferred from a valid ACK or authenticated accepted response,
  matching the child PacketReceipt; it does not imply the RequestReceipt DELIVERED phase.
  Snapshot byte counters are native active-segment progress, not Python float progress.
  Resource cleanup is scoped to that request's hashes, request ID, response kind
  and captured generation;
  cancelling one request cannot remove unrelated transfers.
- Existing Link maintenance drives deadlines. Explicit timeout/reject guards
  match SENT/DELIVERED; caller timeout/cancel and native transfer/lifecycle failure
  are separately named. No per-request spawned timer or new retry policy.
- Expose an observed receipt before handoff with no native lock held. A private
  registration guard settles an abandoned send future even when its caller never
  receives a returned token; maintenance performs scoped cleanup. Replacement,
  reset, teardown and shutdown drain old-generation cleanup before detaching its Arc.
  The existing supervisor interrupts blocked workers before native shutdown closes
  retained Link owners; pending observers cannot outlive a stopped maintenance owner.
  Handoff time is captured before Link bookkeeping can yield, and Resource sender
  activation stays under the send handler so a fast first request is served.
  Handoff cannot
  overwrite an early terminal response. Dropping an outcome wait does not itself
  cancel a successfully returned request.
- Native cancellation exposes a commit observer after releasing native locks
  and before optional frame I/O. rncp/rngit race that commit latch against the
  cancellation future, and use the existing teardown commit latch on closure.
  They can abandon optional frame sends on a full live queue after cleanup;
  elapsed time alone never substitutes for native cleanup.
- Cut rncp's request-status waiter and rngit's native request waiter over to the
  retained outcome. rncp's subsequent ordinary file Resource is a distinct
  application transfer and keeps its own waiter. rnx has no outgoing request
  constructor in the scoped source and needs no RequestReceipt client cutover.

## Evidence

First reproduce the current oversized-response removal allowing later valid
traffic. Compare target Python SENT/DELIVERED × timeout/reject, negative RECEIVING
guards, repeated callbacks, packet ACK and Resource completion state separately.
Packet response size follows Python's reencoded response-data length minus two;
Resource limits use advertisement `d` (uncompressed total size), not wire transfer
size `t`. Include exact boundaries, compressed and split Resources.

Native production-path regressions must cover both request representations and
both response representations, early response before send returns, queue/event
loss, exact owner/generation/request IDs, malformed and mismatched envelopes,
timeout/rejection before and after ACK, size rejection followed by valid replay,
close/public restart/cancel and unrelated-transfer continuity. Actual mixed-peer
request/response traces use target 1.5.5 and retain frozen baseline regressions.
Supporting gates are affected Clippy/tests, diagnostics scanner, formatting,
module size, boundaries and architecture checks. Independent PRE/POST reviews
precede the checkpoint. This does not complete item 4's other remaining rows.
