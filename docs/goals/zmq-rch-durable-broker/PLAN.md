# LXMF-rs / RCH: reviewed Rust implementation plan

Date: 8 October 2026  
Status: proposed implementation plan (reviewed v2); supersedes the original draft  
Publication scope: documentation only. No source, service or database changes.

## 1. Decision

Keep `reticulumd` as the owner of durable LXMF admission and replay. Keep socket/protocol recovery in the LXMF-rs SDK. Give RCH durable intake and duplicate-safe business processing.

The previous plan has the correct reliability goal, but it is not yet an implementation specification. It prescribes expensive work on the older reply transport, leaves cancellation behavior implicit, and assumes a large RCH transaction without defining its actual storage boundary. The replacement narrows the first release and makes those decisions explicit. A single combined RCH transaction is valid when every affected operation genuinely shares it; the defect is assuming this has already been established.

Recommended design:

- Reuse the existing ROUTER/DEALER path as the intended steady-state transport, after qualification. Keep emergency repairs and the endpoint cutover separate from the durable-storage cutover.
- Use the daemon's existing SQLite storage for a transactional message outbox/journal. Poll the committed journal directly; an in-memory notification is only a wake-up hint.
- Give RCH a transactional inbox, followed by an ordered application worker. A newly negotiated `ack_stored` means RCH has accepted durable custody. Application completion is a different state.
- Use Rust ownership, small state enums, typed identifiers, explicit transaction methods and bounded owning workers. Do not add an actor library, a general event bus, a new broker service or a storage abstraction for hypothetical databases.

The last two delivery stages are a deliberate change from the original plan. Do not silently change the meaning of an existing acknowledgement or `Delivered` status.

## 2. Evidence and scope

The original plan reviewed LXMF-rs `04a0ba5e5676a521c87bd7d77064bacf598f08e6` and RCH `96f71a639f041cec48beb553af9af15e2a49e7ce`. This review uses those source baselines, plus a fresh default-branch read of SDK `transport.rs`, whose returned blob SHA was `0d54e1c23004d4bca016243eeadf0538ad7ca88a`, matching the earlier reviewed file. Source references are listed below. This does not identify the deployed binaries. [C1–C5]

Verified additions to the review:

- The SDK leaves a socket in a shared `Option` while awaiting its send/receive exchange. Reset happens after the awaited result is evaluated. An externally dropped future can skip that reset path. This is a static cancellation-safety finding, not a reproduced production root cause. [C1, R3]
- RCH already has a command transaction wrapper that commits database state before publishing marker/zone changes. Extend that ownership and lock-order discipline; do not assume all inbound handlers can participate in it without changes. [C4]
- LXMF-rs declares Rust 1.85, `rusqlite` 0.37.0 with bundled SQLite, and the pure-Rust `zeromq` 0.6.0 crate with Tokio/TCP features. Its workspace lockfile resolves `libsqlite3-sys` 0.35.0. Respect the supported Rust versions and validate the native SQLite build instead of treating libzmq configuration advice as applicable to this crate. [C5, C6]

First release: one daemon, local durable storage, one active RCH process and one ordered consumer for each configured service identity/subscription. A local process/store lock prevents two RCH processes from sharing the same consumer state directory. Separate installations get separate consumer IDs. Shared-consumer clustering, competing consumers, remote failover, arbitrary subscription mutation and replication are excluded.

Keep RNS/LXMF network formats, destination ownership and public RCH application routes unchanged. New delivery distinctions may be exposed additively. Never let RCH open the daemon's database directly.

## 3. Criticism of the original plan

