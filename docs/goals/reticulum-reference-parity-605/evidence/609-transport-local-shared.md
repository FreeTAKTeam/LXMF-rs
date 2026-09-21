# Issue #609: transport, local-client, and shared-instance behavior

Status: implemented but unproven. This is a forward-candidate software slice,
not a claim of mixed-peer or hardware acceptance.

## Reference and scope

- Target Reticulum revision: `99de23c040d507e3fefca19e87b182302902725d`.
- Reference surfaces: `RNS/Transport.py`, `RNS/Reticulum.py`, and
  `RNS/Interfaces/LocalInterface.py`.
- Rust owners: `rns-transport` interface-manager, announce table, and
  announce processing; `reticulumd` local TCP/Unix startup.

The implementation now classifies a local client from its parent relationship
and the parent's shared-instance marker. Active local TCP and Unix listeners
are marked as shared-instance parents; accepted children inherit the runtime
configuration and remain distinguishable from the parent. An announce from a
local client is queued with an immediate timeout and the retry limit already
consumed, producing one immediate transport retransmit before the entry moves
to the bounded cache. Accepted announces are sent directly to sibling local
clients as Type-2 transport announces, excluding the receiving child, so this
fan-out does not become accidental network broadcast/transit.

## Local software evidence

- `cargo test -p reticulum-rs-transport --lib` — 794 passed.
- `cargo test -p reticulumd --bin reticulumd` — 465 passed.
- Focused regressions cover parent/child classification, immediate single
  retransmit, sibling direct fan-out, passive transport admission, and the
  existing announce-table response/cache behavior.
- `cargo clippy -p reticulum-rs-transport --lib --all-features --no-deps -- -D warnings`
  — passed.
- `tools/scripts/check-module-size.sh` and
  `tools/scripts/check-boundaries.sh` — passed.
- `RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LOG_DIR=target/interop/local-interface-python-shared-605 REPORT_PATH=target/interop/local-interface-python-shared-605/report.json TIMEOUT_SECS=45 bash tools/scripts/local-interface-python-shared-smoke.sh` — passed.
  - candidate: `92ce5720` (`codex/issue-605-parity`)
  - Python Reticulum: `99de23c040d507e3fefca19e87b182302902725d`
  - TCP and Linux abstract Unix `LocalClientInterface` rows reported
    `startup_status = attached`.
  - Both Python shared instances observed two live local clients with
    non-zero receive/transmit counters; each pinned Python traffic client
    connected and emitted three announces.
  - raw report: `target/interop/local-interface-python-shared-605/report.json`

## Remaining acceptance boundary

The shared-instance smoke proves only pinned Python↔Rust local attachment and
announce fan-out. It does not yet compare application packet/proof/link or
Resource traffic, duplicate suppression across a multi-hop production path,
daemon replacement/reconnect, cached versus scheduled announce persistence,
or close/reconnect behavior. Those traces are still required before this row
can be promoted. Hardware and public-network evidence remain separate
acceptance axes.
