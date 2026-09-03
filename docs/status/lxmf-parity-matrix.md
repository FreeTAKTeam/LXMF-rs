# LXMF Parity Matrix

Last reassessed: 2026-09-01

This is the maintained row-level status for Python LXMF compatibility.
Repository-level posture and execution order live in
`docs/status/current-roadmap.md`.

Parity is recorded on two independent axes:

- implementation: `complete`, `partial`, or `not-applicable`;
- evidence: one or more of `unit`, `simulated`, `pinned-python`,
  `cross-implementation`, `prepared-host`, `third-party-client`, `hardware`,
  `public-network`, or `hardware-unverified`.

Workspace paths are used for navigation. `crates/libs/lxmf-core` publishes as
`lxmf-wire`; `crates/libs/rns-rpc` publishes as `reticulum-rs-rpc`.

SDK access is independently complete for software-controlled LXMF operations:
ZeroMQ now implements every `SdkBackend` domain, native async operations and
cursor-replaying events, and the `LxmfSdkRouter`/`LxmfSdkPropagation` extension
traits. Row-level transport classification is generated in
`docs/status/sdk-zmq-parity.json`.

The historical v0.9.8 record retains its release boundary. Current `main` and
the v0.10.0 release train keep all seven tracked LXMF module rows `complete` for their
named software scenarios, with hardware, public-network, and third-party-client
evidence tracked independently.

## Module Matrix

| Python module | Rust surface | Implementation | Evidence | Implemented baseline | Residual gap |
| --- | --- | --- | --- | --- | --- |
| `LXMF/LXMF.py` | `crates/libs/lxmf-core` | complete | unit, pinned-python | Pinned module constants, payload fields, message identity, inbound decoding, wire helpers, delivery app-data helpers, compression support detection, and propagation-node announce helper validation. | No confirmed `LXMF.py` blocker in the pinned Python reference. |
| `LXMF/LXMessage.py` | `crates/libs/lxmf-core` | complete | unit, pinned-python | Wire, storage, propagation, paper, signatures, message IDs, binary fidelity, and timestamp precision metadata. | No confirmed base-message blocker. |
| `LXMF/LXMPeer.py` | `crates/libs/rns-rpc`, `crates/apps/reticulumd` | complete | unit, pinned-python | Persistent peers, queue marks, offer selection, policy gates, peering keys, throttling, maintenance, source accounting, cumulative acceptance, serialized restored queue snapshots, boolean/list/numeric offer responses, transfer/retry/restart recovery, and unpeer cleanup. | No confirmed `LXMPeer.py` blocker in the pinned Python-only coverage. |
| `LXMF/LXMRouter.py` | `crates/libs/rns-rpc`, `crates/apps/reticulumd`, `crates/apps/lxmf-cli`, `crates/libs/lxmf-sdk`, `crates/libs/lxmf-runtime` | complete | unit, simulated, pinned-python | Outbound modes and progress/cancellation, selected propagation nodes, direct/propagated resources, accepted-result Resource timeout/failure propagation with transport cancellation before retry, cleanup-failure visibility, fetch/download/sync RPCs, receipts, persistence and maintenance, propagation-node side effects, retry/failure handling, delivery policy, ticket and stamp lifecycle, peer distribution, announce metadata, delivery-link/resource callbacks, transient caches, typed router statistics, and typed message/information storage policy. `SdkBackend`, RPC, ZeroMQ, and in-process backends share the additive router-management contract. | No generated public-callable software gap remains in `LXMRouter.py`; physical/public-network and third-party-client evidence remain separate. The in-process propagated-delivery path (`lxmf-runtime` `send_propagated`) generates a real propagation stamp at the Python default target cost 16, which satisfies default-configured relays (minimum accepted 13); it does not yet honor a relay's announced stamp cost, so relays enforcing a minimum above 16 still reject these transfers until the announced `pn_stamp_cost` is plumbed into stamp generation. |
| `LXMF/Handlers.py` | `crates/apps/reticulumd`, `crates/libs/rns-rpc` | complete | unit, simulated, pinned-python | Delivery and propagation announce handlers implement stamp-cost updates, pending direct/opportunistic wakeup, path-response handling, static-peer refresh, autopeer depth policy, peer removal, malformed announce visibility, and the router-coupled delivery/receipt/drop side effects exposed through daemon events and the typed SDK. | No confirmed software blocker in the pinned four-item public handler surface. |
| `LXMF/LXStamper.py` | `crates/libs/lxmf-core`, `crates/libs/rns-rpc`, `crates/apps/reticulumd` | complete | unit, pinned-python | Validation, generation, ticket-derived stamps, cancellation-aware task work, background deferred worker queue ownership, retry state, cancellation, propagation-stamp pre-handoff preparation, progress metadata, and default-cost propagation stamp mining/validation in `lxmf-wire` (`stamp.rs`, ported `LXStamper` workblock HKDF) with fail-fast rejection of unattainable costs above 256. | No confirmed deferred-stamp lifecycle blocker. |
| `LXMF/Utilities/lxmd.py` | `crates/apps/lxmf-cli`, `crates/apps/reticulumd` | complete | unit, simulated, pinned-python | Canonical `lxmd` workflows, configuration, announce cadence, propagation controls, status, and daemon lifecycle map to the shared Rust daemon and CLI surfaces. | No generated public utility callable is unmapped. |

## Method Checklist

- PARITY_ITEM id=message.pack_wire implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=message.unpack_wire implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=message.storage_roundtrip implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=message.propagation_pack_unpack implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=message.paper_pack implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=message.paper_uri_helpers implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=message.file_unpack_helpers implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=message.signature_verify implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=message.object_accessors implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=module.app_data_helpers implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=stamper.validate_pn_stamp implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=stamper.generate_stamp implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=stamper.cancel_work implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=stamper.outbound_progress_queries implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=ticket.validity_with_grace implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=ticket.renewal_window implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=ticket.derived_stamp implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=peer.serialize_roundtrip implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=peer.queue_accounting implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=peer.acceptance_rate implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=peer.peering_key implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=router.outbound_queue implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=router.handle_outbound_policy implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=router.adapter_transport implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=router.paper_uri_ingest implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=router.cancel_outbound implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=router.accepted_result_resource_cancel implementation=complete evidence=unit,simulated
- PARITY_ITEM id=router.propagation_ingest_fetch implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=router.transfer_state_lifecycle implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=router.node_app_data implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=handlers.delivery_callback implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=handlers.propagation_app_data implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=handlers.router_side_effects implementation=complete evidence=unit,pinned-python
- PARITY_ITEM id=interop.python_live_gate implementation=complete evidence=unit,pinned-python

