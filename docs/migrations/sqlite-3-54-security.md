# Bundled SQLite security update

Current main/source builds move to `rusqlite 0.41.1` and
`libsqlite3-sys 0.39.1`, which bundles SQLite 3.54.1. This addresses
[RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html)
(CVE-2026-119375): opening a crafted database with a sibling WAL of a different
page size could expose uninitialized page-cache memory. The advisory identifies
`libsqlite3-sys >= 0.39.0` as patched. No audit exception is added.

The wrapper declares Rust 1.89, so the main workspace declaration, build matrix
and current build guides now use that minimum. The independent Bluetooth leaf
retains its Rust 1.85 contract. The standalone E2E runner retains its independent
Rust 1.88 declaration. Versioned release notes and dated Rust 1.88 / SQLite
3.53.2 evidence remain historical and are not relabeled.

Existing `bundled` and `hooks` features are preserved. The upstream `hooks`
feature includes the new `wal` feature. RCH must align its direct `rusqlite`
requirement as well: the old wrapper requires native `libsqlite3-sys 0.38`, and
Cargo permits only one native library with `links = "sqlite3"` in the graph.

Upstream changes include PRAGMA-name validation and several iterator/callback
API changes. The source review found no uses of the changed column-iterator or
zeroblob interfaces. `Connection::release_memory` still calls the database-specific
`sqlite3_db_release_memory`, which works without the now-omitted global
`SQLITE_ENABLE_MEMORY_MANAGEMENT` define. No pager-release API adaptation is needed.

The lockfile update is limited to the wrapper and native library. New-engine
storage, broker/WAL, budget/retention, transport and MSRV validation are recorded
separately; previous engine timings and VM-step counts do not qualify SQLite 3.54.1.