| Finding | Why it matters | Correction |
|---|---|---|
| It makes a per-client PUSH reply cache mandatory while also hardening ROUTER. | That adds endpoint lifecycle, eviction, reconnection and security work to a compatibility path. Socket churn is observed; its responsibility for the production stall is not established. | Make the existing ROUTER/DEALER path the target. Apply only necessary safety repairs to PUSH/PULL. Build persistent compatibility replies only when a measured deployment constraint prevents migration. |
| Its timeout handling does not specify external future cancellation. | An outer timeout, dropped request or task cancellation can bypass cleanup after an `.await`. A dirty socket can remain in its pool. | Make the exchange cancellation-safe with ownership; add cancellation-at-each-stage tests before expanding automatic retries. |
| It places all RCH processing in one transaction without a storage-boundary audit. | Existing handlers can own their own transactions, memory changes and outbound actions. An outer wrapper does not make those operations atomic. Large batches also extend lock time and couple broker progress to business handlers. | Use a short inbox transaction, then a separate transaction for each ordered business application step. Reuse and extend existing transaction methods. |
| It does not distinguish process-crash durability from power-loss durability operationally. | A successful database commit is not a sufficient specification of synchronization settings or the supported storage system. | Define the durability class, verify SQLite settings at startup and qualify the real storage. Use WAL plus `synchronous=FULL` for the proposed power-loss durability target, subject to device/fsync guarantees. |
| Its broker API is a list of operations rather than a complete state contract. | Developers still have to invent initial position, outstanding-batch rules, ACK bounds, stale backup behavior and quarantine meanings. | Use one outstanding batch per consumer initially, explicit receipt/cursor rules and distinct stored/applied progress. |
| It warns about starvation and `spawn_blocking`, but does not allocate concrete owners or define overload outcomes. | A semaphore inside spawned tasks does not bound waiting tasks; moving disk work to another pool does not guarantee bounded admission or shutdown. | Bound admission before spawning and decoding large payloads; retain task handles; use a scheduled storage owner and typed Busy/Unknown outcomes. |
| It places broad qualification before a small complete durable flow is specified. | A large task list can pass isolated socket tests while the actual message-to-RCH commit path remains incomplete. | Deliver one complete `Test1234` path, then add failure cases and remaining event producers. A 1,000-cycle soak is optional evidence, not a universal release requirement. |

Several parts of the original plan should remain: atomic message/event persistence, stable command IDs, separate event/message deduplication, explicit expired-history reporting, independent consumer progress, no silent fallback to best-effort mode and protection of unreplicated-disk-loss caveats. Those are not newly discovered omissions. [P1]

## 4. Design patterns and their concrete use

### Transactional outbox, without a second messaging service

One daemon database transaction writes the authoritative message/change, the replay event, and any required durable dispatch intent. Polling reads the committed event table. There is no correctness dependency on copying the event into another in-memory queue. This applies the transactional outbox pattern at the existing database boundary. [R6]

The delivery journal is not a mandate to convert all daemon state to event sourcing. Existing message tables remain authoritative; the journal serves integration delivery and replay.

### Transactional inbox and idempotent receiver

RCH first stores incoming records, their deduplication identities and its received checkpoint atomically. An ordered worker subsequently applies each record, committing business state, completion marker and outbound intents together. The database unique key is the arbiter of replay duplicates. [R7]

This costs a local inbox and a second progress measure. That cost is justified by separating communication from slow or failing handlers. Do not add another queue service: use the existing RCH database, and give pending inbox rows finite storage limits.

### Unit of work

Expose transaction-sized operations such as `append_message_event`, `store_inbound_batch` and `apply_inbox_event`, not an unrestricted generic repository interface. Helpers operating inside a transaction receive the same transaction handle; they do not open another connection or commit independently.

In Rust, retain default rollback-on-drop and call `commit` explicitly. A completed transaction may return a post-commit update to publish; it must not return a success receipt before commit. [R8]

### Owned workers and state machines

Use an owner for mutable socket state and an owner for each scheduled storage-write lane. Communicate through bounded channels when there is a real asynchronous boundary. Reuse existing owning actors where practical instead of layering another supervisor over them. Tokio documents the task-plus-channel approach for shared I/O resources. [R1]

Represent connection recovery with an enum and ordinary transition functions. A large generic typestate API or an actor dependency is unnecessary for this change.

### Bulkheads and backpressure

Separate limited admission for broker poll/ACK/control traffic from bulk outbound work and maintenance. Also limit database work; separate network queues alone cannot prevent storage contention. Rate limits and queue-full responses are part of the API rather than invitations to allocate more memory.

### SOLID in Rust

Separate transport, delivery protocol and database commit responsibilities. Use dependency inversion only at existing transport/storage boundaries; expose narrow traits where a substitute or test seam actually exists. Use composition and typed errors. An in-memory fake must not claim the same crash guarantees as SQLite: run durable-store contract tests against the real engine.

Do not add inheritance-style service trees, a trait for every struct, one crate per pattern, or a generic message-broker API. Refactor only boundaries that obstruct the required change. Preserve Python/Rust wire conformance.

## 5. Exact delivery and acknowledgement meanings

Use separate terms in new SDK responses and diagnostics:

| State | Meaning |
|---|---|
| `DaemonStored` | The message, required event and dispatch intent have committed under the declared daemon durability policy. |
| `RchStored` | RCH's inbox and received checkpoint have committed. RCH now owns local replay until processing or durable rejection. |
| `RchApplied` | Local business changes, the applied marker and any outbound intents have committed. |
| `RchRejected` | A supported event was rejected under an explicit policy; its original bytes and reason are durably retained. It was not successfully applied. |
| Peer network receipt | The protocol's existing receipt meaning. It is not automatically evidence of either RCH state. |

The new broker ACK is `ack_stored`. It advances daemon retention eligibility only through records safely stored by RCH. It does not announce business completion. Preserve any older processing-ACK contract; enable the new meaning only through a new negotiated capability.

For a consumer, distinguish broker-acknowledged stored position `B`, local stored position `S`, and local handled position `A`:

- Normal state: `B <= S` and `A <= S`.
- `B < S` after restart is normally a lost or unconfirmed ACK: retry the persisted receipt.
- `B > S` indicates inconsistent/restored/lost consumer state. Do not silently adopt `B`; enter recovery-required state.
- A handled position can include explicit rejections, so retain counts for applied and rejected records separately.

The contract promises retryable handoff and duplicate-safe committed local effects. External side effects remain at-least-once unless their recipient also honors a stable idempotency key.

## 6. Transport and SDK repair

### 6.1 Cancellation-safe exchange

Immediately repair the existing SDK transport implementation, independently of endpoint migration.

With the existing pool, take a socket out of its slot before the first exchange await, retaining exclusive slot ownership. Return it to the slot only after receiving and validating the matching response. On an error, timeout or external cancellation, drop the locally owned transport and leave the slot empty. Socket-library task cleanup must be tested; Rust ownership alone does not prove an undocumented library shutdown behavior.

This is smaller than rewriting the SDK into actors. An owning I/O task is an alternative only where actual concurrent multiplexing requires it. Do not implement both approaches for the same connection.

A dropped RPC future never proves that a mutation did not execute. Socket cleanup and durable operation reconciliation solve different problems. [C1, R3]

### 6.2 ROUTER/DEALER decision

Qualify the existing endpoint with RCH's actual authentication, service identity, negotiation and event operations. Use persistent client connections. On the server, route responses over the originating connection rather than dialing a client-supplied reply address. Treat the ZeroMQ route as a routing key, not as an authenticated principal.

Bound admission before task creation. Keep request tasks in the existing `JoinSet` or equivalent owned collection; limit queued responses by count and bytes. A disconnected peer must not monopolize reply capacity. Do not block the ingress loop waiting for bulk-work permits in a way that prevents it from admitting reserved control traffic.

An endpoint cutover is allowed only after the same RCH contract tests pass. Keep the previous endpoint mode during an emergency patch. Do not combine endpoint migration with database migration. If migration is blocked, document why; then add only the necessary bounded persistent reply ownership to the compatibility path. [C2, C3]

The project uses the Rust `zeromq` crate, not libzmq bindings. Confirm the pinned crate's queue, disconnect, cancellation and authentication behavior; do not assume libzmq socket-option names or guarantees apply. [C5, R10]

### 6.3 One retry authority

The SDK owns finite transport retries within one logical operation. RCH's worker owns scheduling a later logical attempt after the SDK returns. The daemon does not create a parallel retry queue for ephemeral RPC responses. Its durable journal is the retry source for events.

Use a total logical-operation deadline plus shorter attempt deadlines. A first attempt that consumes the entire total budget leaves no opportunity for retry. Backoff must fit the remaining budget, and queued work must expire before submission when its budget is gone. Do not reset budgets at every recovery stage.

Classify failures using typed fields: failure stage, execution certainty, retry directive and operation ID where applicable. Separate `NotSubmitted`, `Rejected`, `Accepted` and `Unknown`. A response timeout after submission is normally `Unknown` for a mutation. An explicit validation failure is not a broken connection.

Replay-safe reads and durable-batch fetches may be retried. Mutations may be retried only under a documented idempotency contract. An expired token may get one supported credential refresh; permission denial or invalid credentials must not cause endless renegotiation. Store cursors and identity handles independently of socket state. [R9]

### 6.4 Recovery state

Use states equivalent to `Disconnected`, `Connecting`, `Negotiating`, `EnsuringIdentity`, `Ready`, `Backoff`, `RecoveryRequired` and `Stopping`. These are proposed names, not new existing API claims.

