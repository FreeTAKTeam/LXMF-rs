# Issue 659 implementation and evidence

Scope authorized 9 October 2026: implement the paired daemon/SDK/RCH path.
ZeroMQ is the primary interface. Existing non-ZeroMQ SDK behavior remains
unchanged. RCH cuts over without a legacy consumer fallback. The reviewed
[PLAN.md](PLAN.md) remains the design reference; no production root cause or
deployed binary is assumed from source inspection.

## Contract and execution board

The daemon owns admission, its SQLite message/journal/operation transactions,
consumer receipts and dispatch recovery. The SDK owns sockets and finite
transport recovery. RCH owns its locked local store, inbox, ordered business
application and durable outbound intents. `ack_stored` acknowledges custody
only; existing acknowledgements and `Delivered` retain their meanings.

Cutover: opt-in, per-session negotiated broker methods on existing ZeroMQ
endpoints, then one required durable consumer in RCH. Legacy event polling is
preserved for other SDK clients, but removed as an RCH import authority.
No endpoint migration is coupled to schema migration. The existing owning
workers, SQLite stores and transaction code are reused; no broker service,
actor library or generic storage abstraction is added.

| Task | Owner, allowed files and output | Evidence and dependency |
| --- | --- | --- |
| W0 | Main implementation; read-only explorer roles. Map `rns-rpc` message/event writers, `lxmf-sdk` exchange, `reticulumd` ZeroMQ loops and RCH session/transaction paths. | Source audit; exact local source/native versions, declared endpoint/settings and bounded reproduction. Production binary facts remain unavailable. |
| W1 | Main; SDK ZeroMQ exchange/recovery and daemon ZeroMQ admission/ownership modules. Remove pooled-across-await sockets and detached/unbounded ROUTER dispatch. | Real cancellation, invalid/stale reply, safe retry budget, authenticated identity preservation, saturation and joined shutdown tests. Before W2. |
| W2 | Main; existing daemon SQLite writer, additive broker contract and SDK ZeroMQ operations; RCH core inbox and server intake. | Atomic commit/replay, authenticated fixed consumers, bounded storage/encoded responses, invalid/duplicate ACK, restarts around inbox/ACK with `Test1234`. After W1; no durable claim from in-memory stores. |
| W3 | Main; daemon admission/dispatch and RCH ordered transaction application/outbox. | Stable operation/fingerprint, admission reconciliation, crash/retry deduplication and business+completion+outbox atomicity. After W2. |
| W4 | Main; all required event producers and existing test utilities. | Real SQLite/socket/paired tests for failures, bounds, contention, retention/recovery and unchanged legacy SDK/wire contracts. After W3. |
| W5 | Main; additive migrations, required capability/bootstrap controls and runbook. | Existing-state cutover, explicit recovery/downgrade restrictions and paired revision/settings evidence. No live production deployment is authorized. After W4. |

Implementation is sequential in the main agent. Independent roles review
correctness, maintainability and final plan alignment, and verify evidence.
Incomplete packages remain explicitly incomplete; socket success alone never
closes the issue. Kill criteria: remove RCH's old polling/history-import
authority at durable cutover; retain one authoritative post-cutover producer
transaction and one ordered consumer. Forbidden: treating socket/session IDs as
principals, silently tailing expired/restored state, retrying uncertain ordinary
mutations, acknowledging before inbox commit or claiming exactly-once network
effects.

## W0 findings and W1 reproduction

Local framework baseline: `bed26158b2238a1d668d6c0f10a5b35f27de15e6`.
Local RCH baseline: `7ca63d18a9ed7f78468275aba9f4da2756114ca0`.
Both canonical checkouts contain unrelated work, preserved through isolated
implementation checkouts. No production SSH/runtime inspection was performed.

Two real socket regressions reproduce external-cancellation defects in both
PUSH/PULL and DEALER: after a submitted request's future is dropped, the shared
slot still contains its socket. The same tests pass after taking exclusive
local socket ownership across the exchange. Session state remains independent
of sockets. The inner RPC payload must also validate before pool return.

Other source findings: ROUTER tasks are detached and acquire permits only after
spawn; message/event/receipt writes are separate; existing event sequence is
volatile; consumer ownership and negotiated broker state must be per-session;
RCH has no durable inbox or production store lock and commits its cursor after
self-committing handlers. Current bundled SQLite source is 3.50.2 and current
connections use WAL/NORMAL. Durable mode needs a fixed native build and
verified WAL/FULL settings, with explicit weaker/in-memory rejection.

The pinned pure-Rust zeromq 0.6 codec can allocate advertised frames before
application receive, and its TCP listener owns separate handshake tasks.
Application count/byte guards alone do not prove a whole-process memory bound.
Library lifecycle/frame behavior must be qualified or corrected before the
whole-interface bounded-admission criterion is claimed.

