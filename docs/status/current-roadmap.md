# Current Roadmap Status

Last reassessed: 2026-06-18

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
| Propagation interoperable | achieved | Propagated delivery, complete Python-only `LXMPeer.py` lifecycle coverage, and Python-reference propagation router fetch/download/sync lifecycle coverage are implemented and tested. |
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
- Documented basic LXMF field IDs are exported through `lxmf-wire`, and the
  typed ZeroMQ SDK send path preserves those field keys plus
  `_lxmf_fields_msgpack_b64` for REM/RCH payload compatibility.
- The typed ZeroMQ SDK send and batch-send paths now treat payload `body` as
  message content when `content` is absent, while still preserving `body` in
  fields, so direct-chat links/body text do not get JSON-stringified.
- Delivery modes are honored by the daemon; the old claim that requested modes
  are ignored is obsolete.
- Direct and propagated resource sends support receipt-state separation,
  timeout/failure propagation, and active resource cancellation.
- Link sends now register packet/resource receipt tracking before handoff and
  accept Python-style link proofs with default packet context, so Python
  delivery receipts can advance daemon-originated sends from `sent:*` to
  `delivered` while preserving resource completion status.
- The typed ZeroMQ SDK delivery status path now preserves daemon-reported
  retry-attempt counts and reason codes in `DeliverySnapshot`, so REM/RCH can
  inspect retry and recovery state without dropping to raw RPC status calls.
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
- Remote fetch/download/sync imports now also reject payloads for ignored
  destinations during batch validation, so remote relay responses cannot bypass
  local replication policy or queue ignored work to peers.
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
- Remote fetch and download bridge envelopes that return successfully while
  reporting `postponed` or `synced: false` now preserve the failed transfer
  lifecycle, source-peer backoff, peer event, and queue snapshot instead of
  importing an empty result and marking propagation complete.
- Successful remote fetch and download now also mirror existing payload-backed
  live queue marks into active peer record snapshots after applying imports, so
  restart/export state preserves queued retry work even when the remote
  transfer succeeds without consuming those local queued offers.
- Successful remote fetch and download now clear stale retry backoff on the
  active source peer when newly accepted payloads prove the source recovered,
  so later maintenance does not keep postponing a healthy replication peer.
- Successful remote fetch and download now also refresh the active source
  peer's sync-attempt timestamp while clearing stale backoff, so restart and
  status views reflect the successful recovery attempt instead of an obsolete
  failed transfer time.
- Remote peer-sync backoff postponements now mirror existing payload-backed live
  queue marks into active peer record snapshots before returning, so
  restart/export state preserves queued retry work even when sync is deferred.
- Remote peer-sync bridge-unavailable errors now mirror existing payload-backed
  live marks and restored peer-record queue IDs into active peer record
  snapshots for already known peers before returning, including
  case-insensitive requests, while still avoiding peer creation when the bridge
  is absent.
- Remote peer-sync bridge-unavailable errors for already known peers now also
  advance that peer's retry backoff, publish the failed peer-sync event, and
  mark the propagation sync lifecycle failed, keeping queue retry state
  observable without creating new peers.
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
- Remote peer-sync bridge results that explicitly report `synced: false` or
  `postponed: true` now preserve the remote postponement in the peer-sync
  result/event and keep retry scheduling intact instead of clearing the peer's
  backoff as if the transfer completed.
- Failed remote unpeer attempts now mirror existing payload-backed live queue
  marks and restored peer-record queue IDs into active peer record snapshots
  before returning bridge-unavailable or bridge-execution errors, including
  case-insensitive peer requests, so restart/export state preserves queued retry
  work when peering teardown fails; these failed attempts also mark the
  propagation lifecycle failed instead of leaving stale idle/completed state.
- Failed remote unpeer bridge-unavailable errors for active peers now also
  publish the failed peer-sync event after queue snapshot refresh, keeping
  observer-visible peering failure state aligned with remote sync/fetch/download
  bridge-unavailable failures.
- Failed remote unpeer bridge-execution errors for active peers now also
  advance the peer's retry backoff window before refreshing queue snapshots, so
  failed peering teardown does not leave retryable queue work in an immediate
  retry loop.
- Failed remote unpeer bridge-execution errors for active peers now also
  publish the failed peer-sync event after queue snapshot refresh, keeping
  observer-visible peering failure state aligned with remote sync/fetch/download
  failures.
- Access-denied remote unpeer failures now follow the same local peering break
  path as access-denied remote sync/fetch/download, clearing local peer and
  propagation queue state instead of leaving denied teardown work retryable.
