# Message-budget cap checks (#663)

The ordinary message-budget prune command previously summed the measured lengths
of every retained message before returning under its configured cap. A controlled
regression calls the actual synchronous writer command with generated local
fixtures and bundled SQLite 3.53.2. At a fixed 410,000,000-byte cap, two checks each
execute 3,811 VM progress callbacks with 128 rows and 280,227 with 10,000 rows.
The conservative-bound shortcut executes 247 in each case. These are command
instruction counts, not page I/O, admission/update cost or service measurements.
No completed runtime database or inferred runtime message population is used.

## Conservative state and exact decisions

Startup installs private `message_budget_bound` bookkeeping and three triggers
inside the existing schema transaction. The bound is nullable; NULL means unknown.
An existing population starts unknown and reconciles on its first needed cap
check. An intact reopen performs no hidden SUM, including when the bound is
unknown. Missing or changed bookkeeping triggers invalidate a surviving bound
before it can certify an under-cap population.

A BEFORE INSERT trigger charges the whole new row contribution. A BEFORE UPDATE
trigger charges only positive growth over the old contribution and never subtracts
on shrink or no-op updates. AFTER DELETE marks the bound unknown. Credits precede
row mutations that another AFTER trigger may retain while raising SQLite FAIL.
Ignored/failed statements, REPLACE with recursive triggers disabled, and repeated
shrink/regrow can overestimate safely. Invalid numeric state or overflowing
arithmetic becomes unknown rather than a fabricated small count. This invariant
assumes the installed bookkeeping schema; it does not promise safety against
arbitrary direct corruption of its private table or triggers.

The row contribution preserves the original SQL LENGTH expression exactly:
TEXT counts characters up to NUL, BLOB counts bytes, optional fields/status use
the existing COALESCE, and NULL in a required legacy field makes the entire row
contribution NULL. Cache credits coalesce that whole nullable contribution to zero,
matching SUM's treatment of it. Public storage statistics still calculate the
exact SUM and never expose the upper bound as exact usage.

A known bound at or below the cap proves no deletion is required. Otherwise a
short transaction evaluates the original SUM and stores its result consistently.
Only this exact total can govern real pruning. The original status eligibility,
oldest timestamp/ID ordering and durable 256-candidate limit remain unchanged.
Each DELETE still commits separately; a later error must retain earlier committed
deletes. No encompassing pruning transaction, cap increase, history purge,
durability change or schema payload rewrite is introduced.

Overestimation, deletion or uncertainty can still cause a history scan. The design
removes repeated scans when a valid under-cap bound suffices; it does not claim
constant work for every mutation, over-cap pruning or startup migration.

## Native controls and rejected variants

Nineteen focused controls cover exact cap boundaries; Unicode/NUL/BLOB/null
arithmetic; pending/terminal eligibility; ordering and deletion pages; schema
upgrade/reopen; missing/changed triggers; malformed/overflow state; rollback;
INSERT/UPDATE OR REPLACE; IGNORE and UPSERT; nested mutations; both recursive
trigger settings and trigger creation orders; late INSERT/UPDATE FAIL; and
partial-delete errors. A SUM-denying native authorizer independently checks the
no-scan fast path and fallback error propagation.

An AFTER-only prototype passed the first controls but failed two added native
late-FAIL regressions because retained growth could escape its accounting. It was
rejected. A safe BEFORE prototype charging full NEW on every UPDATE passed safety
controls but failed the near-cap cost controls: every unchanged/small-status
update caused 3,884/280,300 prune callbacks with 128/10,000 rows. Growth-only
credits pass all nineteen controls; each of nine subsequent prune commands costs
247 in both populations. Same-length updates also pass with SUM forbidden.
These counters cover the prune command after updates, not the updates themselves;
shrink/regrow does not imply the bound remains exact.

The original current-method raw RED log was accidentally overwritten by an early
prototype wrapper. Its original frozen test executable, source and separately
captured evidence metadata survive. A new execution of that frozen executable
reproduces the eight passing controls and one cost failure, with observed exit
101; its replay timestamps were not recorded. That replay is explicitly distinct
from the lost original raw log. The immutable review manifest separately binds
the disclosure, original surviving evidence, new replay, rejected variants and
nineteen-control candidate; SHA-256:
`a67bff0f2c90471a8f44027d33236e75a89d640029c6e0e285a005b64e6718b7`.
It captures the pre-relocation test source; the final conventional test module
retains the same assertions with adjusted module imports.

```sh
cargo test -p reticulum-rs-rpc --release --locked --lib message_budget
```

This is synthetic source-level evidence. Runtime attribution, exact-pair sustained
resource qualification and external-client acceptance remain separate evidence.