## Capability Detail

### Integrated delivery and router compatibility

- Direct delivery can reuse an identified inbound backchannel link for the
  same LXMF delivery destination, while closed or failed cached links are
  removed before normal link establishment resumes.
- Atomic `allow_destination`, `disallow_destination`, and
  `prioritise_destination` RPC/SDK operations coexist with the broader
  Python-style authentication, allow/ignore, and priority convenience surface.
- SDK delivery trace, opportunistic packet receipt metadata, conversation
  summaries, and inbound duplicate/drop events retain typed metadata after
  daemon restart and through ZeroMQ envelope dispatch. Conversation display
  names fall back to durable delivery announce history without turning delivery
  announces into propagation peers.

### v0.9.0 software-parity boundary

- v0.9.0 extends the existing `lxmf-sdk` and
  `ZmqPipelineBackendClient` in place for REM/RCH-facing workflows. It does
  not add a compatibility layer, adapter, shim, or new parallel SDK surface.
- LXMF send evidence for this release is scoped to typed SDK send/batch-send,
  delivery status, receipt terminality, retry metadata, cancellation, history,
  conversation summaries, paper encode/decode, and propagation-control calls
  exercised through `ZmqPipelineBackendClient`.
- LXMF receive evidence is scoped to SDK-pollable raw inbound, receipt, and
  inbound-drop events plus persisted direct/propagated local-delivery metadata
  produced by the daemon/RPC event stream.
- Carrier attach/announce evidence remains a Reticulum/interface concern:
  LXMF rows can cite it only when a named send/receive scenario crosses that
  software carrier, and optional HIL remains release confidence rather than a
  prerequisite for software-complete LXMF rows.

### Messages and interchange

- Python-compatible wire and storage containers are emitted and accepted.
- Propagation and paper packing use canonical `lxmf-wire` helpers.
- Signed messages, fields, attachment aliases, floating timestamps, and
  non-UTF8 title/content bytes retain client-visible fidelity.
- Documented basic field IDs are exported from `lxmf-wire`, and the typed
  ZeroMQ SDK send path preserves those keys plus `_lxmf_fields_msgpack_b64`.
- The typed ZeroMQ SDK send and batch-send paths map payload `body` into the
  LXMF message content when `content` is absent, while retaining the original
  `body` field for clients that store or render direct-chat bodies.
- The pinned Python `LXMF.py` convenience constants and app-data helpers are
  exposed from `lxmf-wire`: delivery display names, delivery stamp cost,
  compression support defaults, propagation-node names, propagation-node stamp
  cost, and propagation-node announce validation. Rust callers also get a
  typed propagation-node validation error for diagnostics instead of only the
  Python-style boolean result.

### Delivery and receipts

- Direct, opportunistic, propagated, and paper modes are distinct.
- An opportunistic packet carries the wire without its leading destination
  hash, as `LXMessage.send` packs `self.packed[DESTINATION_LENGTH:]`: the
  destination rides in the packet header and the receiving router prepends its
  own hash before unpacking (`LXMRouter.delivery_packet` into
  `LXMessage.unpack_from_bytes`). `lxmf-runtime`'s `send_opportunistic` handed
  over the whole wire, so a receiver saw the destination twice and a message it
  could not verify, and every opportunistic send from the in-process backend
  was dropped silently on the far side. It strips the prefix with
  `rns_transport::delivery::strip_destination_prefix`, as the daemon's
  `bridge_helpers::opportunistic_payload` already did.
- Opportunistic Single/Data delivery now generates receiver-policy-controlled
  proofs on the ingress interface; explicit proof validation requires the
  packet-cache correlation for the proved destination and rejects signatures
  from unrelated known identities.
- Link data and channel fan-out routes through each active link's bound
  interface and exposes structured matched/sent/failed counts to callers that
  need retry or degraded-state decisions.
- Transport completion remains `sent`; final delivery receipts produce
  `delivered`.
- Rust-originated link sends register packet/resource hashes before handoff and
  accept Python reference link proofs that use the default context, preserving
  final delivery receipt correlation after `sent:*`.
- Oversized opportunistic peer sends fall back to link/resource delivery, with
  resource advertisement and outbound tracking coverage.
- The Rust/Python smoke harness now treats Python-stored LXMF payloads as
  authoritative inbound evidence when hooks are unavailable, and hard-checks
  direct `delivered`, `sent: link resource`, and `sent: propagated resource`.
- Resource advertisement failure, retry exhaustion, timeout, and explicit
  cancellation reach daemon message state.
- In-process accepted-result Resource sends retain the backend transfer timeout,
  cancel a failed transfer before returning, and surface both the transfer error
  and any failed cancellation dispatch with the link and Resource hashes.
- Outbound SDK sends enter the daemon-owned queue before bridge handoff, run
  validation/policy checks before scheduling, expose delivery-pipeline queue and
  lane state, and preserve queued identity/path misses for delivery-announce
  wakeup rather than dropping them as terminal failures.
- Delivery callbacks now publish SDK-pollable receipt lifecycle events with
  optional router metadata when available: packet hash, resource hash, peer,
  method/stage, delivery kind, byte count, and link ID. The typed SDK event
  surface exposes the same fields through `DeliveryLifecycleDetails`.
- RPC daemon `lxmf.delivery` announces reschedule stored pending direct,
  default-direct, and opportunistic outbound messages for the announced
  destination, without waking propagated, paper, terminal, already-sending, or
  nonmatching records. Reticulumd direct/opportunistic identity/path misses now
  persist as nonterminal `queued: waiting for announce`, preserving them for
  the delivery-announce wakeup path while propagated/propagation-node misses
  stay terminal.
