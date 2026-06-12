# LXMF Parity Matrix

Last reassessed: 2026-06-08

This is the maintained row-level status for Python LXMF compatibility.
Repository-level posture and execution order live in
`docs/status/current-roadmap.md`.

Status legend:

- `done`: implemented in the active workspace and backed by active tests.
- `partial`: useful behavior exists, but identified Python behavior or evidence
  remains missing.
- `not-started`: no meaningful active implementation.

Workspace paths are used for navigation. `crates/libs/lxmf-core` publishes as
`lxmf-wire`; `crates/libs/rns-rpc` publishes as `reticulum-rs-rpc`.

## Module Matrix

| Python module | Rust surface | Status | Implemented baseline | Residual gap |
| --- | --- | --- | --- | --- |
| `LXMF/LXMF.py` | `crates/libs/lxmf-core` | partial | Constants, payload fields, message identity, inbound decoding, and wire helpers. | The complete convenience/module surface is not mirrored. |
| `LXMF/LXMessage.py` | `crates/libs/lxmf-core` | done | Wire, storage, propagation, paper, signatures, message IDs, binary fidelity, and timestamp precision metadata. | No confirmed base-message blocker. |
| `LXMF/LXMPeer.py` | `crates/libs/rns-rpc`, `crates/apps/reticulumd` | partial | Persistent peers, queue marks, offer selection, policy gates, peering keys, throttling, maintenance, source accounting, cumulative acceptance, serialized restored queue snapshots, and boolean/list/numeric offer responses. | Complete transfer/retry/restart lifecycle and broad live peer interop remain. |
| `LXMF/LXMRouter.py` | `crates/libs/rns-rpc`, `crates/apps/reticulumd` | partial | Outbound modes, selected propagation nodes, direct/propagated resources, cancellation, fetch/download/sync RPCs, receipts, persistence, and status. | Full Python queue, retry, propagation-node, and command side effects remain. |
| `LXMF/Handlers.py` | `crates/apps/reticulumd`, `crates/libs/rns-rpc` | partial | Delivery, announce, propagation app-data, receipt, and inbound bridge handling. | Some router-coupled side effects and negative/drop observability remain narrower. |
| `LXMF/LXStamper.py` | `crates/libs/lxmf-core`, `crates/libs/rns-rpc`, `crates/apps/reticulumd` | partial | Validation, generation, ticket-derived stamps, cancellation-aware task work, and lifecycle metadata. | Python-style deferred worker queue, retry ownership, and continuous progress remain. |

## Method Checklist

- PARITY_ITEM id=message.pack_wire status=done
- PARITY_ITEM id=message.unpack_wire status=done
- PARITY_ITEM id=message.storage_roundtrip status=done
- PARITY_ITEM id=message.propagation_pack_unpack status=done
- PARITY_ITEM id=message.paper_pack status=done
- PARITY_ITEM id=message.paper_uri_helpers status=done
- PARITY_ITEM id=message.file_unpack_helpers status=done
- PARITY_ITEM id=message.signature_verify status=done
- PARITY_ITEM id=message.object_accessors status=done
- PARITY_ITEM id=stamper.validate_pn_stamp status=partial
- PARITY_ITEM id=stamper.generate_stamp status=partial
- PARITY_ITEM id=stamper.cancel_work status=partial
- PARITY_ITEM id=stamper.outbound_progress_queries status=partial
- PARITY_ITEM id=ticket.validity_with_grace status=done
- PARITY_ITEM id=ticket.renewal_window status=done
- PARITY_ITEM id=ticket.derived_stamp status=done
- PARITY_ITEM id=peer.serialize_roundtrip status=partial
- PARITY_ITEM id=peer.queue_accounting status=partial
- PARITY_ITEM id=peer.acceptance_rate status=partial
- PARITY_ITEM id=peer.peering_key status=partial
- PARITY_ITEM id=router.outbound_queue status=partial
- PARITY_ITEM id=router.handle_outbound_policy status=partial
- PARITY_ITEM id=router.adapter_transport status=partial
- PARITY_ITEM id=router.paper_uri_ingest status=partial
- PARITY_ITEM id=router.cancel_outbound status=partial
- PARITY_ITEM id=router.propagation_ingest_fetch status=partial
- PARITY_ITEM id=router.transfer_state_lifecycle status=partial
- PARITY_ITEM id=router.node_app_data status=partial
- PARITY_ITEM id=handlers.delivery_callback status=partial
- PARITY_ITEM id=handlers.propagation_app_data status=partial
- PARITY_ITEM id=handlers.router_side_effects status=partial
- PARITY_ITEM id=interop.python_live_gate status=done

