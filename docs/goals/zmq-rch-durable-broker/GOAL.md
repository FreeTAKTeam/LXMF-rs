# ZeroMQ / RCH durable message broker

Status: Issue #659 is closed. The durable-broker implementation shipped in
v0.14.0; production deployment qualification remains a separate open activity.
See the [release notes](../../release-notes-v0.14.0.md) and
[migration guide](../../migrations/v0.14.0-zmq.md) for shipped behavior,
[PROGRESS.md](PROGRESS.md) for implementation evidence, and
[OPERATIONS.md](OPERATIONS.md) for cutover and recovery.

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

## Retired plan

The implementation plan is retired. Its former URL remains available as a
[retirement pointer](PLAN.md) for issue links.

## Qualification boundary

The release tests cover paired local SQLite and ZeroMQ behavior. They do not
establish a production RCH deployment, physical power-cut durability,
multi-hour load qualification, or the cause of a production incident. Follow
the operations runbook for a deployment and retain its actual revision and
runtime evidence.