- `get_outbound_stamp_cost` now exposes Python-style destination-level cached
  delivery stamp-cost lookup through legacy RPC and the SDK envelope registry,
  returning `null` when no delivery announce cost has been learned.

### Tickets and stamps

- Ticket grace, renewal, derivation, persistence, and reply reuse are complete.
- `app.delivery.ticket.generate` now exposes ticket generation through the
  registered SDK envelope path and typed ZeroMQ backend, with `ticket_generate`
  alias support and ticket-interval suppression metadata preserved.
- Inbound normal and propagation stamps honor configured flexibility.
- Outbound normal and propagation work records generating, ready, failed, and
  cancelled state.
- Normal and propagation stamp retry metadata clears stale error fields when
  later work re-enters generating/ready state.
- Active outbound normal and propagation stamp generation reports stored
  continuous progress through `get_outbound_progress`; failed or cancelled
  stamp states still suppress stale progress.
- Expensive normal and requested propagated sends now enter a background
  deferred-stamp worker before delivery handoff. The worker exposes queue and
  in-flight ownership in `delivery_pipeline`, serializes stamp generation,
  retries failed generation attempts with retry metadata, accepts cancellation
  while stamp work is active, and prebuilds propagation resource payloads before
  delivery/link semaphores are acquired.

### Peers and propagation

- Peer behavior includes static/discovered admission, peering cost/timebase,
  queue accounting, sync/transfer limits, stamp policy, throttling, candidate
  selection, unreachable culling, low-acceptance rotation, and prioritized
  offers.
- Python-style propagation `auth_required` configuration is applied to the
  daemon propagation state and reported with the propagation peer policy.
- Python-style propagation control ACL entries are normalised as 16-byte
  identity hashes through `propagation_enable`, `allow_control`, and
  `disallow_control`, then reported with propagation status and SDK recovery
  state.
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
- Remote peer-sync failure events expose a structured `failure_kind` on both
  the top-level event and nested propagation payload, so observers can
  distinguish throttling, identity, key, data, stamp, not-found, timeout,
  access-denied, and generic failures while the retry state machine remains
  unchanged.
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
- Propagation announce/app-data handling is daemon-state driven: node app-data,
  stamp cost, transfer/sync policy, and auth-required state are regenerated from
  local propagation state, while router-side resource completion/failure updates
  peer counters, receipt state, and SDK-visible lifecycle metadata.
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
- Remote fetch/download/sync imports reject payloads for ignored destinations
  during batch validation, so remote relay responses cannot bypass local
  replication policy or queue ignored work to peers.
- Selected local peer-sync offer responses validate the full selected
  propagation response payload batch before marking any selected ID transferred,
  so malformed queued payloads cannot partially drain peer retry state.
- Ordinary full-offer peer sync validates the propagation payload batch before
  marking any queued ID transferred, so a later malformed queued payload cannot
  partially drain peer retry state.
- Malformed remote fetch and download imports mirror existing payload-backed
  queue marks into active peer record snapshots before returning the import
  failure, so already queued relay work remains visible after restart/export.
- Malformed remote fetch and download imports from an already active source
  peer update that peer's failure backoff and publish the failed peer-sync
  event, so invalid post-transfer payloads use the same retry observability as
  transport-level remote transfer failures.
- Remote fetch and download bridge failures mirror existing payload-backed
  queue marks into active peer record snapshots before returning the failure,
  so already queued relay work remains visible after restart/export.
- Remote fetch and download bridge failures from an already active source peer
  also update that peer's failure backoff and publish the failed peer-sync
  event, aligning retry scheduling and observability with the preserved queue
  snapshot.
- Remote fetch and download access-denied bridge failures follow the remote
  peer-sync denial path for the source peer, clearing local peering and queued
  propagation marks instead of preserving denied relay work for retry, while
  preserving the propagation `no_access` lifecycle state and bridge error text.
- Access-denied remote transfer cleanup emits peer-unpeer events with the
  stored peer identifier even when the remote request uses alternate casing,
  keeping event observability tied to the removed peer record.
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
  advance that peer's retry backoff, publish the failed peer-sync event, and
  mark the propagation sync lifecycle failed, keeping queued retry state
  observable without creating new peers.
- Peer sync RPC rows and events preserve the Python-compatible peer `state`
  namespace while exposing backoff and policy postponement through separate
  scheduling fields; failed attempts continue to use the established error state.
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
- Remote peer-sync bridge results that explicitly report `synced: false` or
  `postponed: true` preserve that remote postponement in the peer-sync
  result/event and keep retry scheduling intact instead of clearing peer
  backoff as a completed transfer.
- Failed remote unpeer attempts mirror existing payload-backed queue marks and
  restored peer-record queue IDs into active peer record snapshots before
  returning bridge-unavailable or bridge-execution errors, including
  case-insensitive peer requests, so failed peering teardown preserves queued
  retry work across restart/export and marks the propagation lifecycle failed
  instead of leaving stale idle/completed state.
- Failed remote unpeer bridge-unavailable errors for active peers publish the
  failed peer-sync event after queue snapshot refresh, keeping observer-visible
  peering failure state aligned with remote sync/fetch/download
  bridge-unavailable failures.
- Failed remote unpeer bridge-execution errors for active peers advance the
  peer's retry backoff window before refreshing queue snapshots, so failed
  peering teardown does not leave retryable queue work in an immediate retry
  loop.
- Failed remote unpeer bridge-execution errors for active peers publish the
  failed peer-sync event after queue snapshot refresh, keeping observer-visible
  peering failure state aligned with remote sync/fetch/download failures.
- Access-denied remote unpeer failures follow the same local peering break path
  as access-denied remote sync/fetch/download, clearing local peer and
  propagation queue state instead of leaving denied teardown work retryable.
- Successful remote unpeer uses the stored peer ID case for the bridge call and
  nested bridge result when callers supply a case-variant peer request, keeping
  remote teardown identity aligned with local queue cleanup.