## Capability Detail

### Messages and interchange

- Python-compatible wire and storage containers are emitted and accepted.
- Propagation and paper packing use canonical `lxmf-wire` helpers.
- Signed messages, fields, attachment aliases, floating timestamps, and
  non-UTF8 title/content bytes retain client-visible fidelity.

### Delivery and receipts

- Direct, opportunistic, propagated, and paper modes are distinct.
- Transport completion remains `sent`; final delivery receipts produce
  `delivered`.
- Oversized opportunistic peer sends fall back to link/resource delivery, with
  resource advertisement and outbound tracking coverage.
- Resource advertisement failure, retry exhaustion, timeout, and explicit
  cancellation reach daemon message state.

### Tickets and stamps

- Ticket grace, renewal, derivation, persistence, and reply reuse are complete.
- Inbound normal and propagation stamps honor configured flexibility.
- Outbound normal and propagation work records generating, ready, failed, and
  cancelled state.
- The remaining gap is background queue/worker/retry behavior, not basic stamp
  cryptography or ticket semantics.

### Peers and propagation

- Peer behavior includes static/discovered admission, peering cost/timebase,
  queue accounting, sync/transfer limits, stamp policy, throttling, candidate
  selection, unreachable culling, low-acceptance rotation, and prioritized
  offers.
- Python-style propagation `auth_required` configuration is applied to the
  daemon propagation state and reported with the propagation peer policy.
- Offer responses support Python boolean and list forms, reject out-of-offer
  IDs, preserve no-transfer liveness, retain cumulative acceptance rates, and
  preserve peers and queues on retryable or otherwise unexpected numeric
  offer-response cleanup paths.
- Retryable numeric local offer responses mirror payload-backed live handled and
  unhandled queue marks into active peer record snapshots before returning,
  preserving restart/export retry state even when the serialized snapshot was
  previously empty.
- Retryable, throttled, generic failed, malformed-import, and
  bridge-unavailable remote peer-sync paths mirror the same payload-backed live
  and restored queue marks into active peer record snapshots before publishing
  the failed sync event, so local and remote retry/export behavior stays
  aligned.
- Retryable remote peer-sync errors keep those queued snapshots but now advance
  the peer's ordinary sync backoff window, avoiding immediate retry loops after
  transient propagation-control failures.
- Payload-backed remote failure snapshots replace stale serialized peer queue
  IDs with live payload-backed marks, so bridge failures do not preserve
  obsolete restart/export work after the underlying payload is gone.
- Zero-cost peer stamp policies transfer unstamped queued offers immediately
  without waiting for absent peering metadata, matching the Python no-stamp
  path and avoiding repeated peer-sync postponement.
- Python propagation announce transfer and sync limits are converted from
  advertised integer or fractional kilobytes into the byte limits used by
  peer-sync queue selection, so valid queued payloads are not misclassified as
  transfer-limited.
- Propagation peer maintenance selection claims the chosen peer before invoking
  sync by recording the sync attempt and next backoff window, while allowing the
  internal maintenance-triggered sync to consume that claim, so concurrent
  scheduler passes cannot double-select the same peer.
- Manual `/pn/peer/sync` control requests force an immediate peer sync through
  ordinary backoff windows, while scheduled maintenance and remote syncs still
  respect retry postponement, matching the operator-triggered retry path.
