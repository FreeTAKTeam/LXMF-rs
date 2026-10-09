# ZeroMQ / RCH durable message broker

Status: paired local implementation candidate; deployment qualification remains open.
See [PROGRESS.md](PROGRESS.md) for verified evidence and [OPERATIONS.md](OPERATIONS.md) for cutover and recovery.

## Outcome

`reticulumd` owns durable LXMF admission and replay. The LXMF-rs SDK owns
socket and protocol recovery. RCH accepts durable custody in a transactional
inbox, then applies business changes with duplicate-safe processing and
recoverable outbound intents.

Distinguish daemon storage, RCH storage and RCH application completion.
Negotiate the new `ack_stored` meaning explicitly; do not redefine existing
network receipts or `Delivered` status.

## Scope

The first release supports one daemon, local durable storage, one active RCH
process and one ordered consumer per configured service identity/subscription.
Preserve RNS/LXMF wire formats, destination ownership and public RCH routes.
Replication, competing consumers and shared-consumer clustering are excluded.

## Implementation plan

The [reviewed v2 Rust implementation plan](PLAN.md) defines design decisions,
repository ownership, work packages W0-W5, migration requirements and tests.
Repair recovery and cancellation first, then prove one durable `Test1234`
handoff and its business application. Qualify ROUTER/DEALER independently;
do not combine endpoint migration with storage migration.

## Acceptance evidence

Use recorded LXMF, operation and event identifiers rather than message text as
deduplication keys. Prove replay across daemon and RCH restarts, lost replies
and acknowledgements, and RCH failure after custody acknowledgement but before
application. Require one durable intake record and no repeated committed local
business effect.

Verify bounded admission, workload isolation, explicit storage and recovery
failures, native SQLite durability settings, supported-version compatibility
and a tested paired deployment. Keep missing runtime evidence explicit; the
plan is not evidence that the production fault is fixed.