- Successful remote unpeer clears stale propagation lifecycle failures and
  error text left by earlier teardown attempts, so peer removal is not reported
  alongside an obsolete failed control state.
- Inbound reticulumd `/pn/peer/sync` and `/pn/peer/unpeer` control commands
  resolve stored peer IDs case-insensitively before dispatching to daemon RPCs,
  so binary peer-control requests do not report not-found for restored or
  configured peers whose status rows preserve a different hex presentation;
  `/pn/peer/sync` also checks hidden unpeered peer records so operator-triggered
  rejoin paths can reach the daemon reactivation state machine.
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
- Peer sync offer acceptance validates all transfer payload hex before marking
  any offered payload transferred, handled, or transfer-limited, so malformed
  response batches cannot partially mutate live marks or serialized
  restart/export queue snapshots.
- Peer sync wanted-ID list responses canonicalize and deduplicate repeated
  transient IDs before transfer selection and accounting, so duplicate response
  entries cannot inflate transfer counts, byte totals, or acceptance rate.
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
- The typed ZeroMQ SDK backend exposes real Reticulum service-identity
  create/list/activate/import/export plus targeted identity announces. Each
  authenticated session is restricted to identities it created or proved by
  private-key import, and each identity retains its own `lxmf.delivery`
  destination, signer, display name, capabilities, metadata, inbound queue,
  and restart-persistent key while the daemon transport identity remains
  independent. Presence, resolve, contact, and bootstrap calls remain on the
  same `ZmqPipelineBackendClient` path for REM/RCH.
- The typed ZeroMQ SDK backend exposes
  `ZmqPipelineBackendClient::identity_announce` for capability-rich announces,
  preserving local identity, display name, callsign, REM capability flags, RCH
  announce-slot metadata, and extensions over `sdk_identity_announce_now_v2`
  while retaining `identity_announce_now` for empty compatibility announces.
- The typed ZeroMQ SDK backend exposes
  `ZmqPipelineBackendClient::workflow_peer_ready` for saved-peer setup,
  preserving display names, callsigns, trust, bootstrap intent, and REM/RCH
  capability metadata while optionally announcing before use.
- The typed ZeroMQ SDK backend exposes
  `ZmqPipelineBackendClient::peer_directory`, merging saved contacts and
  announce-derived presence over typed ZeroMQ SDK methods while preserving
  display names, callsigns, REM capability flags, RCH announce-slot metadata,
  online state, and first/last-seen timestamps.
- The typed ZeroMQ SDK backend exposes
  `ZmqPipelineBackendClient::peer_directory_since` plus
  `min_last_seen_ts_ms` filtering on `sdk_identity_presence_list_v2`, allowing
  stale announce rows to be hidden without dropping saved offline contacts.
- The typed ZeroMQ SDK backend exposes saved-peer lifecycle calls through
  `ZmqPipelineBackendClient::peer_connect`, `peer_disconnect`, and
  `peer_reconnect`, preserving identity, display name, correlation ID,
  callsign, REM capability flags, RCH announce-slot metadata, and extensions
  over `sdk_peer_connect_v2`, `sdk_peer_disconnect_v2`, and
  `sdk_peer_reconnect_v2`.
- The typed ZeroMQ SDK backend exposes the operation registry and envelope
  execution path, including the `app.message.history.list` and
  `app.delivery.destination_hash` operations used by direct-chat history and
  runtime delivery-destination queries, so REM/RCH can keep those flows on the
  `ZmqPipelineBackendClient` path instead of requiring raw RPC/HTTP envelopes.
- Paper-message encode/decode now appears in both the daemon and SDK app
  operation registries as `app.paper.encode`/`app.paper.decode`, preserving
  the `sdk_paper_encode_v2`/`sdk_paper_decode_v2` aliases and typed envelope
  payloads instead of falling through to generic remote-command dispatch.
- The typed SDK paper decode surface now keeps the legacy `paper_decode` Ack
  path while adding `paper_decode_with_metadata`, exposing daemon paper-ingest
  metadata (`transient_id`, duplicate state, destination, destination hint, and
  byte length) through the RPC and ZeroMQ backends. Duplicate bridge-backed
  paper decodes now also publish one bounded `inbound_dropped` event without
  emitting a second inbound event or storing the message again.
- The `lxmf`/`lxmf-cli` command surface now exposes the same paper operation
  path with `paper-encode --message-id` and `paper-decode --uri`, keeping CLI
  paper handling on the typed SDK calls instead of requiring raw RPC envelopes.
- The typed ZeroMQ SDK backend also exposes durable direct-chat history as
  `ZmqPipelineBackendClient::list_message_history`, preserving link-bearing
  message bodies, receipt status, basic LXMF fields, one-to-one
  `peer_id`/`conversation_id` filters, `include_receipts`, and daemon
  pagination cursors for restart recovery through the
  `app.message.history.list` SDK envelope path.
- The typed ZeroMQ SDK backend exposes durable direct-chat conversation
  summaries through `ZmqPipelineBackendClient::list_conversations`, preserving
  peer display names, unread counts, last-message previews with links, receipt
  inclusion intent, and restart pagination cursors through
  `app.message.conversation.list` on the SDK envelope path.
- The native SDK app domain now exposes the same durable message-list,
  conversation-list, and cancellation flows as `app.messages().history(...)`,
  `app.messages().conversations(...)`, and `app.messages().cancel(...)`, so
  direct-chat clients can stay on the existing SDK client surface without raw
  envelope decoding or dropping to the root client handle.
- `ZmqPipelineBackendClient::list_message_history` accepts canonical
  `id`/`content` history rows and legacy direct-chat `message_id`/`body` rows,
  so recovered history remains typed even when the daemon returns the older app
  chat field names.
- The typed ZeroMQ SDK backend exposes the local runtime delivery destination
  through `ZmqPipelineBackendClient::local_delivery_destination_hash` while
  retaining `app.delivery.destination_hash` envelope execution, so direct-chat
  source selection can stay on the typed ZeroMQ SDK path.