The SDK reports protocol/session validity and repairs sockets. The existing RCH session owner decides when its service identity needs restoration. Only this owner starts application-session recovery. A transient poll failure does not recreate the SDK, import the private key or reset the durable consumer.

## 7. Daemon storage and replay contract

### 7.1 Durable journal

Extend existing storage with the minimum missing records:

| Record | Essential fields or behavior |
|---|---|
| Journal metadata | Persistent journal ID, durable next position, retained prefix boundary and schema version. |
| Replay event | Position, event kind/version, visibility scope and immutable payload or retained payload reference. |
| Consumer state | Stable consumer ID, authenticated owner, fixed subscription, acknowledged position and one outstanding batch descriptor. |
| Operation receipt | Stable operation ID, request fingerprint, accepted message ID and durable outcome/dispatch state. Reuse current records where equivalent. |

An event identity can be the pair `(JournalId, Position)`. Another unrelated event UUID is unnecessary unless an existing contract already requires it. Request correlation ID, business operation ID and LXMF message ID remain distinct.

Update the journal counter in the same transaction as the event. Positions must not be reused after pruning or after the journal becomes empty. Order the result by committed position, never wall-clock time. Do not expose an event whose payload transaction has not committed.

Keep diagnostic/lifecycle traces outside this durable message stream. Audit every required event producer: direct inbound, propagated inbound, outbound admission and terminal status changes. One producer still using an independent message write plus best-effort event notification defeats the guarantee.

For network ingress, identify the actual receipt-emission boundary. The first release guarantees records admitted durably by the daemon; any stronger mesh receipt guarantee requires separate protocol-conformance evidence.

### 7.2 Minimal consumer protocol

Extend the existing versioned SDK protocol with a negotiated durable-handoff capability. The operation names here describe behavior, not pre-existing RPC names.

`resume_consumer` checks the fixed identity/subscription and reports journal identity, retained boundary, broker ACK and an outstanding batch when present. A new consumer explicitly starts at the earliest retained data or performs a bootstrap; starting at the tail requires an explicit new-consumer choice.

`fetch_batch` returns a bounded ordered batch and receipt. Start with one outstanding batch per consumer. Persist its covered range before returning it; retries resolve to the same pending range until acknowledged. This small record also permits acknowledgement validation after daemon restart. Do not allocate a per-event delivery row for every poll attempt.

`ack_stored` validates the authenticated consumer, journal and issued batch range. Duplicate ACKs are successful no-ops. Reject acknowledgements outside the issued range. After restart RCH reuses its durable receipt; socket identity and request correlation IDs do not determine consumer progress.

`recovery_status` can be part of resume/status rather than requiring a separate API. It reports expired data and supported recovery choices. Do not change global overflow or retention policy during a consumer negotiation.

If the stream has visibility filtering, receipt ranges cover the scanned prefix, not just the last visible row. A consumer stores all returned visible events before accepting the covered cursor. Fixed subscriptions and current authorization must be validated before returning or acknowledging data. The broker cannot prove that an authorized RCH application really committed its database; the protocol relies on that client's correctness, backed by tests.

Cap the full encoded response, not only the payload fields. A legal maximum event must fit in a legal single-event response. Preserve stable event data on replay; response time metadata is not event creation time.

### 7.3 SQLite policy

Use the existing SQLite engine and transaction discipline in both repositories. For the proposed power-loss durability class, verify local-disk WAL mode and `synchronous=FULL` on relevant connections; performance tests must use the same settings. This is not a guarantee against lying storage hardware, filesystem failure or loss of the entire disk. `NORMAL` mode is an explicitly weaker alternative, not an equivalent durability configuration. [R4, R5]

Schedule short writes under one owner for the message/journal boundary. Reuse the current storage worker or add one bounded dedicated writer thread for a persistent loop; use bounded `spawn_blocking` jobs only for short work, not an indefinite worker loop. Use short independent read transactions where useful; do not hold a read snapshot open while waiting for RCH. SQLite still permits only one concurrent writer, and long reads can obstruct WAL checkpoints. [R4]

Set database busy limits deliberately against request budgets. On contention, return bounded `StorageBusy` rather than claiming that an outer Tokio timeout cancelled a database write. Once blocking work starts, retain and observe its completion. [R2, R8]

Qualify the native SQLite version, not just the crate name. SQLite's official WAL documentation reports the WAL-reset fix in 3.51.3 and later, with backports including 3.44.6 and 3.50.7. Require a fixed native build or documented equivalent patch; inspect `sqlite_version()`/build metadata before deployment. The reviewed Cargo declarations do not by themselves prove the production binary's native version or that this bug caused the reported fault. [C5, C6, R4, R15]

