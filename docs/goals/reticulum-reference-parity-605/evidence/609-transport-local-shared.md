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

## Shared-boundary receive accounting and application trace

The pinned Python `RNS/Transport.py::_inbound` path removes the receive-side
hop added at a local shared-instance boundary before it classifies a packet
for routing. The Rust prequeue path previously performed the increment but
not the matching subtraction. The first real application trace exposed that
discrepancy: Rust attached and learned the Python announce, but its outbound
LXMF link request timed out because it retained a foreign transport header;
the Python shared owner observed no link. Commit `8ae20c54` mirrors the
reference rule: an attached client removes one hop when no local child
interfaces exist, while a shared parent removes a hop only for an actual
local-client child.

- `cargo test -p reticulum-rs-transport --lib shared_instance_ -- --nocapture` — 9 passed, including the new attached-client and parent/child receive-hop regressions.
- `cargo test -p reticulum-rs-transport --lib` — 801 passed.
- `cargo test -p lxmf-cli --bin lxmd` — 14 passed.
- `RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PY_REPO=.tmp/python-refs/LXMF LXMF_PYTHON_BIN=python3 cargo test -p lxmf-cli --test python_lxmd_remote_relay python_shared_instance_rust_lxmd_application_and_restart_e2e -- --ignored --nocapture` — passed against Reticulum `99de23c040d507e3fefca19e87b182302902725d` and LXMF `727830cefda83d9c6e3982b48675425f3f988f9c`.
  - Rust attached as a `local_client` to the Python-owned TCP shared instance and exchanged LXMF messages in both directions before restart.
  - Rust restarted from the same state, reattached, preserved the delivery destination identity, and exchanged both directions again; Python reported the delivery link active.
  - This is a direct one-hop/application/restart trace, not multi-hop or physical-network acceptance.

## Remaining acceptance boundary

The combined evidence now proves pinned Python↔Rust local attachment, announce
fan-out, a direct application/link exchange, and Rust daemon restart with
identity continuity. It does not yet compare multi-hop packet/proof/link or
Resource traffic, duplicate suppression across a multi-hop production path,
cached versus scheduled announce persistence, or link-close/stream reconnect
behavior. Those traces are still required before this row can be promoted.
Hardware and public-network evidence remain separate acceptance axes.
