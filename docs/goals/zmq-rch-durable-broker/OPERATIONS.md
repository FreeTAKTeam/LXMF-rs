# Durable ZeroMQ / RCH operations

This implementation enables the paired path described in [PLAN.md](PLAN.md).
The daemon and SDK expose an additive, explicitly negotiated capability.
RCH requires that capability and has no legacy message-history/polling fallback.
Existing non-ZeroMQ clients retain their public SDK and network receipt contracts.

## Deployment and custody boundary

Use one daemon, local reliable disk, one active RCH process and one ordered
consumer per registered service identity. RCH never opens the daemon database.
Keep the service identity, consumer ID and authenticated principal stable across
restarts. A socket route or a client-selected session string is not a principal.
Remote access requires the existing token configuration and access controls;
loopback fixtures do not qualify remote deployment security or physical storage.

The daemon commits messages, journal events and dispatch admission under
WAL/FULL (`DaemonStored`). RCH separately commits its inbox and stored position
(`RchStored`) before issuing `ack_stored`. Its application worker subsequently
commits business state, completion and outbound intents together (`RchApplied`
or `RchRejected`). Neither custody acknowledgement means peer `Delivered`.

The bundled native engine is SQLite 3.53.2 (rusqlite 0.40.2,
libsqlite3-sys 0.38.2). Durable startup rejects in-memory storage, unsupported
writer versions and native engines before 3.51.3. WAL/FULL is checked on durable
connections; these settings still depend on filesystem/device synchronization
and do not replicate a lost disk. Process SIGKILL tests are not power-cut tests.

## Controlled cutover

1. Stop RCH intake and new operator sends. Drain the old RCH retry queue using
   the old deployment, or record its unresolved operations before retiring it.
   The new RCH does not import an old best-effort cursor or retry ownership.
2. Stop the daemon and take consistent backups of both databases, identities,
   configuration and the RCH state directory. Copying a live SQLite main file
   alone is insufficient: use a SQLite-consistent backup or stop all writers
   and preserve the complete database/WAL state. Keep these pre-cutover backups.
3. Build and deploy the paired revisions recorded in the RCH dependency pin and
   [PROGRESS.md](PROGRESS.md). Record the actual binary SHA256 hashes, source
   revisions, native SQLite version, endpoint mode, deadlines and service flags
   in the deployment record; the repository does not invent production values.
4. Start the daemon with `--zmq-durable-broker`. Enabling it migrates and backfills
   existing messages before admitting network/ZeroMQ work. Existing inbound
   rows appear as `bootstrap_message`; RCH stores their history without executing
   historical commands or issuing new relays. Arrivals serialized after migration
   are live events. Identical logical inbound messages do not gain another event.
5. Start paired RCH with its persisted local database and ZeroMQ endpoints.
   It acquires a process/store lease, negotiates `sdk.capability.durable_broker_v1`,
   imports/activates its persisted service identity, and resumes its stored
   checkpoint. Another RCH process on that state directory must fail startup.
6. Check `/Status` and `/diagnostics/runtime`, then demonstrate a uniquely
   identified `Test1234` handoff and response. Check stored/handled positions,
   pending and rejected counts, lane errors, operation/message IDs and actual
   delivery receipts. HTTP readiness alone is insufficient.

Do not combine this storage cutover with an endpoint migration. The same broker
operations work on qualified ROUTER/DEALER and dual-endpoint PUSH/PULL transports.
For local dual endpoints, an example daemon command is:

```sh
reticulumd --db /srv/reticulum/reticulum.db --identity /srv/reticulum/identity \
  --zmq-durable-broker --zmq-rpc-command tcp://127.0.0.1:9100
```

Follow RCH's README for its complete launch/configuration, including
`--lxmf-zmq-command`, `--lxmf-zmq-response` and identity ownership. A managed
RCH daemon receives `--zmq-durable-broker` automatically. The standalone daemon
must be built with `zmq-pipeline-rpc`. RCH's adapter sets a 30-second logical SDK
request budget; the standalone SDK default is five seconds. Replay-safe broker
reads/fetch/ACK/reconcile get at most two transport attempts inside that original
budget. Ordinary mutations have one attempt; an uncertain durable admission is
reconciled using its persisted operation ID before resubmission. RCH schedules
later logical attempts with 0.5-second exponential backoff capped at 30 seconds.

## Finite capacity and retention