## 8. RCH intake and application

### 8.1 Inbox commit

After basic envelope/authentication validation, store the returned event bytes, scoped event identity, batch receipt and local received checkpoint in one short RCH transaction. Deduplicate using a unique database key. Verify matching immutable content when a duplicate key appears; a conflicting payload is a protocol/storage error, not a harmless duplicate.

Only after this commit may RCH send `ack_stored`. If RCH disk admission fails, do not acknowledge; pause fetches and report local backpressure. Pending inbox events remain recoverable without contacting the daemon.

Do not require business decoding before accepting custody of a supported opaque envelope. A later malformed business payload is retained in the inbox for durable disposition rather than forcing repeated network recovery.

### 8.2 Apply commit

An ordered worker loads the next stored record. Stage its changes using the existing RCH command transaction path. Commit business state, handled-event identity, any durable outbound intent and the handled position together.

The `command_persistence::mutate` pattern already commits before publishing marker/zone deltas. Extend its unit of work to include the inbox completion marker and outgoing intent; do not call a self-committing handler and then mark the inbox row handled in a later transaction. Preserve its documented lock order or replace it only with an equivalent tested ownership rule. [C4]

No network call or browser notification occurs inside the database transaction. Memory projections are updated only from committed state, preserving commit order. A projection update failure after commit triggers rebuild/reload; it must not re-execute the business command as compensation.

Different events about one message are not duplicates merely because they share a message ID. Replayed event IDs suppress repeated event application; stable command/message IDs suppress repeated creation or logical commands. Keep these keys separate.

### 8.3 Rejection and unsupported versions

A malformed supported record can be durably rejected, with original bytes, reason and operator-visible status. Do not call this `Applied`.

An unknown required event version is not necessarily corrupt data. Pause the application lane with `UpgradeRequired` or an equivalent explicit state. The inbox can continue only while its storage bound allows. Never automatically quarantine and discard mandatory new event kinds to make a progress counter advance.

Retryable business failures preserve order initially. Parallel per-topic workers, poison-message bypass and competing consumers are not part of the first release.

## 9. Outbound retry safety

RCH persists a logical operation ID before first submission, with immutable business fields and original timing/expiry information. Each network attempt gets its own request correlation/authentication data, but reuses the same logical operation ID.

The daemon transaction stores the operation receipt together with accepted message data and durable dispatch intent. On an identical retry it returns the existing admission result/message handle; on a different payload under the same scoped key it returns a conflict. Fingerprint the fixed business request, excluding authentication and transport attempt fields. Do not hash unstable map ordering or regenerated timestamps. [R9]

Recovery must resume dispatch intents that were committed before a daemon crash. Recording an accepted receipt alone does not ensure accepted work will run. A crash after physical transmission can still lead to another transmission, so retain the same message identity and do not claim exactly-once over the radio/network.

Define a maximum retry/reconciliation age. Keep deduplication receipts for at least that period plus the supported recovery margin. Old operations after receipt expiry must be rejected or explicitly reconciled rather than admitted silently as new work; persist compact tombstones or enforce a validated immutable request-age window. Choose and test one rule before receipt garbage collection is enabled.

Do not enable automatic retries for other mutations until they have an equivalent contract.

## 10. Capacity, retention, recovery and shutdown

Bound pending requests, message bytes, reply bytes, database operations, journal storage and RCH inbox storage. Count the database, WAL, indexes, retained payloads and quarantined data. Reserve headroom before physical disk exhaustion; successful cleanup must not depend on adding a new record to an already-full filesystem.

Default policy: preserve unacknowledged admitted records and backpressure new admission at the configured bound. This means a consumer offline for too long can stop new admissions. Finite disk, unlimited outage duration and unlimited incoming accepted traffic cannot all be guaranteed.

Automatic unacknowledged expiry is disabled initially. Add it only with a stated retention promise, durable prefix/gap metadata and an explicit expired-history response. Prune eligible prefixes in small transactions; update the lower bound atomically. Keep message payloads while any required replay/inbox reference depends on them. Polling must never be treated as acknowledgement.

RCH owns retention for pending inbox records after `RchStored`. It cannot purge an unapplied/unrejected event merely because the daemon accepted its ACK.

