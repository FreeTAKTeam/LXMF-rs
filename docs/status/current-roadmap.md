# Current Roadmap Status

Last reassessed: 2026-06-08

This file is the repository-level source of truth for parity posture, release
confidence, and execution order. Detailed row-level status lives in:

- `docs/status/reticulum-parity-matrix.md`
- `docs/status/lxmf-parity-matrix.md`

Historical plans and issue lists explain how work was approached; they do not
override these status files.

## Current Position

LXMF-rs is a usable Rust implementation of Reticulum and LXMF with strong core
protocol coverage and repeatable interoperability against pinned Python
references. It is not yet a complete drop-in replacement for every Python
Reticulum/LXMF runtime, interface, router, and utility behavior.

The project is best described by capability level:

| Capability | Status | Meaning |
| --- | --- | --- |
| Wire compatible | achieved | Core Reticulum packet/identity primitives and LXMF message encodings are implemented and tested. |
| Direct-message interoperable | achieved | Selected bidirectional Rust/Python direct, link, channel, paper, and daemon paths are exercised in CI. |
| Propagation interoperable | partial | Propagated delivery and substantial peer/node behavior exist, but the complete Python router and peer lifecycle is not yet reproduced. |
| Operationally substitutable | partial | `reticulumd` is deployable and supports several production interfaces, but runtime, interface, and utility breadth remains narrower than Python. |
| Full Python surface parity | not achieved | Remaining gaps are tracked in the two parity matrices. |

## Strong Areas

### Reticulum

- Identity, destination, packet, cryptography, link, resource, and buffer
  behavior are the strongest RNS areas.
- Link establishment, proof validation, interface binding, watchdog timing,
  teardown, receipts, and resource lifecycle have active regression coverage.
- `reticulumd` supports TCP client/server, UDP, serial, KISS, AutoInterface,
  LoRa/RNode, feature-gated RNode BLE, and feature-gated VR-N76 KISS-over-BLE.
- AutoInterface has a live daemon runtime, including discovery, peer lifecycle,
  peer-data sockets, transport ingress, outbound routing, and multicast proof
  fallback.
- Known but unsupported Python interface families now fail config parsing with
  deterministic unsupported-family diagnostics instead of silently becoming
  inert unknown interface entries.
- `rnstatus-rs` now provides a local daemon status utility over the existing
  RPC status surface, including JSON output plus human interface runtime
  startup state and propagation peer state.

### LXMF

- Message wire/storage packing, signatures, propagation packing, paper
  encoding, timestamp precision metadata, binary-field preservation, and
  Python-compatible storage containers are implemented.
- Delivery modes are honored by the daemon; the old claim that requested modes
  are ignored is obsolete.
- Direct and propagated resource sends support receipt-state separation,
  timeout/failure propagation, and active resource cancellation.
- Ticket validity, renewal, derivation, persistence, and inbound ticket reuse
  are implemented.
- Propagation peers have real queue, policy, maintenance, throttling, peering,
  offer-response, source-accounting, and acceptance-rate behavior. These are
  substantial implementations, not SDK-only placeholders.
- Python-style propagation `auth_required` configuration now reaches
  `propagation_enable` and the daemon propagation status, so node-level
  propagation auth policy is visible with the rest of the propagation peer
  policy.
- Local and remote peer-sync offer-response cleanup now preserves peers and
  propagation queues for retry on retryable or otherwise unexpected numeric
  responses, while still treating access denial and throttling as distinct
  Python paths.
- Retryable numeric local offer responses now mirror payload-backed live queue
  marks into the active peer record snapshot before returning, so restart/export
  state preserves the retry queue even when the serialized snapshot was empty.
- Retryable, throttled, generic failed, malformed-import, and
  bridge-unavailable remote peer-sync paths now perform the same payload-backed
  live and restored queue snapshot mirroring before reporting the failed sync,
  keeping local and remote retry/export behavior aligned.
- Remote peer-sync failure events now include a structured `failure_kind` on
  the top-level event and nested propagation payload, preserving observer-level
  distinctions for throttling, identity, key, data, stamp, not-found, timeout,
  access-denied, and generic failures without changing retry behavior.