- The typed ZeroMQ SDK backend preserves negotiated receipt terminality when
  mapping `sdk_status_v2` into `DeliverySnapshot`, so direct-chat delivery
  status reports `sent` as terminal only until
  `sdk.capability.receipt_terminality` is negotiated.
- The native SDK app facade now routes `app.delivery.cancel` locally through
  `Client::cancel_delivery`, preserving typed cancellation results for app
  callers instead of falling through to generic remote-command dispatch.
- The typed ZeroMQ SDK backend preserves daemon-reported retry-attempt counts
  and reason codes when mapping `sdk_status_v2` into `DeliverySnapshot`, so
  direct-chat restart and retry recovery state remains visible to REM/RCH over
  `ZmqPipelineBackendClient`.
- The typed ZeroMQ SDK backend exposes burst sends through
  `ZmqPipelineBackendClient::send_batch` and also supports
  `app.delivery.send_batch` envelope execution, preserving ordered per-message
  acceptance and rejection results without raw RPC envelopes.
- `BatchSendItem` preserves per-message idempotency keys, TTL, correlation IDs,
  and SDK extensions in each batch message's `_sdk` field metadata, keeping
  burst direct-chat retry and restart recovery state on the typed SDK path.
- The typed ZeroMQ SDK backend and operation registry expose direct-chat
  cancellation through both `ZmqPipelineBackendClient::cancel` and
  `app.delivery.cancel` envelope execution, preserving daemon cancellation
  outcomes for REM/RCH without raw RPC envelopes.
- `app.delivery.cancel` now covers queued/pre-handoff outbound work: accepted
  cancellation persists `receipt_status = cancelled`, records delivery trace
  and event state, exposes cancel metadata through both delegated
  `sdk_cancel_message_v2` and `sdk_envelope_execute_v2` lifecycle traces, and
  prevents a later outbound bridge handoff after worker lanes resume. ZeroMQ
  tests cover all cancel result variants plus non-accepted envelope payload and
  extension preservation.
- The typed ZeroMQ SDK backend starts the final propagation-first branch with
  `ZmqPipelineBackendClient::propagation_peer_sync`, routing
  `app.propagation.peer_sync` over `sdk_envelope_execute_v2` to the daemon's
  existing `peer_sync` lifecycle while preserving offer, transfer, postponed,
  retry, and persistent queue metadata in the typed response.
- `PropagationPeerSyncResult` now projects daemon `messages` and `propagation`
  queue fields into a typed `queue` snapshot, including offered/outgoing/
  incoming/unhandled counters and handled, unhandled, transferred, skipped,
  rejected, and transfer-limited transient IDs while retaining raw payloads.
- The typed peer-sync `queue` snapshot now also exposes transferred, skipped,
  rejected, and transfer-limited counters plus their byte totals, so retry and
  sync-limit callers do not need raw propagation JSON for queue accounting.
- `PropagationPeerSyncResult` now falls back to propagation-level transfer and
  sync limits and exposes target stamp cost plus stamp cost flexibility, so
  propagation policy metadata stays typed for REM/RCH clients.
- `PropagationPeerSyncResult` now also exposes typed failure kind,
  timeout/access-denied classification, and existing retry scheduling fields
  for postponed peer-sync attempts, so offer and queue retry callers do not
  need raw propagation JSON for common failure branching.
- `PropagationPeerSyncResult` now also falls back to propagation-level
  `postponed` and `postpone_reason` fields when the peer-sync envelope omits
  top-level values, keeping remote nested peer-sync retry state fully typed.
- The same typed ZeroMQ SDK propagation branch now covers remote router status,
  fetch, download, sync, and unpeer lifecycle calls through registered
  `app.propagation.*` envelopes, preserving daemon propagation, peer-sync,
  transfer, denial, timeout, and queue-cleanup payloads for REM/RCH clients
  without raw RPC envelopes.
- `PropagationRemoteSyncResult` now also projects nested remote-sync
  `peer_sync` payloads into typed `peer_sync_state`, so remote propagation sync
  callers can inspect sync status and queue transient IDs without parsing raw
  JSON while still retaining the original daemon payload.
- `PropagationRemoteSyncResult` now also projects top-level remote-sync
  propagation cleanup IDs into a typed `queue` snapshot, so transferred,
  skipped, rejected, and transfer-limited sync work is visible without raw
  propagation JSON even when nested peer-sync state is incomplete.
- `PropagationRemoteSyncResult` now also projects its propagation lifecycle and
  result payloads into typed `transfer_state`, so sync timeout, denial, retry,
  next-attempt, and last-error handling are visible without raw propagation
  JSON parsing.
- `PropagationRemoteStatusResult` now projects remote router status into typed
  `status_state`, covering lifecycle state, selected node/peer, queue depth,
  failure kind, timeout/access-denied classification, retry count, next sync
  attempt, and last error while preserving raw status JSON.
- `PropagationRemoteTransferResult` now projects remote fetch/download result
  and propagation lifecycle payloads into typed `transfer_state`, covering
  sync/postpone status, imported IDs/counts, transferred bytes, progress, and
  last error while retaining the original daemon JSON.
- `PropagationRemoteTransferResult` now also projects remote fetch/download
  propagation queue IDs into typed `queue`, so transferred, skipped, rejected,
  and transfer-limited transient IDs are visible without raw propagation JSON.
- `PropagationRemoteTransferState` now also exposes failure kind, timeout and
  access-denied booleans, retry count, and next sync attempt for remote
  fetch/download results, so clients can branch on denial and timeout recovery
  without parsing raw propagation JSON.
- `PropagationRemoteTransferState` now also exposes `last_sync_started` and
  `last_sync_completed` for remote fetch/download/sync/unpeer lifecycle
  results, keeping transfer freshness visible without raw propagation JSON.
- `PropagationRemoteTransferState` now also exposes selected router context
  through `selected_node` and `selected_peer` for remote fetch/download/sync/
  unpeer lifecycle results, keeping peer/router selection visible without raw
  propagation JSON.
- Remote fetch/download/sync/unpeer SDK envelopes convert denied, timed out,
  and retryable bridge failures into typed result payloads with daemon
  propagation recovery state, so REM/RCH clients can keep propagation recovery
  on `ZmqPipelineBackendClient` instead of handling raw RPC errors.