For recovery beyond retained data, record an explicit recovery session and continuation. Use a stable upper boundary or a bounded admission pause to reconcile history with live events; a timestamp alone is not a safe cutover. History can restore only retained current records, not every lost status transition. A missing historical range remains reported unless the recovery operation actually reconstructs it.

A restored daemon backup must receive a new journal incarnation before accepting consumers. Do not rotate the RNS identity merely to rotate the journal. A restored RCH backup that is behind broker ACK enters recovery-required state. Detecting every undeclared copy/restore is not promised; backup/restore tooling must enforce these procedures.

Shutdown closes new admission first, signals owners, waits for admitted writes to finish or roll back, then joins owned tasks and closes sockets. `Drop` releases local resources but is not an asynchronous shutdown protocol. A started `spawn_blocking` call cannot be aborted like an async task. A stuck filesystem can prevent a fully graceful deadline; a supervisor may force termination with an explicit unclean outcome, followed by database recovery. Never report that all work completed merely because a wait timed out. [R2, R11]

Keep liveness separate from durable readiness. A process may be alive while storage admission or consumer application is unavailable. Health responses must expose that distinction; do not restart the daemon repeatedly just because RCH is temporarily unavailable.

## 11. Rust implementation rules

Use concrete existing modules and crates. Suggested logical responsibilities are `transport_exchange`, `recovery_policy`, `durable_events` and `inbox_application`; these names are not instructions to create new crates.

Identifiers should have distinct newtypes, for example `RequestId`, `OperationId`, `JournalId`, `EventPosition`, `ConsumerId`, `StoredCursor` and `HandledCursor`. Use private constructors for commit receipts where practical. This prevents accidental interchange, but the compiler cannot prove that another process persisted data. [R12]

Use a typed internal error enum with source errors retained. Convert to the existing external SDK error representation at the boundary. RCH must not decide whether to reset identity by substring matching error text.

Keep short in-memory critical sections under ordinary mutexes when no await occurs. Use an async mutex only for the existing serialized async ownership need, or channels for an actual owning task. Do not replace every mutex with an actor.

A bounded channel is not a whole-system memory bound when messages are large, the socket library has its own queues, or callers can allocate before queue admission. Validate frame lengths early and account for bytes at every retained-work boundary.

Reuse the existing Tokio runtime. The async SDK path should not create a runtime per operation; synchronous compatibility calls must use a defined bridge rather than blocking an async runtime thread.

Keep the current Rust minimum versions, feature combinations, crate dependency rules and conformance tests. No new `unsafe` is needed. Use focused linting, tests and small responsibility-led module changes; do not perform cosmetic repository-wide restructuring.

## 12. Six work packages

| Package | Owners | Deliverable | Completion evidence |
|---|---|---|---|
| W0: contract and reproduction | Both, led by LXMF-rs | Exact paired binary/source record; selected endpoint; failure trace; durability/ACK meanings; producer and RCH transaction map. | Existing regressions reproduced or bounded non-reproduction documented. No deployed-state assumptions. |
| W1: recovery and transport | Daemon + SDK; RCH session owner | Cancellation-safe exchange, typed failure decisions, bounded retry policy and owned server tasks. Qualify ROUTER separately. | Cancellation, lost reply, stale reply and restart tests; no blanket identity reimport; healthy client isolation. |
| W2: durable inbound slice | Daemon + SDK + RCH core/server | One real message producer writes message/journal atomically; one consumer stores an inbox batch and issues `ack_stored`. Storage bounds are already enforced. | `Test1234` survives both process restarts and ACK loss; one inbox record remains. Do not yet advertise all-event durable coverage. |
| W3: apply and outbound safety | RCH transaction/worker paths + daemon admission + SDK | Inbox application shares a unit of work with deduplication and outbound intents. Stable admission receipts resume dispatch. | Crash after custody ACK but before business processing; crash during application; lost outbound admission reply; no duplicate committed business effect. |
| W4: complete and qualify | Both | All required producers use the transaction boundary; retention/recovery, protocol validation and workload isolation verified. | Disk-full, stale backup, invalid cursor, malformed/versioned event, competing workload and legacy-compatibility tests. |
| W5: controlled cutover | Both | Tested schema upgrade, bootstrap boundary, capability checks, rollback/drain procedure and paired release manifest. | Existing-state migration with live arrivals has no skipped/duplicated logical import; rollback procedure is demonstrated; supported topology is exercised. |