- Successful remote unpeer now also uses the stored peer ID case for the bridge
  call and nested bridge result when callers use a case-variant peer request,
  keeping remote teardown identity aligned with local queue cleanup.
- Successful remote unpeer now clears stale propagation lifecycle failures and
  error text left by earlier teardown attempts, so status reflects completed
  peer removal instead of a prior failed control operation.
- Active outbound normal and propagation stamp generation now reports stored
  generation progress through `get_outbound_progress`, while terminal failed or
  cancelled stamp states continue to suppress stale progress values.
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
- Peer sync offer acceptance now validates all transfer payload hex before
  marking any offered payload transferred, handled, or transfer-limited, so a
  malformed response batch cannot partially mutate live marks or serialized
  restart/export queue snapshots.
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
- Direct `reticulumd` `[propagation_node]` config now activates the
  Python-shaped propagation/control destinations, advertises configured stamp
  costs, exposes outbound propagation cost lookup, and stores self-selected
  propagated payloads locally instead of linking to its own node.
- Normal and propagation stamp retry metadata now clears stale stamp error
  fields when later work re-enters generating/ready state, so status no longer
  reports a prior failed attempt after a successful retry.
- Peer sync queue creation also records newly queued existing propagation IDs in
  the peer record snapshot, so postponed syncs can restart/export with the same
  unhandled queue visible in live status.
- Local peer offer-error responses now publish failed peer-sync state fields at
  both the top-level peer event and nested propagation result while preserving
  the retryable peer queue, improving parity with the peer sync state machine.
- Ordinary full-offer peer sync now validates the propagation payload batch
  before marking any queued ID transferred, so a later malformed queued payload
  cannot partially drain peer retry state.
- Inbound and remotely imported propagation payloads update active peer record
  snapshots when they queue new unhandled IDs or mark source peers handled,
  keeping restart/export state aligned with live queue fan-out and source
  accounting.
- Duplicate inbound peer propagation payloads now still apply source-aware
  fan-out to active relay peers while keeping the source peer handled, so a
  known local payload does not skip relay queue creation.
- The typed ZeroMQ SDK backend now covers identity list/activate/import/export,
  identity announce, presence list, identity resolve, contact update/list, and
  identity bootstrap, so REM/RCH peer discovery, identity recovery, and
  saved-peer setup can use `ZmqPipelineBackendClient` instead of falling back
  to raw RPC/HTTP identity/contact calls.
- The typed ZeroMQ SDK backend now also exposes
  `ZmqPipelineBackendClient::identity_announce` for capability-rich announces,
  preserving local identity, display name, callsign, REM capability flags, RCH
  announce-slot metadata, and extensions over `sdk_identity_announce_now_v2`
  while keeping the no-argument `identity_announce_now` compatibility path.
- The typed ZeroMQ SDK backend now exposes
  `ZmqPipelineBackendClient::workflow_peer_ready`, preserving display names,
  callsigns, trust, bootstrap intent, and REM/RCH capability metadata while
  optionally announcing before use, so saved-peer setup has a direct typed path.
- The typed ZeroMQ SDK backend now exposes
  `ZmqPipelineBackendClient::peer_directory`, merging saved contacts and
  announce-derived presence over `sdk_identity_contact_list_v2` and
  `sdk_identity_presence_list_v2` while preserving display names, callsigns,
  REM capability flags, RCH announce-slot metadata, online state, and
  first/last-seen timestamps.
- The typed ZeroMQ SDK backend now exposes
  `ZmqPipelineBackendClient::peer_directory_since` and a
  `min_last_seen_ts_ms` presence-list filter, so REM/RCH can suppress stale
  announce rows over the SDK path while keeping saved contacts visible offline.
- The typed ZeroMQ SDK backend now exposes saved-peer lifecycle calls through
  `ZmqPipelineBackendClient::peer_connect`, `peer_disconnect`, and
  `peer_reconnect`, routing `sdk_peer_*_v2` methods while preserving identity,
  display name, correlation ID, callsign, REM capability flags, RCH
  announce-slot metadata, and per-call extensions.
- The typed ZeroMQ SDK backend now also covers the operation registry and SDK
  envelope execution path, including `app.message.history.list` and
  `app.delivery.destination_hash`, so REM/RCH direct-chat history and runtime
  delivery-destination lookups can stay on `ZmqPipelineBackendClient` instead
  of constructing raw RPC/HTTP envelopes.
- The typed ZeroMQ SDK backend now exposes durable direct-chat history through
  `ZmqPipelineBackendClient::list_message_history`, preserving message bodies
  with links, receipt status, basic LXMF fields, one-to-one
  `peer_id`/`conversation_id` filters, `include_receipts`, and restart
  pagination cursors through the daemon `app.message.history.list` SDK envelope
  path.
