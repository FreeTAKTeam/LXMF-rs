# P0 closure progress (#663, #608, #609, #625)

Started 2026-10-28 from main `45b5f565`. One PR per [PLAN.md](PLAN.md) item.

| Item | Issue | State | Branch / PR / head | Evidence |
| --- | --- | --- | --- | --- |
| 1. Attribute and bound daemon memory | #663 | Not started | | |
| 2. systemd readiness notification | #663 | Not started | | |
| 3. Maintenance-pass metrics and quota paging | #663 | Not started | | |
| 4. IFAC alias fix, CI wiring, evidence refresh | #608 | Not started | | |
| 5. IFAC remaining carrier-family traces | #608 | Not started | | |
| 6. Designated no_std core and seeded checks | #625 | Not started | | |
| 7. Duplicate-filter differentials into CI | #609 | Not started | | |
| 8. Two-relay reverse delivery diagnosis | #609 | Not started | | |
| 9. Shared-instance and restart matrix | #609 | Not started | | |

## Baseline carried forward (2026-10-28)

- Preview.30 operator sample: daemon anon 357 MiB, cgroup RAM 755 MiB at a
  768 MiB high, +1,857 MemoryHigh events in 1 h 44 min, CPU 6.5 %, persistence
  21 runs per 1 h 44 min, WAL 39 MiB with one truncation, poll errors ~0.002 %,
  15 RCH startup retries when started before daemon readiness.
- #608: 14 of 20 carrier families qualified locally; six open rows listed in
  `docs/goals/open-issues-2026-10/TRANSPORT.md:92-97`.
- #609: two-relay reverse delivery 1 pass / 2 timeouts at #629 head; five
  duplicate-filter differentials ignored and absent from Verify.
- #625: only `reticulum-rs-core` built for thumbv7em; no seeded-violation
  test; `lxmf-core` re-enables `rns-core/std` for every dependant.