- Remote fetch/download/sync imports validate the full returned propagation
  payload batch before mutating the local store or in-memory payload cache, so
  mixed valid/invalid remote responses fail without leaving partial relay state.
- Selected local peer-sync offer responses validate the full selected
  propagation response payload batch before marking any selected ID transferred,
  so malformed queued payloads cannot partially drain peer retry state.
- Malformed remote fetch and download imports mirror existing payload-backed
  queue marks into active peer record snapshots before returning the import
  failure, so already queued relay work remains visible after restart/export.
- Remote fetch and download bridge failures mirror existing payload-backed
  queue marks into active peer record snapshots before returning the failure,
  so already queued relay work remains visible after restart/export.
- Remote fetch and download access-denied bridge failures follow the remote
  peer-sync denial path for the source peer, clearing local peering and queued
  propagation marks instead of preserving denied relay work for retry, while
  preserving the propagation `no_access` lifecycle state and bridge error text.
- Remote fetch and download bridge-unavailable errors mirror existing
  payload-backed queue marks into active peer record snapshots before
  returning and mark the propagation sync lifecycle failed, so queued relay work
  remains visible after restart/export without leaving stale lifecycle state
  when no bridge is configured.
- Successful remote fetch and download mirror existing payload-backed queue
  marks into active peer record snapshots after applying imports, preserving
  queued retry work across restart/export even when the remote transfer succeeds
  without consuming those local queued offers.
- Remote peer-sync backoff postponements mirror existing payload-backed queue
  marks into active peer record snapshots before returning, so deferred syncs
  preserve queued retry work across restart/export.
- Remote peer-sync bridge-unavailable errors mirror existing payload-backed
  live marks and restored peer-record queue IDs into active peer record
  snapshots for already known peers before returning, including
  case-insensitive requests, without creating new peers when the bridge is
  absent.
- Remote peer-sync bridge-unavailable errors for already known peers also
  publish the failed peer-sync event and mark the propagation sync lifecycle
  failed, keeping queued retry state observable without creating new peers.
- Successful remote peer-sync mirrors existing payload-backed live queue marks
  into active peer record snapshots after applying imports, preserving queued
  retry work across restart/export even when the remote sync succeeds without
  transferring those local queued offers.
- Successful remote peer-sync imports refresh payload-backed queue snapshots
  for all active peers affected by imported payloads, so relay peers preserve
  complete restart/export-visible unhandled queues rather than only newly
  imported IDs.
- Remote peer-sync imports transferred propagation payloads from both daemon
  `payload_hex` fields and MessagePack binary payload arrays, so bridge results
  converted through `rmpv_to_json` enqueue the same relay work without treating
  numeric `payload_bytes` count metadata as malformed payload data.
- Remote peer-sync uses the stored peer ID case for the bridge call, import
  source accounting, state updates, and response envelope when callers supply a
  case-variant peer request.
- Failed remote unpeer attempts mirror existing payload-backed queue marks and
  restored peer-record queue IDs into active peer record snapshots before
  returning bridge-unavailable or bridge-execution errors, including
  case-insensitive peer requests, so failed peering teardown preserves queued
  retry work across restart/export and marks the propagation lifecycle failed
  instead of leaving stale idle/completed state.
- Successful remote unpeer uses the stored peer ID case for the bridge call and
  nested bridge result when callers supply a case-variant peer request, keeping
  remote teardown identity aligned with local queue cleanup.
- Inbound reticulumd `/pn/peer/sync` and `/pn/peer/unpeer` control commands
  resolve stored peer IDs case-insensitively before dispatching to daemon RPCs,
  so binary peer-control requests do not report not-found for restored or
  configured peers whose status rows preserve a different hex presentation.
- Payload-backed peer queue snapshot mirroring resolves stored peer IDs
  case-insensitively before reading live queue marks, preserving queued
  restart/export work when callers use Python-style peer case variants.
- Incremental peer queue snapshot updates resolve stored peer IDs before
  checking completed live marks, so transfer-limited or handled work is not
  serialized as retryable unhandled queue state through peer case variants.