- Payload-backed remote failure snapshots now replace stale serialized peer
  queue IDs with live payload-backed marks, so bridge failures do not preserve
  obsolete restart/export work after the underlying payload is gone.
- Zero-cost peer stamp policies now sync unstamped queued offers immediately
  without waiting for absent peering metadata, matching the Python "no stamp
  required" path and avoiding repeated peer-sync postponement.
- Python propagation announce transfer and sync limits are now converted from
  advertised integer or fractional kilobytes into the byte limits used by
  peer-sync queue selection, so valid queued payloads are not misclassified as
  transfer-limited.
- Propagation peer maintenance selection now claims the chosen peer before
  invoking sync by recording the sync attempt and next backoff window, while
  allowing the internal maintenance-triggered sync to consume that claim, so
  concurrent scheduler passes cannot double-select the same peer.
- Manual `/pn/peer/sync` control requests now force an immediate peer sync
  through ordinary backoff windows, while scheduled maintenance and remote
  syncs still respect retry postponement, matching the operator-triggered
  retry path.
- Remote fetch/download/sync imports now validate the full returned propagation
  payload batch before mutating the local store or in-memory payload cache, so a
  mixed valid/invalid remote response fails without leaving partial relay state.
- Malformed remote fetch and download imports now mirror existing
  payload-backed live queue marks into active peer record snapshots before
  failing, preserving restart/export retry state for already queued relay work.
- Malformed remote fetch and download imports from an already active source
  peer now also update that peer's failure backoff and publish the failed
  peer-sync event, so invalid post-transfer payloads share retry scheduling and
  observability with transport-level remote transfer failures.
- Remote fetch and download bridge failures now mirror existing payload-backed
  live queue marks into active peer record snapshots before returning the
  failure, preserving restart/export retry state for already queued relay work.
- Remote fetch and download bridge failures from an already active source peer
  now also update that peer's failure backoff and publish the failed peer-sync
  event, so retry scheduling and observability match the preserved queue
  snapshot.
- Remote fetch and download access-denied bridge errors now preserve the
  propagation `no_access` lifecycle state instead of collapsing the denial into
  generic failure, while retaining the bridge error text for operators.
- Access-denied remote transfer cleanup now reports the stored peer identifier
  in peer-unpeer events even when callers address the remote with different hex
  casing, keeping operator-visible teardown events aligned with the peer record
  that was actually removed.
- Remote fetch and download bridge-unavailable errors now mirror existing
  payload-backed live queue marks into active peer record snapshots before
  returning and mark the propagation sync lifecycle failed, so already queued
  relay work stays restart/export visible without leaving stale lifecycle state
  when no bridge is configured.
- Successful remote fetch and download now also mirror existing payload-backed
  live queue marks into active peer record snapshots after applying imports, so
  restart/export state preserves queued retry work even when the remote
  transfer succeeds without consuming those local queued offers.
- Successful remote fetch and download now clear stale retry backoff on the
  active source peer when newly accepted payloads prove the source recovered,
  so later maintenance does not keep postponing a healthy replication peer.
- Remote peer-sync backoff postponements now mirror existing payload-backed live
  queue marks into active peer record snapshots before returning, so
  restart/export state preserves queued retry work even when sync is deferred.
- Remote peer-sync bridge-unavailable errors now mirror existing payload-backed
  live marks and restored peer-record queue IDs into active peer record
  snapshots for already known peers before returning, including
  case-insensitive requests, while still avoiding peer creation when the bridge
  is absent.
- Remote peer-sync bridge-unavailable errors for already known peers now also
  publish the failed peer-sync event and mark the propagation sync lifecycle
  failed, keeping queue retry state observable without creating new peers.
- Peer sync RPC rows and events now preserve the Python-compatible peer `state`
  namespace while exposing backoff and policy postponement through separate
  scheduling fields; failed attempts continue to use the established error state.
