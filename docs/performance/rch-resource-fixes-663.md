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
storage root with 100,000 signed cached announces under the interface hash the
daemon's first interface receives. The runner copies the fixture per run,
starts `reticulumd` under `systemd-run --user --scope -p MemoryHigh=768M
-p MemoryMax=1G` pinned to two CPUs, drives it with an RCH-shaped 100 ms ZeroMQ
event poller, and samples `smaps_rollup`, `/proc/<pid>/io`, cgroup v2 memory
and CPU files, WAL size and the daemon `/metrics` JSON every five seconds. The
ingress profile adds a second plain daemon announcing every second over TCP.

Reproduce:

```sh
cargo xtask resource-soak-663 -- fixture --root target/soak-663/fixture \
  --entries 200000 --announces 100000
cargo xtask resource-soak-663 -- run --root target/soak-663/fixture \
  --out target/soak-663/baseline-idle --profile idle --duration-secs 600
cargo xtask resource-soak-663 -- run --root target/soak-663/fixture \
  --out target/soak-663/baseline-ingress --profile ingress --duration-secs 600
```

Fixture: 200,000 propagation entries of 2 KiB (database 1,134 MiB, WAL
551 MiB), 300 peers with 600,000 history rows, 100,000 cached announces. Host:
Linux 7.0, 22 cores with the daemon pinned to two, rustc 1.96.0, release
profile, baseline daemon `main` at `8f872639`, ten minutes per run.

| Measure | Idle, first minute | Idle, last minute | Ingress, first minute | Ingress, last minute |
| --- | ---: | ---: | ---: | ---: |
| process RSS | 82.4 MiB | 84.0 MiB | 487.3 MiB | 547.1 MiB |
| process anonymous | 62.7 MiB | 64.1 MiB | 466.4 MiB | 526.3 MiB |
| process swap | 102.5 MiB | 137.6 MiB | 0 | 0 |
| cgroup memory.current | 195.3 MiB | 560.8 MiB | 495.2 MiB | 557.4 MiB |
| cgroup file cache | 124.8 MiB | 487.9 MiB | 24.8 MiB | 26.9 MiB |
| WAL file | 550.9 MiB | 550.9 MiB | 550.9 MiB | 550.9 MiB |

| Rate or delta over the run | Idle | Ingress |
| --- | ---: | ---: |
| time to `/readyz` (WAL replay plus 100,000 cached announces) | 404 s | 289 s |
| daemon CPU, percent of one core | 5.7 | 13.0 |
| logical reads (`rchar`) | 7.9 MiB/s | 8.0 MiB/s |
| read syscalls per second | 2,031 | 2,691 |
| `memory.events` high / max / oom | 1,906 / 0 / 0 | 0 / 0 / 0 |
| store write ops / read ops | 3 / 47,191 | 1,903 / 48,052 |
| store write-lock wait, total | 0.001 ms | 19,029 ms |
| SDK polls / errors | 5,996 / 0 | 5,993 / 0 |
| SDK poll p95 latency, median of windows / max | 819 us / 6.3 ms | 790 us / 27.9 ms |

Observations the baseline attributes or leaves open:

- The idle profile reproduces the production picture: file cache grows by
  about 360 MiB and `memory.events high` fires 1,906 times in ten minutes with
  three writes. Reading 100,000 cached announce files back (startup, then the
  five-minute verification) is the only file traffic of that size.
- Idle store reads run at about 80 per second. Half are the durable dispatch
  pump in `rpc/daemon/sdk_outbound.rs`, which re-ran `broker_pending_dispatch`
  every 25 ms on an idle daemon; a probe through `handle_rpc` confirmed the SDK
  poll path itself performs no store reads. The remaining idle reads are not
  yet attributed.
- Under announce ingress, anonymous memory grows about 60 MiB in ten minutes
  (466 to 526 MiB) and writers wait 19 s in total across 1,903 writes while the
  refill runs; the daemon logged 410 "dropped path-table persistence trigger"
  warnings. These are the item 3 and item 4 targets.
- A SIGINT during startup rewrites `destination_table` empty, discarding the
  cached routes; the harness now works on a per-run fixture copy because of it.

## Operation-level metrics, 2026-10-24

The daemon metrics snapshot (`GET /metrics`) now carries `storage.refill`
(invocations, skips, pages, candidates examined, rows inserted, capacity stops,
wall and max page time, priority yields), `storage.wal` (checkpoint attempts and
outcomes, emergency reclaims, last WAL size), `memory` (process `smaps_rollup`
and the daemon's own cgroup v2 memory files, or an explicit unavailable
marker) and `providers.announce_persistence` / `providers.transport_tables`
(path-table save costs and in-memory table occupancy). The soak sampler records
all of them, so later runs attribute reads and memory per operation instead of
per process. Counters carry no payloads, identities or SQL values.

## Acceptance boundary

Local SQL plans, regressions and socket tests do not establish a multi-hour
memory/swap plateau or fresh delivery on the production droplet. Remaining
acceptance uses its production-shaped database/WAL and announce inventory,
two cores, 768 MiB MemoryHigh and 1 GiB MemoryMax; measures CPU, logical reads,
anon/file/cache, swap and reclaim deltas; and verifies durable inbound,
outbound/fanout, restart and lost-ACK recovery. Hardware, public-network and
production deployment evidence are not claimed by this increment.