- The typed ZeroMQ SDK backend now exposes durable direct-chat conversation
  summaries through `ZmqPipelineBackendClient::list_conversations`, preserving
  peer display names, unread counts, last-message previews with links, receipt
  inclusion intent, and restart pagination cursors through
  `app.message.conversation.list` on the SDK envelope path.
- `ZmqPipelineBackendClient::list_message_history` now accepts both canonical
  `id`/`content` records and legacy direct-chat `message_id`/`body` records
  from `app.message.history.list`, keeping restart-recovered conversation
  history readable without raw envelope decoding.
- The typed ZeroMQ SDK backend now exposes the local runtime delivery
  destination through `ZmqPipelineBackendClient::local_delivery_destination_hash`,
  while still routing `app.delivery.destination_hash` through SDK envelope
  execution, so REM/RCH direct-chat source selection does not need raw RPC/HTTP
  status calls.
- The typed ZeroMQ SDK backend now tracks negotiated receipt terminality for
  delivery status, so direct-chat status reports match the SDK contract:
  `sent` is terminal until `sdk.capability.receipt_terminality` is negotiated,
  after which `delivered` is the terminal receipt state.
- The typed ZeroMQ SDK backend now exposes burst sends through
  `ZmqPipelineBackendClient::send_batch` and still routes
  `app.delivery.send_batch` envelope calls to `sdk_send_batch_v2`, preserving
  ordered per-message acceptance and rejection results without raw RPC
  envelopes.
- `BatchSendItem` now carries per-message idempotency keys, TTL, correlation
  IDs, and SDK extensions into each batch message's `_sdk` field metadata, so
  burst direct-chat retries can remain stable across client restarts.
- The typed ZeroMQ SDK backend and operation registry now expose direct-chat
  cancellation through both `ZmqPipelineBackendClient::cancel` and
  `app.delivery.cancel` envelope execution, preserving daemon cancellation
  outcomes without raw RPC envelopes.
- The typed ZeroMQ SDK backend now starts the final propagation-first branch
  with `ZmqPipelineBackendClient::propagation_peer_sync`, routing
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
- The same ZeroMQ SDK propagation branch now exposes remote router status,
  fetch, download, sync, and unpeer lifecycle calls through typed
  `ZmqPipelineBackendClient` methods and registered `app.propagation.*`
  envelopes, preserving daemon propagation, peer-sync, transfer, denial,
  timeout, and queue-cleanup payloads without requiring REM/RCH to use raw RPC.
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
- Remote fetch/download/sync/unpeer SDK envelopes now convert denied, timed
  out, and retryable bridge failures into typed result payloads with daemon
  propagation recovery state, so REM/RCH clients can stay on
  `ZmqPipelineBackendClient` for failure recovery instead of dropping to raw
  RPC errors.
- `PropagationRemoteUnpeerResult` now projects remote unpeer `messages` and
  propagation cleanup payloads into a typed `queue` snapshot, so denial and
  teardown cleanup callers can inspect handled, unhandled, transferred,
  skipped, rejected, and transfer-limited IDs without parsing raw JSON.
- `PropagationRemoteUnpeerResult` now also projects teardown lifecycle payloads
  into typed `transfer_state`, so denied or failed unpeer attempts expose
  failure kind, access-denied/timeout classification, retry scheduling, and
  last error without parsing raw propagation JSON.
- The same branch now exposes propagation sync completion/failure
  acknowledgement as
  `ZmqPipelineBackendClient::propagation_acknowledge_sync_completion` and
  `app.propagation.acknowledge_sync_completion`, preserving daemon recovery
  state for retry, timeout, and restart flows on the typed ZeroMQ SDK path.
- `PropagationStatusResult` and `PropagationAcknowledgeSyncResult` now project
  their propagation payloads into typed `recovery_state`, so status, enable,
  and acknowledgement callers can inspect sync state, retry counts, queue
  depth, and last error without parsing raw JSON.
- `PropagationRecoveryStateResult` now also exposes failure kind, timeout and
  access-denied booleans, and next sync attempt, so local recovery and sync
  acknowledgement callers can branch on denial/timeout handling without raw
  propagation JSON.
- `PropagationRecoveryStateResult` now also exposes the propagation lifecycle
  `timestamp`, so restart/recovery status callers can inspect daemon recovery
  freshness without parsing raw propagation JSON.