- `PropagationRemoteUnpeerResult` now projects remote unpeer `messages` and
  propagation cleanup payloads into a typed `queue` snapshot, so denial and
  teardown cleanup callers can inspect handled, unhandled, transferred,
  skipped, rejected, and transfer-limited IDs without parsing raw JSON.
- `PropagationRemoteUnpeerResult` now also projects teardown lifecycle payloads
  into typed `transfer_state`, so denied or failed unpeer attempts expose
  failure kind, access-denied/timeout classification, retry scheduling, and
  last error without parsing raw propagation JSON.
- The same branch now covers propagation sync completion/failure
  acknowledgement through
  `ZmqPipelineBackendClient::propagation_acknowledge_sync_completion` and
  `app.propagation.acknowledge_sync_completion`, keeping retry, timeout, and
  restart recovery state visible through the typed ZeroMQ SDK path.
- `PropagationStatusResult` and `PropagationAcknowledgeSyncResult` now project
  their propagation payloads into typed `recovery_state`, so status, enable,
  and acknowledgement callers can inspect sync state, retry counts, queue
  depth, and last error without parsing raw JSON.
- `PropagationRecoveryStateResult` now also exposes failure kind, timeout and
  access-denied booleans, and next sync attempt, so local recovery and sync
  acknowledgement callers can branch on denial/timeout handling without raw
  propagation JSON.
- The same typed propagation branch now covers outbound propagation router
  get/set/list through `ZmqPipelineBackendClient::propagation_node_get`,
  `propagation_node_set`, and `propagation_node_list`, backed by
  `app.propagation.node.*` envelopes that preserve selected-node and node-list
  metadata for REM/RCH router lifecycle flows.
- `PropagationNodeListResult` now projects listed router candidates into typed
  `PropagationNodeRecord` entries, exposing peer, display name, last-seen time,
  selected flag, and capability strings while retaining the raw node JSON.
- `PropagationNodeSelectionResult` now projects node get/set `meta` into typed
  `selection_state`, exposing selected peer, selection flag, queue depth,
  failure kind, timeout/access-denied classification, retry scheduling, and
  last error without parsing raw router metadata.
- The same typed propagation branch now covers local propagation status,
  enable/config, delivery policy get/set, and peer maintenance through
  `ZmqPipelineBackendClient` methods and `app.propagation.*` envelopes, keeping
  policy, stale-peer cleanup, and retry/maintenance state available without raw
  RPC calls.
- `PropagationPeerMaintenanceResult` now projects maintenance-triggered
  `peer_sync` payloads into typed `peer_sync_state`, so stale-peer cleanup and
  automatic retry/rotation callers can inspect sync timing and queue transient
  IDs without parsing raw JSON.
- The same typed propagation branch now covers local propagation payload ingest
  and fetch through `ZmqPipelineBackendClient::propagation_ingest` and
  `propagation_fetch`, preserving transient IDs, payload bytes, duplicate
  accounting, and durable store recovery for disconnected-client relay flows.
- `PropagationIngestResult` and `PropagationFetchResult` now also preserve
  daemon propagation lifecycle payloads and project them into typed
  `recovery_state`, so disconnected-client ingest/fetch callers can inspect
  selected node, sync state, queue depth, and local ingest/serve counters
  without parsing raw propagation JSON.
- `PropagationDeliveryPolicyResult` now projects delivery policy payloads into
  typed `policy_state`, so propagation-first clients can inspect auth-required
  mode plus allowed, denied, ignored, and prioritised destination sets without
  parsing raw policy JSON.
- The router policy surface now also exposes Python-style incremental
  `set_authentication`, `requires_authentication`, `allow`, `disallow`,
  `ignore_destination`, `unignore_destination`, `prioritise`, and
  `unprioritise` operations through legacy RPC aliases and SDK envelope
  operation IDs. The daemon aliases validate 16-byte destination hashes and
  suppress duplicate list entries, while the typed ZeroMQ helpers preserve
  unrelated auth, allow, deny, ignore, and priority fields through get/set.
- The same typed propagation branch now exposes
  `ZmqPipelineBackendClient::propagation_recovery_state`, projecting
  `app.propagation.status` into structured sync state, selected-node,
  last-error, retry count, queue depth, timestamp, and local ingest/serve
  counters while keeping the raw propagation payload available for queue
  diagnostics.
- `PropagationRecoveryStateResult` now also exposes the propagation lifecycle
  `timestamp`, so restart/recovery status callers can inspect daemon recovery
  freshness without parsing raw propagation JSON.
- `PropagationRecoveryStateResult` now also exposes local propagation config
  fields for `auth_required`, `control_allowed`, `static_peers`, and
  `sync_limit`, so status and enable/config callers can verify recovery policy
  without raw propagation JSON.
- `PropagationRecoveryStateResult` now also exposes propagation storage and
  transfer-limit config for `store_root`, `target_cost`,
  `message_storage_limit_mb`, and `propagation_limit`, keeping durable queue
  policy visible on the typed ZeroMQ SDK path.
- `PropagationRecoveryStateResult` now also exposes the remaining propagation
  enable/status config for `stamp_cost_flexibility`, `delivery_limit`,
  `autopeer`, `autopeer_maxdepth`, `max_peers`, `from_static_only`,
  `retain_synced_on_node`, `peering_cost`, and `remote_peering_cost_max`, so
  router/peering policy is visible through the typed ZeroMQ SDK path.
- Python-style `lxmd` `[lxmf] announce_interval` drives peer/delivery announce
  cadence separately from `[propagation] announce_interval`, which remains the
  propagation-node announce cadence.
- Delivery announce application data uses the shared `lxmf-wire` structured
  encoder/decoder: the Python-compatible display-name and stamp-cost positions
  remain stable, while optional application capability strings occupy an
  extension metadata slot. `reticulumd` and embedded consumers use the same
  codec instead of encoding capabilities in callsigns or private text formats.
- Outbound propagated delivery resolves selected propagation-node
  `propagation_stamp_cost` case-insensitively, so Python-style hash casing does
  not fall back to the default propagation stamp cost.
