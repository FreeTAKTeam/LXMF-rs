# Propagation payload retention uses bounded candidate windows

The #663 resource fix limits each payload-retention candidate set to the 4,096
oldest rows, ordered by `received_at ASC, transient_id ASC`. The existing weight,
age and destination-priority ordering applies within that window. Cross-window
selection follows age: an old prioritised destination can be evicted before a
newer ordinary destination outside the current window. Prioritisation continues
to reduce eviction weight inside each window; it is not an absolute exemption.

This replaces global weight sorting, which materialised every inventory row when
over budget. No schema or data migration is needed. Limits and custody/ACK
semantics do not change. The low-level library retains caller-owned transaction
semantics. `MessagesStore` commits each window separately, releases its writer
and yields to waiting control work before continuing. A failure rolls back its
current window while earlier windows stay committed; the error is preserved and
metrics report the final deleted-row count as unknown rather than zero.

The gate was measured on 2026-10-28 with the same 200,000-entry/100,000-announce
fixture under MemoryHigh=768M/MemoryMax=1G on two CPUs. Payload pruning took
8.08 seconds and 397,365,248 physical read bytes; quota pruning took 1.74 ms with
zero physical reads. Only payload pruning was paged because quota was not the
measured driver. Multi-hour production resource acceptance remains separate.
