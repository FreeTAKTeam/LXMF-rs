# Compatibility Matrix

Last updated: 2026-02-11

## Python LXMF -> Rust LXMF-rs

Top-level module status is tracked here; method-level parity is tracked and enforced from `docs/plans/lxmf-parity-matrix.md`.

| Python module | Rust module | Status | Source of truth |
| --- | --- | --- | --- |
| `LXMF/LXMF.py` | `crates/lxmf/src/constants.rs`, `crates/lxmf/src/helpers.rs` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXMessage.py` | `crates/lxmf/src/message/*` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXMPeer.py` | `crates/lxmf/src/peer/mod.rs` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXMRouter.py` | `crates/lxmf/src/router/mod.rs` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/Handlers.py` | `crates/lxmf/src/handlers.rs` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXStamper.py` | `crates/lxmf/src/stamper.rs`, `crates/lxmf/src/ticket.rs` | done | `docs/plans/lxmf-parity-matrix.md` |

## Python Reticulum -> Rust Reticulum-rs

Detailed mapping and tests are tracked in `docs/plans/reticulum-parity-matrix.md`.

Release-track compatibility: `lxmf 0.2.1` targets `reticulum-rs 0.1.3` today (or pinned branch revisions during refactor).