- Successful remote peer-sync now also mirrors existing payload-backed live
  queue marks into active peer record snapshots after applying imports, so
  restart/export state preserves queued retry work even when the remote sync
  itself succeeds without transferring those local queued offers.
- Successful remote peer-sync imports now also refresh payload-backed queue
  snapshots for all active peers affected by imported payloads, so relay peers
  preserve complete restart/export-visible unhandled queues instead of only the
  newly imported IDs.
- Remote peer-sync now uses the stored peer ID case for the bridge call, import
  source accounting, state updates, and response envelope when callers use a
  case-variant peer request.
- Failed remote unpeer attempts now mirror existing payload-backed live queue
  marks and restored peer-record queue IDs into active peer record snapshots
  before returning bridge-unavailable or bridge-execution errors, including
  case-insensitive peer requests, so restart/export state preserves queued retry
  work when peering teardown fails; these failed attempts also mark the
  propagation lifecycle failed instead of leaving stale idle/completed state.
- Access-denied remote unpeer failures now follow the same local peering break
  path as access-denied remote sync/fetch/download, clearing local peer and
  propagation queue state instead of leaving denied teardown work retryable.
- Successful remote unpeer now also uses the stored peer ID case for the bridge
  call and nested bridge result when callers use a case-variant peer request,
  keeping remote teardown identity aligned with local queue cleanup.
- Successful remote unpeer now clears stale propagation lifecycle failures and
  error text left by earlier teardown attempts, so status reflects completed
  peer removal instead of a prior failed control operation.
- Inbound reticulumd `/pn/peer/sync` and `/pn/peer/unpeer` control commands now
  resolve stored peer IDs case-insensitively before dispatching to daemon RPCs,
  so binary peer-control requests do not report not-found for restored or
  configured peers whose status rows preserve a different hex presentation;
  `/pn/peer/sync` also checks hidden unpeered peer records so operator-triggered
  rejoin paths can reach the daemon reactivation state machine.
- Payload-backed peer queue snapshot mirroring resolves stored peer IDs
  case-insensitively before reading live queue marks, so restart/export state
  preserves queued work when callers use Python-style peer case variants.
- Incremental peer queue snapshot updates also resolve stored peer IDs before
  checking completed live marks, preventing transfer-limited or handled work
  from being serialized as retryable unhandled queue state through case
  variants.
- Incremental peer queue snapshot helpers now canonicalize transient IDs before
  serializing handled or unhandled queue state, preventing padded or upper-case
  caller IDs from leaking into restart/export snapshots.
- Transfer-limited peer marks now remain terminal when later generic handled
  reports arrive, so transfer-limit retry decisions do not get reclassified as
  offered/handled work in peer queue accounting.
- Transfer-limited peer marks also remain terminal when later transferred
  reports arrive, so completed transfer-limit decisions cannot be reclassified
  as outgoing/offered work by a subsequent queue update.
- Transfer-limited peer marks also remain terminal when later received reports
  arrive, so completed transfer-limit decisions cannot be reclassified as
  incoming work by a subsequent propagation import.
- Terminal peer marks now clear case-variant unhandled rows for the same
  transient ID, so handled, transferred, received, and transfer-limited work
  cannot remain retryable under an alternate caller-case peer key.
- Peer sync unhandled transfer selection and retry cleanup now read and remove
  caller-case peer variants as one effective peer, so queued transfer work
  cannot be missed or left retryable under alternate peer casing.
- Prospective peer queue selection now also reads case-variant completed marks
  before returning unhandled work, so helper-level queue selection cannot reopen
  received, transferred, handled, or transfer-limited payloads under alternate
  peer casing.
- Static-only propagation peer replacement now routes removed static peers
  through the same local unpeer cleanup as explicit unpeer, so handled,
  received, transfer-limited, and unhandled queue marks are cleared and
  accounted consistently.
- Completed peer mark helpers now write and read received/transferred live
  marks under the stored peer ID case when a peer record already exists, keeping
  live queue state and serialized restart/export snapshots on the same peer key.
- Restored Python peer records now update their serialized queue ID snapshot
  when peer sync handles, transfers, or transfer-limits queued offers, reducing
  restart/export drift after live offer-response processing.