- `PropagationRecoveryStateResult` now also exposes local propagation config
  fields for `auth_required`, `static_peers`, and `sync_limit`, so status and
  enable/config callers can verify recovery policy without raw propagation JSON.
- `PropagationRecoveryStateResult` now also exposes propagation storage and
  transfer-limit config for `store_root`, `target_cost`,
  `message_storage_limit_mb`, and `propagation_limit`, keeping durable queue
  policy visible on the typed ZeroMQ SDK path.
- `PropagationRecoveryStateResult` now also exposes the remaining propagation
  enable/status config for `stamp_cost_flexibility`, `delivery_limit`,
  `autopeer`, `autopeer_maxdepth`, `max_peers`, `from_static_only`,
  `retain_synced_on_node`, `peering_cost`, and `remote_peering_cost_max`, so
  router/peering policy is visible through the typed ZeroMQ SDK path.
- The typed propagation branch also exposes outbound propagation router
  selection and listing as `ZmqPipelineBackendClient::propagation_node_get`,
  `propagation_node_set`, and `propagation_node_list`, backed by
  `app.propagation.node.*` envelopes that preserve selected-node and node-list
  metadata without raw RPC.
- `PropagationNodeListResult` now projects listed router candidates into typed
  `PropagationNodeRecord` entries, exposing peer, display name, last-seen time,
  selected flag, and capability strings while retaining the raw node JSON.
- `PropagationNodeSelectionResult` now projects node get/set `meta` into typed
  `selection_state`, exposing selected peer, selection flag, queue depth,
  failure kind, timeout/access-denied classification, retry scheduling, and
  last error without parsing raw router metadata.
- The typed propagation branch now also exposes local propagation status,
  enable/config, delivery policy get/set, and peer maintenance through
  `ZmqPipelineBackendClient` methods and `app.propagation.*` envelopes, keeping
  daemon policy, stale-peer cleanup, and retry/maintenance state visible without
  raw RPC.
- `PropagationPeerMaintenanceResult` now projects maintenance-triggered
  `peer_sync` payloads into typed `peer_sync_state`, so stale-peer cleanup and
  automatic retry/rotation callers can inspect sync timing and queue transient
  IDs without parsing raw JSON.
- The typed propagation branch now exposes local propagation payload ingest and
  fetch as `ZmqPipelineBackendClient::propagation_ingest` and
  `propagation_fetch`, backed by `app.propagation.ingest` and
  `app.propagation.fetch` envelopes that preserve transient IDs, payload bytes,
  duplicate accounting, and durable store recovery through the ZeroMQ SDK path.
- `PropagationIngestResult` and `PropagationFetchResult` now also preserve
  daemon propagation lifecycle payloads and project them into typed
  `recovery_state`, so disconnected-client ingest/fetch callers can inspect
  selected node, sync state, queue depth, and local ingest/serve counters
  without parsing raw propagation JSON.
- `PropagationDeliveryPolicyResult` now projects delivery policy payloads into
  typed `policy_state`, so propagation-first clients can inspect auth-required
  mode plus allowed, denied, ignored, and prioritised destination sets without
  parsing raw policy JSON.
- The typed propagation branch now also exposes
  `ZmqPipelineBackendClient::propagation_recovery_state`, projecting
  `app.propagation.status` into structured sync state, selected-node,
  last-error, retry count, queue depth, timestamp, and local ingest/serve
  counters while retaining the raw propagation payload for queue recovery
  diagnostics.
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

1. **Deferred stamp lifecycle**
   - Add Python-style background work ownership, queueing, retry, cancellation,
     and progress behavior for expensive normal and propagation stamps.
2. **Interop breadth**
   - Propagation router lifecycle now has dispatchable Python-reference cases
     for remote status, Rust-to-Python fetch/download/sync, Python-origin
     `/get` haves acknowledgement, and Python-origin `/offer` side effects.
   - Capture release evidence for Sideband, MeshChatX, and Columba before making
     client-specific compatibility claims.
3. **Reticulum behavioral breadth**
   - Finish channel ordering, resolver/bootstrap, announce/path edge behavior,
     and runtime mutation parity.
4. **Operational breadth**
   - Add prepared-host hardware evidence for BLE/RNode paths.
   - Implement or explicitly defer missing Python interface families and
     utility commands.

## Active Execution Order

1. Finish deferred stamp worker and retry lifecycle.
2. Expand pinned Rust/Python interoperability gates with each completed row.
3. Close RNS channel, discovery, resolver, and transport-policy gaps.
4. Collect hardware, soak, and external-client release evidence.
5. Expand interface and utility breadth after protocol behavior stabilizes.

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