- Direct `reticulumd` propagation-node config activates Python-shaped
  propagation/control destinations, exposes outbound propagation cost lookup,
  and stores self-selected propagated payloads locally instead of trying to
  activate a link to itself.
- The live Rust/Python remote-relay interop gate now selects a Python `lxmd`
  propagation destination as the Rust outbound propagation node, covering mixed
  propagation-node discovery and selection before broader store-and-forward
  claims are made.
- The live Rust/Python propagation-control gate now also exercises
  Python-origin `/offer` requests against Rust `reticulumd`, proving partial
  wanted-ID responses, duplicate wanted-ID response canonicalization,
  repeated-offer throttling, and source-peer completed marks across the live
  link request path.
- The live Rust/Python propagation-control gate now also splits out a
  Python-origin `/offer` peer-queue lifecycle case, proving post-sync handled
  IDs, no retryable missing-ID queue state, and cleared sync backoff after
  transfer creates the Rust peer row.
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
- The live Rust/Python propagation-control gate now exercises a Python-origin
  `/get` haves-only request against Rust `reticulumd`, proving the `true`
  acknowledgement, retained-payload purge, and absence of retryable unhandled
  peer queue state across the live link request path.
- Inbound propagation message-get `haves` handling records matched haves as
  received/completed work for the requesting propagation peer after purge, so
  reintroduced payloads are not queued back to peers that already declared
  them.
- Inbound propagation message-get `haves` handling records stale peer-acknowledged
  IDs even when local payload rows are already absent, while still honoring
  `retain_synced_on_node` payload-retention behavior so completed peers are
  marked without regressing local payload reuse.
- Inbound propagation message-get `haves` completion is now constrained to
  locally known payloads or existing peer queue marks, so arbitrary unknown
  haves cannot pre-complete future propagation work for that peer.
- Link-based remote propagation downloads wait for the final haves
  acknowledgement response after imported or duplicate payloads are reported,
  so node-side rejection or timeout is surfaced instead of reporting a
  completed download before remote cleanup is confirmed.
- Link-based propagation-control waits now surface matching resource transfer
  failure or cancellation immediately, so remote fetch/download callers see the
  terminal transfer state instead of a generic response timeout.
- Inbound propagation message-get purge-only requests return the Python-style
  boolean success response after haves are applied, and payload purge cleanup
  preserves completed peer accounting for other peers while removing stale
  unhandled marks, so reintroduced payloads are not offered back to peers that
  already completed them.
- Propagation nodes honor `retain_synced_on_node` during message-get haves
  handling: requesting peers are still marked completed, while retained payloads
  remain stored and queued for peers that have not completed them; retained
  payload listings now filter IDs already completed by the requesting peer.
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
- Inbound propagation message-get requests validate every wanted and have
  transient ID instead of silently filtering malformed entries, so malformed
  mixed `/get` lists cannot fetch or purge queue state behind the rejected
  request.
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
- Propagation peer maintenance expires local processed-transient markers after
  Python's six-message-expiry cache window, preventing duplicate suppression
  state from retaining stale transient IDs indefinitely.
- Propagation-node ingest enforces the configured message-storage byte limit
  against retained propagation entries, using age, size, and
  prioritised-destination weighting while pruning stale retryable peer marks.
- Link-based remote downloads wait for the propagation node's `/get` haves
  acknowledgement and propagate peer/control errors, so failed remote cleanup is
  not reported as a completed replication drain.
- Link-based remote fetch and download preserve propagation-node-advertised
  transient IDs for imported payloads and haves acknowledgement instead of
  recomputing IDs from returned payload bytes, matching Python store-and-forward
  behavior for payloads whose body could otherwise be mistaken for stamped
  material.
- Link-based remote propagation control waits surface authenticated link-close
  peer/control signals immediately, so denied or closed remote fetch/download/sync
  requests fail on the signal instead of waiting for the request timeout.
- Remote fetch/download acknowledgements use canonical propagation transient
  IDs for stamped payloads, so `/get` haves clear the peer's offered queue entry
  instead of reporting the stamped payload bytes under a different hash.
- Repeated remote fetch/download/sync imports increment source peer incoming
  counts and receive bytes only for payload IDs not already marked received
  from that source, while still replaying known payloads into relay queues when
  their live marks were cleared.
- Remote fetch/download bridge envelopes that successfully return `postponed`
  or `synced: false` preserve the failed transfer lifecycle, source-peer
  backoff, failed peer event, and retryable queue snapshot instead of treating
  the transfer as an empty completed import.
- Successful remote fetch/download imports clear stale retry backoff on an
  active source peer after newly accepted payloads, so recovered propagation
  sources are not left postponed by an earlier failed transfer attempt.
- Successful remote fetch/download imports also refresh the active source
  peer's sync-attempt timestamp while clearing stale backoff, so status and
  restart/export views do not retain an obsolete failed transfer attempt time.
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
- Restart-visible peer rows replay restored handled and unhandled snapshots
  before reporting queue counters and ID lists, so payload-backed unhandled
  work survives reload, completed work stays handled, and missing payload IDs
  are pruned before `list_peers` exposes peer state.
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
- Propagation ingest rejects payloads for ignored destinations before storing
  or queueing them, emits a bounded `inbound_dropped` event through the RPC/SDK
  event stream, and enforces local replication policy before relay state is
  created.
- Local peer offer-error responses now expose failed peer-sync state fields at
  both the top-level event/result and nested propagation result while keeping
  retryable queue marks intact.
- Retryable local peer offer-error responses now advance the ordinary peer
  sync backoff window and expose structured `failure_kind` fields at both the
  top-level event/result and nested propagation result while preserving queued
  work for retry.
- Inbound propagation distinguishes clients, validated peers, unpeered
  identified senders, and local delivery; source peers are accounted and not
  re-offered their own payloads.
- These behaviors close the pinned Python-only `LXMPeer.py` lifecycle row.
- Propagation router lifecycle coverage now also includes persistent restart
  replay after real remote fetch/download/sync mutations, retryable and
  terminal failure classification for fetch/download, success side-effect
  matrices for remote fetch/download/sync, transfer-limit precedence, and live
  Python remote fetch/download/sync cases against pinned `lxmd`.