- Restored Python peer records now parse fractional `propagation_sync_limit`
  values through Python's integer-kilobyte restore path before peer-sync queue
  selection, preventing restored fractional sync limits from transferring work
  that Python would leave queued.
- Restored Python peer records now coerce numeric stamp, stamp-flexibility, and
  peering costs through Python's integer restore path before peering checks, so
  float-valued snapshots can still transfer queued stamped offers.
- Restored Python peer records now also coerce numeric `sync_strategy` through
  Python's integer restore path, so float-valued persistent-peer snapshots keep
  draining queued offers across sync-limit batches.
- Restored Python peer records now accept Python `time.time()` float
  timestamps for heard/sync/backoff fields, so restart-loaded peers can still
  reach queued transfer instead of failing restore before sync.
- Restored Python peer records now coerce numeric message and byte counters
  before peer-sync accounting, so restart-loaded peers keep cumulative
  offered/outgoing/incoming totals while transferring newly queued work.
- Restored Python peer records now preserve serialized LXMPeer metadata through
  Rust peer record round trips, so restart/export snapshots do not drop
  peer-specific metadata before later queue work resumes.
- Live propagation announces now retain Python PN metadata on active peer
  records, so announce-derived peer metadata survives into later peering and
  queue restart/export snapshots.
- Python-style `lxmd` `[lxmf] announce_interval` now drives peer/delivery
  announce cadence separately from `[propagation] announce_interval`, which
  remains the propagation-node announce cadence.
- Outbound propagated delivery now resolves selected propagation-node
  `propagation_stamp_cost` case-insensitively, so Python-style hash casing does
  not fall back to the default propagation stamp cost.
- Peer sync queue creation also records newly queued existing propagation IDs in
  the peer record snapshot, so postponed syncs can restart/export with the same
  unhandled queue visible in live status.
- Local peer offer-error responses now publish failed peer-sync state fields at
  both the top-level peer event and nested propagation result while preserving
  the retryable peer queue, improving parity with the peer sync state machine.
- Inbound and remotely imported propagation payloads update active peer record
  snapshots when they queue new unhandled IDs or mark source peers handled,
  keeping restart/export state aligned with live queue fan-out and source
  accounting.
- Duplicate inbound peer propagation payloads now still apply source-aware
  fan-out to active relay peers while keeping the source peer handled, so a
  known local payload does not skip relay queue creation.
- Locally delivered inbound peer propagation payloads now also store the
  accepted transient and apply source-aware relay fan-out without double
  counting source peer activity, so local delivery does not bypass relay queue
  creation.
- Inbound peer propagation ingest now also marks inactive identified sources
  as received before later activation, so newly peered sources are not offered
  payloads they supplied while still unpeered.
- Inbound propagation message-get serving now admits or refreshes the remote
  propagation peer before marking served payloads transferred, so peer transfer
  accounting is preserved even when the peer fetches before a prior offer row.
- Inbound propagation message-get serving now previews fetchable payloads and
  passes peer admission before mutating served counters, so peers rejected by
  static-only or capacity policy do not look like successful transfers.
- Inbound propagation message-get listing now also applies peer admission before
  returning non-empty payload ID lists, so rejected peers cannot enumerate
  queued transfers they are not allowed to fetch.
- Inbound propagation message-get `haves` handling now applies peer admission
  before purging matching local payloads, so rejected peers cannot delete queued
  transfers they are not allowed to acknowledge.
- Inbound propagation message-get `haves` handling now also records matched
  haves as received/completed work for the requesting propagation peer after
  purge, so reintroduced payloads are not queued back to peers that already
  declared them.
- Retained propagation payload listings now filter IDs already completed by
  the requesting peer, so `retain_synced_on_node` keeps payloads available for
  other peers without re-offering them to the peer that declared the haves.
- Link-based remote propagation downloads now wait for the final haves
  acknowledgement response after imported or duplicate payloads are reported,
  and also after all-known listings are acknowledged with purge-only haves, so
  node-side rejection or timeout is surfaced instead of reporting a completed
  download before remote cleanup is confirmed.