- Incremental peer queue snapshot helpers canonicalize transient IDs before
  serializing handled or unhandled queue state, so padded or upper-case caller
  IDs do not leak into restart/export snapshots.
- Transfer-limited peer marks remain terminal when a later generic handled
  report arrives, so transfer-limit retry decisions are not reclassified as
  offered/handled work in peer queue accounting.
- Transfer-limited peer marks also remain terminal when a later transferred
  report arrives, so completed transfer-limit decisions are not reclassified as
  outgoing/offered work by subsequent queue updates.
- Transfer-limited peer marks also remain terminal when a later received
  report arrives, so completed transfer-limit decisions are not reclassified as
  incoming work by subsequent propagation imports.
- Terminal peer marks clear case-variant unhandled rows for the same transient
  ID, so handled, transferred, received, and transfer-limited work cannot
  remain retryable under an alternate caller-case peer key.
- Peer sync unhandled transfer selection and retry cleanup read and remove
  caller-case peer variants as one effective peer, so queued transfer work is
  not skipped or left retryable under alternate peer casing.
- Prospective peer queue selection also reads case-variant completed marks
  before returning unhandled work, so helper-level queue selection cannot reopen
  received, transferred, handled, or transfer-limited payloads under alternate
  peer casing.
- Static-only propagation peer replacement routes removed static peers through
  the same local unpeer cleanup as explicit unpeer, so handled, received,
  transfer-limited, and unhandled queue marks are cleared and accounted
  consistently.
- Completed peer mark helpers write and read received/transferred live marks
  under the stored peer ID case when a peer record already exists, keeping live
  queue state and serialized restart/export snapshots aligned.
- Restored peer record queue IDs are replayed into the live store, newly queued
  existing and inbound/imported propagation IDs are reflected in the serialized
  peer snapshot, source-peer handled IDs are preserved for restart/export, and
  offer-response handling keeps IDs in sync when queued messages become handled,
  transferred, or transfer-limited.
- Restored Python peer records parse fractional `propagation_sync_limit` values
  through Python's integer-kilobyte restore path before peer-sync queue
  selection, so restored fractional sync limits leave the same queued work
  pending as Python.
- Restored Python peer records coerce numeric stamp, stamp-flexibility, and
  peering costs through Python's integer restore path before peering checks, so
  float-valued snapshots can still transfer queued stamped offers.
- Restored Python peer records also coerce numeric `sync_strategy` through
  Python's integer restore path, so float-valued persistent-peer snapshots keep
  draining queued offers across sync-limit batches.
- Restored Python peer records accept Python `time.time()` float timestamps for
  heard/sync/backoff fields, so restart-loaded peers can still reach queued
  transfer instead of failing restore before sync.
- Restored Python peer records coerce numeric message and byte counters before
  peer-sync accounting, so restart-loaded peers preserve cumulative
  offered/outgoing/incoming totals while transferring newly queued work.
- Restored Python peer records preserve serialized LXMPeer metadata through
  Rust peer record round trips, so restart/export snapshots keep peer-specific
  metadata before later queue work resumes.
- Live propagation announces retain Python PN metadata on active peer records,
  so announce-derived peer metadata survives into later peering and queue
  restart/export snapshots.
- Python-style `lxmd` `[lxmf] announce_interval` drives peer/delivery announce
  cadence separately from `[propagation] announce_interval`, which remains the
  propagation-node announce cadence.
- Outbound propagated delivery resolves selected propagation-node
  `propagation_stamp_cost` case-insensitively, so Python-style hash casing does
  not fall back to the default propagation stamp cost.
- The live Rust/Python remote-relay interop gate now selects a Python `lxmd`
  propagation destination as the Rust outbound propagation node, covering mixed
  propagation-node discovery and selection before broader store-and-forward
  claims are made.
- Duplicate inbound peer propagation payloads still fan out to active relay
  peers while keeping the source peer handled, so a known local payload does
  not bypass relay queue creation.