| Boundary | Initial limit and behavior |
| --- | --- |
| Daemon database/journal budget | 512 MiB default; persisted when enabled, not silently resized on reopen. Includes native database/WAL growth admission and reserved completion/control capacity. Native database pages use at most one quarter of that budget (128 MiB by default), leaving bounded WAL-spill/checkpoint and control headroom. Cutover refuses larger existing databases; choose a sufficient initial `--zmq-broker-budget-bytes` before enabling. |
| Journal batch | At most 128 events / 12 MiB encoded response, including envelope headroom. One persisted issued range per authenticated consumer. |
| Daemon ZeroMQ work | Reserved control lane: 8 requests / 96 MiB; bulk: 24 / 160 MiB. Count and retained bytes are acquired before decoded work/task creation and remain owned through reply delivery. Small bounded rejection capacity is separate. |
| Native ZeroMQ receive | Maximum frame 16 MiB plus 64 KiB framing headroom; multipart count 32, total retained receive bytes 128 MiB. Bounded active connections and timed handshakes. These are transport limits, not a total daemon RSS cap. |
| Daemon writer | At most 64 pending writes / 64 MiB owned command bytes. Storage work is serialized; started blocking work remains owned through completion. |
| Dispatch scheduler | At most 64 retained sends / 128 MiB, 32 active globally, one active per peer. Prepared signed wire bytes and propagation selection survive restart. |
| Terminal receipt owner | 1,024 channel slots; a durable operation reserves its terminal slot before physical transmission and keeps it across fallback/retry until a committed terminal result. Persistence retries hold the event; fatal owner failure stops daemon admission and returns an unclean shutdown. |
| RCH inbox/outbox | 256 MiB / 128 MiB logical admission; native database page limit 512 MiB and database/WAL growth guard 1 GiB. Supported oversize attachments above 8 MiB are durably rejected. |
| RCH presentation | Latest 500 messages / 32 MiB, 200 system events, latest telemetry per peer. Pending business/outbound ownership lives in SQLite. |
| Announce projection | Latest 32 bounded records every 30 seconds. This informational snapshot can miss larger announce bursts; it is not the durable message stream. |

There is no automatic expiry of unacknowledged custody. With no consumer,
admitted events remain retained. Prefix pruning requires every registered
consumer to acknowledge storage; polling alone never prunes. Applied inbox rows
release their large payload; rejected inputs retain original bytes and reason.
Operation receipts, immutable input claims and compact handled-event identities
are retained rather than re-admitting an old operation after expiry. These
records consume the finite budget over time. Reclaim/archive requires a separate
reviewed lifecycle; do not delete them or silently reset consumer positions.

The daemon's single writer checks physical admission before mutations, including
shared announce/ticket/metadata writers. Its commit hook reserves bounded native
WAL spills and checkpoint growth; opaque multi-autocommit jobs reserve another
transaction, while broker/status jobs use an explicitly scoped single transaction.
A pinned reader causes observable backpressure, and releasing it allows checkpoint
recovery without restarting. This bound applies to the guarded daemon writer:
external writable SQLite connections or older writers must not share the file.
No cache-spill disabling or database-sized RAM workaround is used.

A long consumer outage or full store intentionally backpressures new admission.
An offline consumer can therefore stop new accepted traffic. `StorageBusy`,
`StorageBackpressure`, `SDK_STORAGE_BROKER_FULL` and native disk-full failures
are observable errors, not successful sends. Free disk, reader contention and
queue depths must be monitored alongside RAM/swap. This work does not establish
that the earlier production swap growth was caused by ZeroMQ.

## Backup restore, gaps and downgrade

Normal invariant: broker acknowledged position `B <= S` (RCH stored position)
and handled position `A <= S`. When `B < S`, RCH reuses its persisted batch receipt
and retries the missing ACK. Wrong owner/subscription, forged/unissued receipts
and changed journal identity fail explicitly. Older valid duplicate ACKs are
idempotent. Unknown required event versions pause ordered application as
`UpgradeRequired`; supported malformed input commits `RchRejected` and advances
handled progress without claiming successful application.

If restoring a consistent daemon backup, stop both services and start the
restored daemon **once** with both `--zmq-durable-broker --zmq-broker-restored`.
This explicitly creates a new journal incarnation/receipt secret and invalidates
old issued receipts while preserving stored messages and operation receipts.
Remove the restore flag for subsequent normal starts. Treat the resulting RCH
journal mismatch as recovery required. Records admitted after the backup are
not magically present in that backup. Preserve the unrecovered RCH inbox,
original files and operation IDs for reconciliation.

If RCH is restored behind `B`, loses its state directory, or sees a changed
journal, stop affected intake/dispatch and investigate using consistent paired
backups and recorded identifiers. There is no automatic gap reconstruction,
consumer deletion, adoption of the broker tail or forced recovery API in this
first implementation. Operator recovery must establish what custody/business
work remains before a new installation/consumer is provisioned. Never edit
checkpoints to make readiness green. Restoring both databases is only safe when
their recorded custody/effect boundary is understood; external network effects
cannot be rolled back by a database restore.

Migration is forward-only. Old binaries do not enforce the new writer profile,
so do not run them against migrated stores. Before reverting, stop admissions,
account for all admitted inbox/dispatch work, and restore an agreed consistent
pre-cutover pair. RCH has no compatibility consumer to downgrade in place.
Physical network delivery may repeat after a crash; the same signed message
identity is reused. Exactly-once radio/remote side effects are not promised.

## Evidence before a release

Local acceptance is recorded in [PROGRESS.md](PROGRESS.md). Keep hosted CI,
production binaries, multi-hour production soak, real power interruption and
physical/public-network interoperability separate. The implementation is not a
published prerelease or proof of repair on the live server. Publish matching
revisions and independently verify package hashes before a controlled test
installation.