- Inbound propagation message-get purge-only requests now return the
  Python-style boolean success response after haves are applied, and payload
  purge cleanup preserves completed peer accounting for other peers while
  removing stale unhandled marks, so reintroduced payloads are not offered back
  to peers that already completed them.
- Inbound propagation message-get requests now mark wanted payloads skipped by
  the peer's transfer budget as transfer-limited completed work after peer
  admission, so oversized fetch attempts do not remain retryable queue entries.
- Inbound propagation message-get transfer-budget handling now leaves payloads
  skipped only by the cumulative response budget retryable for a later request,
  while still completing individually oversized wanted payloads as
  transfer-limited.
- Inbound propagation offer requests with too-short list payloads now follow
  Python's caught-exception nil response path without validating the link or
  admitting a propagation peer.
- Valid inbound propagation offers now answer Python's `False`, `True`, or
  wanted-ID list responses after peering-key validation without admitting the
  remote peer or queuing local propagation payloads before a real transfer or
  message-get admission point.
- Inbound propagation offers now validate every offered transient ID before
  applying any source-accounting marks, so malformed mixed offers cannot leave
  partial received/completed queue state behind.
- Inbound propagation offers now deduplicate validated offered transient IDs
  before building wanted-ID responses or applying source-accounting marks, so a
  duplicate offer cannot request or account the same payload more than once.
- Remote fetch and download imports now mark inactive source peers as received
  before later activation, so a propagation node is not offered back payloads it
  previously supplied just because it was not yet an active peer record.
- Remote import batches now deduplicate accepted transient IDs before applying
  peer queue and incoming-message side effects, so duplicate payloads in one
  fetch/download/sync response do not inflate peer queue accounting.
- Remote import batch byte accounting now uses the same deduplicated accepted
  IDs, so duplicate payloads in one fetch/download/sync response do not inflate
  transferred byte totals or source peer receive byte counters.
- Local propagation ingest now persists processed transient IDs separately
  from retained payload entries, so reintroduced payloads after purge or peer
  acknowledgement can refresh relay state without inflating local received or
  ingested counters.
- Propagation payload ingest now enforces the configured node message-storage
  byte limit against retained propagation entries, using age, size, and
  prioritised-destination weighting while clearing retryable peer queue marks.
- Link-based remote downloads now wait for the propagation node's `/get` haves
  acknowledgement and surface peer/control errors, so failed remote cleanup does
  not look like a completed replication drain.
- Link-based remote propagation control waits now surface authenticated
  link-close peer/control signals immediately, so denied or closed remote
  fetch/download/sync requests do not sit until the request timeout.
- Remote fetch/download acknowledgements now use canonical propagation
  transient IDs for stamped payloads, so `/get` haves purge the peer's offered
  queue entry instead of acknowledging the stamped payload bytes under a
  different hash.
- Repeated remote fetch/download/sync imports now increment source peer
  incoming counts and receive bytes only for payload IDs not already marked
  received from that source, while still replaying known payloads into relay
  queues when their live marks were cleared.
- Repeated peer-origin propagation ingests now also avoid double-counting
  source peer incoming counts and receive bytes for already received payload
  IDs, while still refreshing relay queue marks for peers that need the
  payload.
- Remote peer-sync imports now accept transferred payload arrays from full
  Python-style responses where top-level `messages` is a peer counter object
  and payloads live under `propagation.messages`/`propagation.payloads`, as
  well as legacy top-level `messages`/`payloads` envelopes.
- Propagation purge cleanup removes deleted local payload IDs from active peer
  record snapshots, so restart/export state does not retain purged queue entries
  after the live peer marks have been cleared.
- Duplicate or replayed propagation queue attempts respect already-completed
  peer marks when updating peer record snapshots, avoiding restart/export drift
  that would reopen handled IDs as unhandled.
- Duplicate or replayed propagation queue attempts also respect case-variant
  completed live marks, so a handled, transferred, received, or
  transfer-limited ID cannot be serialized as retryable unhandled work through
  the stored peer key.