- Locally delivered inbound peer propagation payloads are stored and fanned out
  to active relay peers while keeping source peer activity counted once, so
  local delivery does not bypass propagation queue creation.
- Inbound peer propagation ingest marks inactive identified sources as
  received before later activation, so source-accounting survives when a sender
  becomes a propagation peer after supplying payloads.
- Inbound propagation message-get serving admits or refreshes the remote
  propagation peer before marking served payloads transferred, so transfer
  accounting survives when a peer fetches before a prior offer row exists.
- Inbound propagation message-get serving previews fetchable payloads and
  passes peer admission before mutating served counters, so rejected static-only
  or capacity-limited peers do not look like successful transfers.
- Inbound propagation message-get listing applies peer admission before
  returning non-empty payload ID lists, so rejected peers cannot enumerate
  queued transfers they are not allowed to fetch.
- Inbound propagation message-get `haves` handling applies peer admission
  before purging matching local payloads, so rejected peers cannot delete queued
  transfers they are not allowed to acknowledge.
- Inbound propagation message-get `haves` handling records matched haves as
  received/completed work for the requesting propagation peer after purge, so
  reintroduced payloads are not queued back to peers that already declared
  them.
- Inbound propagation message-get `haves` handling records stale peer-acknowledged
  IDs even when local payload rows are already absent, while still honoring
  `retain_synced_on_node` payload-retention behavior so completed peers are
  marked without regressing local payload reuse.
- Link-based remote propagation downloads wait for the final haves
  acknowledgement response after imported or duplicate payloads are reported,
  so node-side rejection or timeout is surfaced instead of reporting a
  completed download before remote cleanup is confirmed.
- Inbound propagation message-get purge-only requests return the Python-style
  boolean success response after haves are applied, and payload purge cleanup
  preserves completed peer accounting for other peers while removing stale
  unhandled marks, so reintroduced payloads are not offered back to peers that
  already completed them.
- Propagation nodes honor `retain_synced_on_node` during message-get haves
  handling: requesting peers are still marked completed, while retained payloads
  remain stored and queued for peers that have not completed them.
- Inbound propagation message-get requests mark wanted payloads skipped by the
  peer's transfer budget as transfer-limited completed work after peer
  admission, so oversized fetch attempts do not remain retryable queue entries.
- Inbound propagation message-get transfer-budget handling keeps payloads
  skipped only by the cumulative response budget retryable for a later request,
  while individually oversized wanted payloads still complete as
  transfer-limited.
- Inbound propagation offer requests with too-short list payloads return the
  Python-compatible nil response without validating the link or admitting the
  remote propagation peer.
- Valid inbound propagation offers answer Python's `False`, `True`, or
  wanted-ID list responses after peering-key validation without admitting the
  remote propagation peer or queuing local payloads before a real transfer or
  message-get admission point.
- Structurally decoded inbound propagation offers with invalid peering keys
  start the per-peer offer throttle while still avoiding peer admission or
  queue marks, so repeated bad replication offers share the valid-offer
  throttle window.
- Inbound propagation offers validate every offered transient ID before
  applying source-accounting marks, so malformed mixed offers cannot leave
  partial received/completed queue state behind.
- Inbound propagation offers deduplicate validated offered transient IDs before
  building wanted-ID responses or applying source-accounting marks, so duplicate
  offers cannot request or account the same payload more than once.
- Capacity-limited but valid inbound propagation offers also start the offer
  throttle after peering-key and transient-ID validation, so repeated
  deferred-admission offers return the Python-style throttled response instead
  of repeatedly probing peer capacity.
- Remote fetch and download imports mark inactive source peers as received
  before later activation, so source-accounting survives even when the
  propagation node was not yet an active peer record.
- Remote import batches deduplicate accepted transient IDs before peer queue
  and incoming-message side effects are applied, so duplicate payloads in one
  fetch/download/sync response do not inflate peer queue accounting.
