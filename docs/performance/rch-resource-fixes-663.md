# RCH resource regressions: 663, 657 and 655

The [issue 663 investigation](https://github.com/FreeTAKTeam/LXMF-rs/issues/663)
attributes measured daemon reads to propagation refill and announce-cache
persistence. The earlier [657](https://github.com/FreeTAKTeam/LXMF-rs/issues/657)
and [655](https://github.com/FreeTAKTeam/LXMF-rs/issues/655) reports concern private
memory/swap growth and ZeroMQ timeouts. File-cache pressure and retained private
memory are distinct measurements; reducing one path does not establish the
cause of every historical timeout.

This work starts from upstream `634e0fc7ebe112a258cabe65b92b71ea5ab28d06`.
That baseline already includes remaining-capacity enforcement, durable peer
inventory, SDK socket cancellation/reset, timeout-stage diagnostics and bounded
daemon dispatch. Those protections are retained rather than replaced with
longer timeouts, weaker acknowledgement or authorization checks.

## Propagation refill

The inventory has a newest-first `(received_at DESC, transient_id DESC)` index.
A persisted per-peer keyset cursor records scan progress and completed passes.
Each transaction examines at most 128 candidate identifiers, uses indexed
membership checks, and counts effective unhandled entries through a partial
index capped at configured capacity. The public operation releases the shared
writer between pages. A singleton inventory generation tracks mutations without
updating every peer's cursor for each message.

Completed/received entries keep precedence over pending entries, including
case-variant peer histories. Existing per-peer capacity semantics and global
maintenance policy are retained. Reopen, mostly-known inventories, newly
arriving inventory and unchanged completed passes have focused regressions.
The approach reuses SQLite transactions and the existing writer owner;
it adds no competing connection or propagation queue authority.

## Announce cache

Each Transport keeps fingerprints of packet bytes and the bound interface for
only the current route snapshot and one storage root. A repeated known record
skips MessagePack encoding and cache-file open/read/write. Hashing still walks
packet data. Initial and five-minute verification use bounded exact-byte reads
to repair missing or corrupt files. Failed writes do not mark records clean;
removed routes are pruned even if a later snapshot write fails. A semaphore
serializes concurrent saves without holding transport locks across filesystem
I/O. Restart performs a fresh verification.

The explicitly run 10,000-record Linux cache-file qualification reported four
read syscalls attributable to sampling `/proc/self/io`, no write syscalls and
zero physical read/write bytes in its repeated-cache phase. This is an
operation-specific observation, not a full daemon CPU or memory benchmark.
Transport still exports paths and writes `destination_table` and `tunnels`.

Reproduce the cache qualification with:

```sh
cargo test --locked -p reticulum-rs-transport --lib \
  profile_repeated_announce_cache_writes -- --ignored --nocapture
```

The deterministic Transport regression also removes an already fingerprinted
file, confirms the fast save does not touch it, then verifies that fresh
Transport instances repair missing and corrupt records and restore the path.
Interface/header changes, write errors, verification scheduling and concurrent
saves are covered independently.

## Complementary RCH work

[RCH issue 276](https://github.com/FreeTAKTeam/Reticulum-Community-Hub/issues/276)
owns repeated RCH connection setup and historical inbox/dispatch queries.
Schema 8 adds partial pending-only indexes; high-frequency lane owners retain
configured connections with purge and joined-shutdown ownership. A controlled
10,000-completed-row fixture reduced empty inbox/dispatch VM steps from
75,018/30,009 to 17/10, unchanged between 32 and 10,000 history rows. These
results are separate from the daemon cache qualification.

## Local validation, 2026-10-15

The RPC, transport, SDK and daemon package test suite passed 2,957 tests with
128 explicit ignores using four test threads. The initial default-concurrency
run passed the changed RPC/transport/daemon targets but ten SDK unit tests hit
the shared process-wide ZMTP connection quota. The complete four-thread rerun
passed without changing production quotas, timeouts or assertions. Focused
refill tests additionally cover inventory inserted between pages after the
invocation boundary was captured.

The complementary RCH workspace passed 697 tests with two existing ignores.
All eight paired durable-broker tests also passed with a disposable RCH source
copy resolving the patched local LXMF 0.14.0 workspace. That qualification leaves
the repository's immutable production dependency pins unchanged. It covers
lost ACK and both restarts, replay, outbound admission/receipt reconciliation,
worker pause/resume/join and actual purge with retained worker connections.

The four-package test command is:

```sh
cargo test --locked -p reticulum-rs-rpc -p reticulum-rs-transport \
  -p lxmf-sdk -p reticulumd --tests --no-fail-fast -- --test-threads=4
```

## Acceptance boundary

Local SQL plans, regressions and socket tests do not establish a multi-hour
memory/swap plateau or fresh delivery on the production droplet. Remaining
acceptance uses its production-shaped database/WAL and announce inventory,
two cores, 768 MiB MemoryHigh and 1 GiB MemoryMax; measures CPU, logical reads,
anon/file/cache, swap and reclaim deltas; and verifies durable inbound,
outbound/fanout, restart and lost-ACK recovery. Hardware, public-network and
production deployment evidence are not claimed by this increment.