- Peer sync queue replay records preexisting live unhandled marks into the peer
  record snapshot even when the store did not insert new rows, preserving
  restart/export visibility for already-queued work.
- Peer activation now also snapshots preexisting live completed marks, so
  transfers recorded before the peer record exists survive restart/export as
  handled IDs once the propagation peer is active.
- Peer activation also merges case-variant preexisting live completed marks
  into the activated peer key before queue replay, avoiding restart/export
  drift when transfer accounting arrives before the peer record case is known.
- Selected propagation node activation now reuses the existing peer record case
  before queue replay and canonicalizes merged live marks, so caller-case
  variants do not leave duplicate peer queue rows.
- Peer unpeer cleanup now clears case-variant propagation marks as one peer,
  so completed marks merged during activation cannot survive teardown and
  reappear as handled work when that peer is later reactivated.
- Peer unpeer cleanup now also removes the peer from configured static
  propagation membership, so an explicit unpeer cannot be undone by the next
  static-peer activation pass.
- Peer unpeer cleanup accounting now also reads case-variant live queue marks
  as one effective peer before clearing them, so the response and event report
  the same handled/unhandled IDs and byte totals that teardown actually
  removes.
- Reactivating a persisted `unpeered` record clears stale serialized peer queue
  snapshots before the peer becomes active again, avoiding restart/export
  resurrection of pre-unpeer propagation work.
- Reactivating a persisted `unpeered` record also clears stale live completed
  propagation marks before queue replay, so still-local payloads are offered
  again after the peer rejoins as manual or configured static.
- Persisted `unpeered` non-static records now re-run peer admission before
  reactivation, so static-only propagation policy cannot be bypassed by a
  stale teardown record.
- Static peer activation now clears stale serialized queue snapshots when it
  revives a persisted `unpeered` record, so configured static peering cannot
  resurrect pre-unpeer propagation work on restart/export.
- Reactivating a persisted `unpeered` record now also clears stale sync
  backoff postponement fields, so rejoined manual or configured static peers
  are not blocked by pre-unpeer retry scheduling.
- Peer sync reactivation now bypasses stale pre-unpeer backoff postponements
  before admission and queue replay, so manual rejoins are not returned as
  postponed `unpeered` peers.
- Peer sync reactivation now also applies the active peer type even when a
  restored `unpeered` record has a future `last_seen` timestamp, so clock-skewed
  restart state cannot leave a successfully rejoined peer marked unpeered.
- Peer sync stale queue cleanup now removes matching unhandled and completed
  IDs from active peer record snapshots when the underlying propagation payload
  no longer exists, keeping export/restart state aligned with live queue
  cleanup.
- Peer sync stale queue cleanup now also treats case-variant live peer marks as
  the same peer, so stale unhandled or completed rows cannot survive under a
  caller-case variant and later reappear in restart/export state.
- Restored peer records now accept Python MessagePack binary
  `destination_hash`, handled, and unhandled IDs, prune serialized queue IDs
  whose payloads are missing during replay, and canonicalize/deduplicate the
  surviving IDs, avoiding restart/export drift when Python snapshot entries
  outlive or duplicate local propagation storage.
- Early transfer-limit decisions made before peering-key handling now update
  active peer record snapshots as completed work, keeping serialized state in
  sync with the live transfer-limited mark.
- Early transfer-limit handling now also ignores explicit "wants none" offer
  responses before peering-key gates, so oversized queued entries complete as
  transfer-limited instead of remaining retryable behind a postponed sync.
- Persistent peer sync now preserves explicit offer-response boundaries by
  leaving sync-limit-skipped IDs queued for the next offer instead of
  auto-transferring messages outside the peer's current response.
- Peer maintenance now replays payload-backed restored unhandled queue
  snapshots before choosing a sync candidate, so restart-loaded peers can be
  selected and transferred without waiting for a manual `peer_sync`.
- Peer maintenance rotation now also replays restored queue snapshots before
  low-acceptance drop decisions, so restart-loaded peers with pending transfer
  work are not rotated out as if their queues were empty.