- Remote import batch byte accounting follows the same deduplicated accepted
  IDs, so duplicate payloads in one fetch/download/sync response do not inflate
  transferred byte totals or source peer receive byte counters.
- Local propagation ingest persists processed transient IDs separately from
  retained payload entries, so payloads reintroduced after purge or peer
  acknowledgement can refresh relay state without inflating local received or
  ingested counters.
- Propagation-node ingest enforces the configured message-storage byte limit
  against retained propagation entries, pruning oldest payloads and stale
  retryable peer marks.
- Link-based remote downloads wait for the propagation node's `/get` haves
  acknowledgement and propagate peer/control errors, so failed remote cleanup is
  not reported as a completed replication drain.
- Remote fetch/download acknowledgements use canonical propagation transient
  IDs for stamped payloads, so `/get` haves clear the peer's offered queue entry
  instead of reporting the stamped payload bytes under a different hash.
- Repeated remote fetch/download/sync imports increment source peer incoming
  counts and receive bytes only for payload IDs not already marked received
  from that source, while still replaying known payloads into relay queues when
  their live marks were cleared.
- Link-based remote propagation downloads classify listed transient IDs before
  payload retrieval, report locally known IDs as `/get` haves, and use the
  purge-only `[nil, haves]` request when every listed ID is already local, so
  duplicate payloads are not downloaded just to acknowledge them.
- Repeated peer-origin propagation ingests also avoid double-counting source
  peer incoming counts and receive bytes for already received payload IDs,
  while still refreshing relay queue marks for peers that need the payload.
- Remote peer-sync imports accept transferred payload arrays from full
  Python-style responses where top-level `messages` is a peer counter object
  and payloads live under `propagation.messages`/`propagation.payloads`, as
  well as legacy top-level `messages`/`payloads` envelopes.
- Purging local propagation payloads removes matching deleted IDs from active
  peer record snapshots, preventing restart/export drift after queue cleanup.
- Duplicate or replayed propagation queue attempts preserve completed peer
  snapshot state instead of reopening handled IDs as serialized unhandled work.
- Duplicate or replayed queue attempts also respect case-variant completed live
  marks, so handled, transferred, received, or transfer-limited IDs are not
  serialized as retryable unhandled work through the stored peer key.
- Peer sync queue replay mirrors preexisting live unhandled marks into active
  peer record snapshots, keeping restart/export state aligned even when no new
  store rows were inserted.
- Peer activation also mirrors preexisting live completed marks into active
  peer record snapshots, so transfers recorded before the peer record exists
  survive restart/export as handled IDs once the propagation peer is active.
- Peer activation also merges case-variant preexisting completed marks into
  the activated peer key before queue replay, keeping restart/export state
  aligned when transfer accounting arrives before the peer record case is known.
- Selected propagation node activation reuses the existing peer record case
  before queue replay and canonicalizes merged live marks, preventing
  caller-case variants from leaving duplicate peer queue rows.
- Peer unpeer cleanup clears case-variant propagation marks as one peer, so
  completed marks merged during activation cannot survive teardown and reappear
  as handled work when that peer is later reactivated.
- Peer unpeer cleanup also removes the peer from configured static propagation
  membership, so an explicit unpeer cannot be undone by the next static-peer
  activation pass.
- Peer unpeer cleanup accounting reads case-variant live queue marks as one
  effective peer before clearing them, so the response and event report the
  handled/unhandled IDs and byte totals that teardown actually removes.
- Rejoining from a persisted `unpeered` peer record clears stale serialized
  queue snapshots before the peer is active again, preventing pre-unpeer work
  from being restored on export/restart.
- Rejoining from a persisted `unpeered` peer record also clears stale live
  completed propagation marks before queue replay, so still-local payloads are
  offered again when the peer rejoins as manual or configured static.
- Rejoining from a persisted `unpeered` non-static record re-runs admission
  before activation, so static-only policy cannot be bypassed by stale teardown
  state.
- Static peer activation clears stale serialized queue snapshots when it
  revives a persisted `unpeered` record, preventing configured static peering
  from restoring pre-unpeer propagation work on export/restart.