## Implemented local candidate

W1-W3 are integrated across the paired source. ZeroMQ exchanges own sockets
across awaits; replay-safe broker methods have finite retries inside the original
budget. Daemon request/codec/connection/handshake admission is bounded before
allocation or spawning. The existing SQLite writer owns atomic inbound journal,
immutable issued-range receipts, scoped outbound admission/fingerprint and
persisted signed/propagation bytes. The native SQLite build is 3.53.2 with
verified WAL/FULL and writer-version rejection.

RCH commits an ordered inbox before ACK, then commits command/mission/checklist
changes, handled/rejected completion, attachments and generated outbound intents
in one unit of work. Logical input claims and generated business message IDs
include source ownership. Primary-only REM members retain exact announce lookup.
Bootstrap history cannot execute commands/relays. Unknown required versions pause;
recognized bad inputs are durable rejections. Legacy production polling/history
and retry owners have been removed. RCH compatibility is not a requirement.

The final resource pass guards shared daemon SQLite writes as well as broker
writes. Native page cap, job-start reservation and scoped commit hooks account
for cache spills and checkpoint growth without disabling spill. Pinned readers
backpressure ordinary writes; control and successful idempotent custody requests
remain available. Terminal receipt slots belong to durable operations across
fallback/retry. The owned worker retains failed persistence, joins its blocking
completion, and supervises fatal failure through daemon shutdown/admission.

## Verification

- Actual Rust 1.85: RCH workspace tests passed (703 tests, 2 explicit ignores),
  full workspace Clippy with `-D warnings` passed, and formatting passed.
  This includes the native rusqlite hooks build, inbox SIGKILL phases, command
  alias/scope authorization, primary-only REM routing, over-limit attachment
  rejection and two senders sharing the same command ID without intent collision.
- Rust 1.88: framework workspace/all-targets passed (3,632 tests, 288 existing
  explicit ignores). Subsequent complete affected SDK/RPC/daemon suites passed
  after the final writer-hook and receipt-supervision corrections (1,862 tests,
  119 explicit ignores). The added initial-budget CLI validation also passes.
- Paired real SQLite + SDK + ZeroMQ socket tests passed: `Test1234`, lost stored
  ACK and restart of both sides before application, replay beyond the old event
  window, lost admission response/reconciliation, actual owning worker
  pause/resume/join, and terminal receipt projection. Physical network delivery
  is stubbed in that harness; it does not establish live peer delivery.
- Daemon broker disk tests passed: issued-range restart replay, duplicate/wrong
  owner/scoped/forged ACK, no-consumer retention, atomic rollback, migration
  arrivals, prepared-byte restart and SIGKILL, completion capacity and retention,
  explicit backup restore incarnation, shared metadata pressure/idempotent ACK
  and checkpoint recovery. Tiny-cache real SQLite spills and multi-autocommit
  saturation exercise the physical guard with a pinned reader.
- Receipt tests passed: full progress queue with reserved terminal result,
  retained storage failure then one committed receipt/drain, typed SQLite busy
  classification, and fatal persistence failure notifying the daemon owner and
  rejecting further admission.
- Vendored ZeroMQ tests passed: 33 library tests and two real raw-handshake
  churn/same-peer replacement tests. Architecture boundary/module-size checks
  passed. The non-ZeroMQ daemon configuration compiles.
- Compiled RCH plus managed `reticulumd` started against temporary local-disk
  databases and loopback endpoints. `/Status` and `/diagnostics/runtime` returned
  200, durable broker lanes were healthy, native SQLite was 3.53.2/WAL/FULL,
  13 broker polls completed without errors, and the RCH shutdown exited zero
  with no daemon left running. This is startup/poll/shutdown evidence, not a
  live physical-peer delivery or load/plateau qualification.
- Strict framework-wide Clippy is blocked by pre-existing
  `clippy::uninlined_format_args` findings. Affected crates pass all-target/all-
  feature Clippy with `-D warnings -A clippy::uninlined_format_args`; this is a
  local qualification, not a repository lint-policy weakening or a green claim
  for the exact strict gate.

W4 local source/socket/SQLite evidence is complete for this candidate. W5
cutover/restore/downgrade instructions are in [OPERATIONS.md](OPERATIONS.md) and
RCH `docs/durable-broker.md`; immutable paired pins identify its source.
Publication and deployment are not authorized by this implementation request.
The source is retained in local branches; framework publication must precede
any remote RCH build that resolves the candidate Git revision.

Still unperformed: hardware power-cut qualification, production multi-hour
load/memory/swap plateau, public-network or hardware interoperability, full
release-readiness runner (PowerShell unavailable), and hosted CI. No production
root-cause confirmation, deployment, release readiness or issue closure is
claimed by these local tests.