- Shared unpeer cleanup now replays restored queue snapshots before computing
  and clearing propagation marks, so policy culls and explicit teardown do not
  discard restart-loaded peer queue work without cleanup accounting.
- Inbound propagation offers now mark already-known offered payload IDs as
  received from the offering peer after peering-key validation, so later peer
  admission does not queue those source payloads back to the sender.
- Valid inbound propagation offers now start the peer offer throttle window
  after peering-key and transient-ID validation, so repeated replication offers
  from the same peer take the throttled response path even when the peer changes
  the offered transient-ID set.
- Propagation ingest now rejects payloads for ignored destinations before
  storing or queueing them, enforcing local replication policy before relay
  state is created.
- Inbound propagation message-get `haves` completion now applies only to
  locally known payloads or existing peer queue marks, preventing unknown haves
  from suppressing future propagation work for the declaring peer.
- The live Python compatibility gate now includes a Python-origin propagation
  `/get` haves-only case against Rust `reticulumd`, covering `true`
  acknowledgement, Rust-side payload purge, and suppression of retryable
  unhandled peer queue state for the declaring propagation peer, plus a
  Python-origin `/offer` case covering partial wanted-ID responses,
  repeated-offer throttling, and source-peer completed marks before broad
  peer/router interop is claimed.
- Link-based propagation-control waits now treat matching resource transfer
  failure and cancellation as terminal remote fetch/download outcomes instead
  of waiting for the generic response timeout.
- The live Python compatibility gate now also splits out a Python-origin
  `/offer` peer-queue lifecycle case, covering post-sync handled IDs,
  absence of retryable missing IDs, and cleared sync backoff after the Rust
  peer row is created by transfer.

## Remaining Release Blockers

These are blockers to a broad "Python replacement" claim, not blockers to using
the implemented subset.

1. **Propagation router lifecycle**
   - Complete peer offer, transfer, retry, fetch, download, and synchronization
     behavior across success, denial, timeout, identity, and restart paths.
   - Close remaining `LXMRouter` side effects and persistent queue semantics.
2. **Deferred stamp lifecycle**
   - Add Python-style background work ownership, queueing, retry, cancellation,
     and progress behavior for expensive normal and propagation stamps.
3. **Interop breadth**
   - Add bidirectional live Python cases for every claimed delivery mode and
     newly completed peer/router row.
   - Propagation remote-status/control now has dispatchable compatibility cases
     for Python control-path discovery, Rust-to-Python remote status, and
     Python-origin `/get` haves acknowledgement and `/offer` side effects;
     broader peer/router row coverage still needs additional live scenarios.
   - Capture release evidence for Sideband, MeshChatX, and Columba before making
     client-specific compatibility claims.
4. **Reticulum behavioral breadth**
   - Finish channel ordering, resolver/bootstrap, announce/path edge behavior,
     and runtime mutation parity.
5. **Operational breadth**
   - Add prepared-host hardware evidence for BLE/RNode paths.
   - Implement or explicitly defer missing Python interface families and
     utility commands.

## Active Execution Order

1. Finish propagation peer/router state machines.
2. Finish deferred stamp worker and retry lifecycle.
3. Expand pinned Rust/Python interoperability gates with each completed row.
4. Close RNS channel, discovery, resolver, and transport-policy gaps.
5. Collect hardware, soak, and external-client release evidence.
6. Expand interface and utility breadth after protocol behavior stabilizes.

## Verification Baseline

- Primary CI: `.github/workflows/ci.yml`
- Pinned Python interop: `.github/workflows/python-interop.yml`
- Reference revisions are declared in the interop workflow rather than copied
  into status prose.
- Current run status belongs in GitHub Actions, not in this maintained document.
- A passing Python-reference workflow proves only the scenarios it executes.

## Status Rules

- `done` requires active implementation plus active automated evidence.
- A local model, RPC projection, or SDK state machine alone does not establish
  Python protocol/runtime parity.
- A passing interop workflow does not promote unrelated matrix rows.
- Update this file and the affected matrix in the same change.
- Keep implementation history in Git and historical plans, not in this file.