W0/W1 provide the immediate defect repair. W2 is the first durable handoff demonstration. W3 is required before claiming duplicate-safe business processing. W4/W5 are required for release. Endpoint migration and storage migration remain separate changes even when both are included in this program.

Do not defer essential admission bounds, authentication or durable storage configuration until W4; W4 verifies their complete coverage.

## 13. Acceptance tests

Use deterministic failpoints and real subprocess termination for crash claims. `Test1234` is human-readable content; assertions use its stable LXMF/operation identity and recorded event identity.

| Fault or boundary | Required observation |
|---|---|
| Cancel SDK future at connect, send and correlated receive | No dirty socket returns to its pool; next operation is correctly correlated; uncertain mutation remains reconcilable. |
| Late response arrives after another request begins | It cannot complete the wrong request or advance a cursor. |
| Daemon exits after message/event commit, before reply | Restart exposes the committed event and the original admission result. |
| RCH exits before inbox commit | Broker ACK does not advance; the batch replays. |
| RCH exits after inbox commit, before ACK | Replay deduplicates to the same inbox records; persisted receipt can be acknowledged. |
| RCH exits after ACK but before apply | Pending local inbox work resumes without fetching a missing broker event. |
| Apply fails or exits before commit | Business state, handled marker and outbox changes roll back together. |
| Apply commits but projection publication fails | Reload committed state; do not run the command twice. |
| Admission response lost; daemon exits before dispatch | Stable operation resolves to the original message and dispatch resumes. |
| Reuse operation ID with different content; retry after supported age | Conflict/expired-operation is explicit; no silent new admission. |
| More than 1,024 events; prune all eligible events; restart | No legacy-window freeze or position reuse; later events remain discoverable. |
| Oversized envelope, unauthorized consumer or invalid ACK range | Rejected within bounded resources; no other consumer's data/progress changes. |
| Slow reader, SQLite contention, maintenance and UI/outbound burst | Queues/task counts remain within limits; healthy operations either succeed or return bounded typed overload errors. |
| Daemon/RCH storage full, expired history or restored backup | No false durable success or silent jump to the newest cursor. |
| Unknown required event version | Durable record retained; explicit upgrade-required application state. |
| Shutdown during a blocking write | Completion is joined or an unclean shutdown is reported; no claim that Tokio timeout cancelled committed work. |
| Actual power interruption in a disposable test setup | Validate the declared synchronization policy separately from process-kill tests. |

Use unit/property tests for transition rules, idempotency fingerprints and cursor bounds. Use Tokio's controlled clock for timer-policy tests, not as a substitute for real socket/deadline tests. [R16] Apply Loom only to small concurrency logic that it can actually model; it does not validate SQLite or the whole ZeroMQ stack. Keep real SQLite, paired daemon/SDK/RCH and Python interoperability tests as the main evidence for behavior.

Measure useful outcomes: admission-to-inbox latency, inbox-to-applied latency, broker and local backlogs, retry amplification, identity restoration count, queue bytes, native database/WAL size, open sockets/tasks and storage-busy duration. Separate process RSS from cgroup memory. Report workload/hardware and the observed percentile/error bound; do not invent universal timeout or throughput requirements.

## 14. Migration and release

Before enabling durable mode, identify which existing messages and statuses can be bootstrapped. Historical message rows do not reconstruct all historical events. Mark the bootstrap boundary explicitly.

Prefer a brief controlled admission pause for the first storage cutover: complete current writes, migrate, seed the journal/consumer boundary, switch the authoritative producer path and resume. A zero-downtime migration is not required for this first release. Test arrivals immediately before, during and after the pause and the network's documented retry behavior.

Keep only one authoritative ingestion path for each post-cutover event. Do not run legacy history import and the new journal consumer as competing independent authorities. Deduplication protects overlap but does not justify an undefined cutover.

Readiness must fail explicitly when RCH requires durable mode and the daemon lacks the negotiated capability. The release manifest records daemon/SDK/RCH revisions, native SQLite version, schema version and durability configuration. Do not silently report durable service after falling back to the old polling path.

Back up using a supported consistent SQLite method; do not copy a live database while ignoring its WAL. Reject unsupported schema writers through packaging/startup controls. Old binaries may not understand a new minimum-writer marker, so a database flag alone is not a complete downgrade barrier. Rollback requires a tested compatible binary or controlled drain and reconciliation; a stale backup restore is not lossless rollback. [R4, R14]

## 15. Confidence and limits