## Highest-Priority Gaps

1. Validate external clients before making client-specific claims.
2. Continue widening non-propagation router convenience coverage only where it
   affects supported clients.

## Evidence

- `.github/workflows/verify.yml` runs pinned Python reference
  conformance plus live channel, paper, compatibility-matrix, and LXMD
  remote-relay tests.
- The compatibility matrix includes ignored live `propagation_remote_status_bidir`,
  `propagation_remote_fetch_rust_to_python`,
  `propagation_remote_download_rust_to_python`, and
  `propagation_remote_sync_rust_to_python` cases for Rust-to-Python
  propagation-control status and remote lifecycle coverage when the Python
  harness environment is available.
- The compatibility matrix also includes Python-origin
  `propagation_get_haves_python_to_rust`, `propagation_offer_python_to_rust`,
  `propagation_offer_queue_python_to_rust`, and
  `propagation_offer_duplicate_wanted_source_completed_python_to_rust` cases
  for haves-only `/get` side effects, offer side effects, duplicate wanted-ID
  handling, and peer queue lifecycle evidence.
- Focused daemon/RPC tests cover delivery modes, propagation offers, paper
  operation registry/envelope dispatch, SDK ticket-generation envelopes, peer
  maintenance, queue policy, source accounting, stamps, tickets, receipts, and
  cancellation.
- Focused daemon bridge tests cover deferred normal-stamp queue ownership,
  cancellation, retry metadata, and propagation-stamp preparation before
  delivery handoff.
- Focused direct packet/resource delivery tests cover successful inbound
  `sdk_poll_events_v2` callback observability, including raw
  `lxmf_bytes_hex`, stored/event identity, content, and metadata consistency,
  direct transport metadata, and verified signature metadata for packet and
  resource deliveries.
- Focused direct packet/resource delivery tests cover structured raw
  `inbound_dropped` event-stream entries for malformed direct LXMF payloads,
  including default identifier redaction and `delivery_kind` classification,
  without storing a message or updating peer activity. Direct stamp-policy and
  delivery-policy rejections route through the same bounded drop-event helper.
- Focused paper decode tests cover bridge-backed duplicate paper ingests
  emitting `delivery_kind = "paper"` `inbound_dropped` events through
  `sdk_poll_events_v2` while preserving duplicate metadata and avoiding a
  second raw inbound event.
- The native SDK app event mapper now exposes typed inbound message, inbound
  drop, receipt, and delivery-lifecycle helpers on the existing event path,
  preserving message IDs, peer/source identity, destination hashes, raw LXMF
  bytes, signature/stamp metadata, drop reason, remote propagation
  operation/transient/peer context, receipt status, and lifecycle state without
  requiring normal client UI flows to decode raw event JSON.
- Typed inbound message helpers now expose the richer handler metadata already
  carried in raw inbound events: signature validity, stamp validity,
  propagation stamp validity, LXMF method, and direct transport encryption
  fields.
- Receipt lifecycle events preserve handler/bridge stage provenance in both
  the pollable SDK payload and typed app lifecycle helper, so transport-origin
  delivery receipts remain distinguishable without raw event JSON parsing.
- Focused propagated local-delivery tests cover ignored-source delivery-policy
  rejections emitting bounded raw `inbound_dropped` events with
  `delivery_kind = "propagation"` while preserving Python-style no-store
  behavior and default identifier redaction. Coverage includes local envelope
  ingest plus decryptable remote fetched and remote downloaded propagation
  payloads that reach local decode, stamp, or delivery-policy handling, so
  those router-coupled local-delivery drops do not disappear into rejected
  counters without observer-visible context.
- Focused propagated pre-decode tests cover local-addressed too-short local
  envelopes through `sdk_poll_events_v2` and strict remote fetch/download
  local-import rejects for too-short payloads, destination mismatches, and
  decrypt failures. Local propagation envelope destination mismatches remain
  relay/store-forward candidates instead of local drop events.
- Focused RPC propagation tests cover ignored-destination rejects from
  `propagation_ingest`, Python-served alias ingest, and remote
  fetch/download/sync imports, asserting no payload storage, `PermissionDenied`
  preservation, bounded `inbound_dropped` events, default identifier redaction,
  and SDK event-poll visibility where the event stream is part of the ingest
  contract.
- Focused propagated local-delivery tests also cover handler-facing `_lxmf`
  signature metadata on stored messages and raw inbound events for local
  envelope ingest, remote fetch imports, and remote-control download imports.
  Production imports pass through the shared transport-aware signature
  annotation path when available, while unknown source identities are recorded as
  `source_identity_unknown` instead of leaving signature status absent.
- Focused local propagated-delivery tests feed a real source announce through
  the transport interface channel before delivery, proving known-source
  signatures report `verified` and corrupted signatures report
  `signature_invalid` on both stored records and raw inbound events.
- Focused local propagated-delivery tests now also cover the Python-style
  processed-transient side effect: accepted local propagated delivery marks the
  transient in `propagation_local_entries`, and later `propagation_ingest` of
  the same stamped transient reports duplicate accounting without incrementing
  local receive counters again. Replayed local propagated delivery for an
  already processed transient, or for an already stored message carried by a
  fresh transient, emits one bounded `inbound_dropped` duplicate event while
  preserving the same no-store/no-recount behavior. Propagation maintenance
  now prunes those local processed-transient markers after the Python
  six-message-expiry cache window.
- Propagation announce handling now mirrors Python handler activation: valid
  `lxmf.propagation` announces are recorded but do not create peers or queue
  propagation entries unless local propagation handling is active.
- Focused remote propagation import tests now cover duplicate observability for
  same-response duplicate payloads across sync/fetch/download and already
  processed remote fetch payloads: still-stored duplicates remain accepted for
  peer queue side effects, processed-only duplicates avoid unservable peer
  marks, and both emit bounded `inbound_dropped` duplicate events without
  duplicate storage/upsert work.
- `interop.python_live_gate` means the configured scenarios run successfully;
  it does not imply every partial row is complete.