- Rejoining from a persisted `unpeered` peer record clears stale sync backoff
  postponement fields, preventing pre-unpeer retry scheduling from blocking
  manual or configured static peering.
- Peer sync reactivation bypasses stale pre-unpeer backoff postponements
  before admission and queue replay, preventing manual rejoins from returning
  as postponed `unpeered` peers.
- Peer sync reactivation applies the active peer type even when a restored
  `unpeered` record has a future `last_seen` timestamp, preventing clock-skewed
  restart state from leaving a rejoined peer marked unpeered.
- Peer sync stale queue cleanup prunes matching active peer record snapshot IDs
  for unhandled and completed marks when the propagation payload has already
  been removed, keeping serialized restart/export state aligned with live queue
  cleanup.
- Peer sync stale queue cleanup treats case-variant live peer marks as the same
  peer, so stale unhandled or completed rows cannot survive under caller-case
  variants and later reappear in restart/export state.
- Restored peer record replay accepts Python MessagePack binary
  `destination_hash`, handled, and unhandled IDs, prunes serialized IDs whose
  payloads are absent, and canonicalizes/deduplicates surviving IDs, so stale
  or repeated Python snapshot entries are not exported again after replay.
- Transfer-limit decisions made before peering-key handling update active peer
  record snapshots as completed queue work, so restart/export state reflects
  the live transfer-limited mark.
- Transfer-limit handling also wins over explicit "wants none" offer responses
  before peering-key gates, keeping oversized entries out of retryable queues
  when the peer declines the current offer.
- Persistent peer sync preserves explicit offer-response boundaries by keeping
  sync-limit-skipped IDs queued for the next offer instead of auto-transferring
  messages outside the peer's current response.
- Peer maintenance replays payload-backed restored unhandled queue snapshots
  before selecting a sync candidate, so restart-loaded queue work can be
  transferred by automatic maintenance without a manual peer sync first.
- Peer maintenance rotation also replays restored queue snapshots before
  low-acceptance drop decisions, so restart-loaded peers with pending transfer
  work are not rotated out as empty.
- Shared unpeer cleanup replays restored queue snapshots before computing and
  clearing propagation marks, so policy culls and explicit teardown account for
  restart-loaded peer queue work before removing the peer.
- Inbound propagation offers mark already-known offered payload IDs as received
  for the offering peer after peering-key validation, preventing later peer
  admission from offering the sender its own known payloads.
- Valid inbound propagation offers start the peer offer throttle window after
  peering-key and transient-ID validation, so repeated replication offers from
  the same peer return the throttled response even when the peer changes the
  offered transient-ID set.
- Inbound propagation distinguishes clients, validated peers, unpeered
  identified senders, and local delivery; source peers are accounted and not
  re-offered their own payloads.
- These behaviors materially narrow the gap, but complete Python peer transfer,
  restart recovery, and router queue lifecycle remain unproven.

## Highest-Priority Gaps

1. Complete peer transfer, retry, restart, and persistent queue lifecycle.
2. Complete propagation-node fetch/download/sync and router side effects.
3. Add deferred stamp queue ownership and retry semantics.
4. Expand live bidirectional Python interop for propagation and peer rows.
5. Validate external clients before making client-specific claims.

## Evidence

- `.github/workflows/python-interop.yml` runs pinned Python reference
  conformance plus live channel, paper, compatibility-matrix, and LXMD
  remote-relay tests.
- The compatibility matrix includes an ignored live
  `propagation_remote_status_bidir` case that validates Python discovery of
  the Rust propagation-control path and dispatches a Rust-to-Python
  propagation-node status query when the Python harness environment is
  available.
- Focused daemon/RPC tests cover delivery modes, propagation offers, peer
  maintenance, queue policy, source accounting, stamps, tickets, receipts, and
  cancellation.
- `interop.python_live_gate` means the configured scenarios run successfully;
  it does not imply every partial row is complete.