Static defect/ownership assessment: 0.94. Proposed Rust design: 0.93. Integration feasibility without the remaining producer/transaction audit: 0.86. Weighted overall assessment: 0.91, rounded, using weights 0.30 / 0.40 / 0.30 respectively. These are engineering confidence estimates, not measured delivery probabilities.

No runtime reproducer, test suite, native SQLite inspection or deployment was executed during this review. The source supports a cancellation gap and identifies reusable transaction/transport code; it does not establish the original production stall's root cause. The acknowledgement split is an intentional protocol change that requires version/capability negotiation. Unreplicated disk loss, undeclared database restoration, expired retained data and external recipients without idempotency remain outside the claimed local guarantees.

## Sources

[P1] Superseded original draft supplied in the planning discussion: `LXMF-rs_RCH_ZeroMQ_Broker_Plan.md`, 8 October 2026, especially P0-B and P1-C through P2-H. It is not a maintained repository document; this reviewed v2 plan is its replacement.

[C1] LXMF-rs SDK transport, reviewed baseline: `https://github.com/FreeTAKTeam/LXMF-rs/blob/04a0ba5e5676a521c87bd7d77064bacf598f08e6/crates/libs/lxmf-sdk/src/backend/zmq_pipeline/transport.rs`

[C2] Daemon ROUTER: `https://github.com/FreeTAKTeam/LXMF-rs/blob/04a0ba5e5676a521c87bd7d77064bacf598f08e6/crates/apps/reticulumd/src/bin/reticulumd/zmq_rpc_loop/router.rs`

[C3] Pipeline response writer: `https://github.com/FreeTAKTeam/LXMF-rs/blob/04a0ba5e5676a521c87bd7d77064bacf598f08e6/crates/apps/reticulumd/src/bin/reticulumd/zmq_rpc_loop/response_writer.rs`

[C4] RCH command persistence: `https://github.com/FreeTAKTeam/Reticulum-Community-Hub/blob/96f71a639f041cec48beb553af9af15e2a49e7ce/crates/r3akt-rch-server/src/command_persistence.rs`

[C5] LXMF-rs workspace manifest: `https://github.com/FreeTAKTeam/LXMF-rs/blob/04a0ba5e5676a521c87bd7d77064bacf598f08e6/Cargo.toml`

[C6] LXMF-rs workspace lockfile: `https://github.com/FreeTAKTeam/LXMF-rs/blob/04a0ba5e5676a521c87bd7d77064bacf598f08e6/Cargo.lock`

[R1] Tokio, channels and owning I/O tasks: `https://tokio.rs/tokio/tutorial/channels`

[R2] Tokio, `spawn_blocking` limits and cancellation: `https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html`

[R3] Tokio, `select!` and cancellation safety: `https://docs.rs/tokio/latest/tokio/macro.select.html`

[R4] SQLite, WAL concurrency, durability, backups and the WAL-reset fix: `https://www.sqlite.org/wal.html`

[R5] SQLite, synchronous pragma: `https://www.sqlite.org/pragma.html#pragma_synchronous`

[R6] AWS Prescriptive Guidance, transactional outbox: `https://docs.aws.amazon.com/prescriptive-guidance/latest/cloud-design-patterns/transactional-outbox.html`

[R7] Hohpe/Woolf, Enterprise Integration Patterns, Idempotent Receiver: `https://www.enterpriseintegrationpatterns.com/patterns/messaging/IdempotentReceiver.html`

[R8] rusqlite 0.37.0, transaction API and busy timeout: `https://docs.rs/rusqlite/0.37.0/rusqlite/struct.Transaction.html`

[R9] AWS Builders' Library, making retries safe with idempotent APIs: `https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/`

[R10] Rust `zeromq` crate API: `https://docs.rs/zeromq/0.6.0/zeromq/`

[R11] Tokio, graceful shutdown: `https://tokio.rs/tokio/topics/shutdown`

[R12] Rust API Guidelines, type safety and newtypes: `https://rust-lang.github.io/api-guidelines/type-safety.html`

[R13] ZeroMQ Guide, reliable request/reply patterns and disk-based queuing: `https://zguide.zeromq.org/docs/chapter4/`

[R14] SQLite, online backup API: `https://www.sqlite.org/backup.html`

[R15] SQLite 3.51.3 release notes, WAL-reset fix: `https://www.sqlite.org/releaselog/3_51_3.html`

[R16] Tokio, timer-policy unit testing: `https://tokio.rs/tokio/topics/testing`
