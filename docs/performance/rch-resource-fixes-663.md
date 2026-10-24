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

## Local harness and 10-minute smoke baseline, 2026-10-24

The issue 663 follow-through adds a repeatable, production-shaped harness
(`tools/scripts/resource_soak_663.py`, wrapped by
`cargo xtask resource-soak-663`). The fixture generator writes a
durable-broker `reticulum.db` with bulk propagation inventory, mixed-case peer
history and a deliberately grown WAL, plus a Python-compatible Reticulum
storage root with cached announces. The runner starts `reticulumd` under
`systemd-run --user --scope -p MemoryHigh=768M -p MemoryMax=1G` pinned to two
CPUs, drives it with an RCH-shaped 100 ms ZeroMQ event poller, and samples
`smaps_rollup`, `/proc/<pid>/io`, cgroup v2 memory and CPU files, WAL size and
the daemon `/metrics` JSON.

Reproduce:

```sh
cargo xtask resource-soak-663 -- fixture --root target/soak-663/fixture \
  --entries 200000 --announces 100000 --wal-bytes 240000000
cargo xtask resource-soak-663 -- run --root target/soak-663/fixture \
  --out target/soak-663/baseline-idle --profile idle --duration-secs 600
```

Fixture: 200,000 propagation entries of 2 KiB (database 1,134 MiB, WAL
551 MiB), 300 peers with 600,000 history rows, 100,000 cached announces.
Host: Linux 7.0, 22 cores with the daemon pinned to two, rustc 1.96.0, release
profile, baseline daemon `main` at `8f872639`. Ten-minute idle profile (100 ms
SDK event polls, no network peer):

| Measure | First minute | Last minute |
| --- | ---: | ---: |
| process RSS | 185.2 MiB | 220.2 MiB |
| process anonymous | 165.1 MiB | 199.7 MiB |
| process swap | 0 | 0 |
| cgroup memory.current | 183.8 MiB | 215.1 MiB |
| cgroup file cache | 16.3 MiB | 13.2 MiB |
| WAL file | 550.9 MiB | 550.9 MiB |

| Rate or delta over 596 s | Value |
| --- | ---: |
| time to `/readyz` (WAL replay plus 100,000 cached announces) | 206 s |
| daemon CPU | 4.7 % of one core |
| logical reads (`rchar`) | 8.1 MiB/s |
| read syscalls | 2,080/s |
| physical reads | 0 |
| `memory.events` high / max / oom | 0 / 0 / 0 |
| store write ops / read ops | 3 / 47,191 |
| SDK polls / errors | 5,997 / 0 |
| SDK poll p95 latency (median of 5 s windows / max) | 739 us / 1,024 us |

Two observations from the idle baseline stand out and are not yet attributed:

- Anonymous memory grew by about 35 MiB in ten idle minutes with no
  announces and no messages. Item 4 of the plan owns the attribution.
- The daemon performed 47,191 store read operations for 5,952 SDK polls, about
  eight per empty poll, and about 2,080 read syscalls per second of roughly one
  SQLite page each. The poll handler itself does not touch the store, so the
  reads come from the request path around it. This is the "empty polls
  repeatedly materialize persistent state" candidate from the original report;
  it is recorded here for item 2 (per-operation counters) and item 3 to act on.

The run is a harness smoke check of the baseline daemon (`main` at
`8f872639`), not acceptance evidence: ten minutes on one host cannot show a
multi-hour memory plateau, and the ingress profile drives real announces but no
inbound, outbound or fanout messages. Those remain listed under item 6 of
[the execution plan](../goals/issue-663-resource-acceptance/PLAN.md).

## Acceptance boundary

Local SQL plans, regressions and socket tests do not establish a multi-hour
memory/swap plateau or fresh delivery on the production droplet. Remaining
acceptance uses its production-shaped database/WAL and announce inventory,
two cores, 768 MiB MemoryHigh and 1 GiB MemoryMax; measures CPU, logical reads,
anon/file/cache, swap and reclaim deltas; and verifies durable inbound,
outbound/fanout, restart and lost-ACK recovery. Hardware, public-network and
production deployment evidence are not claimed by this increment.
