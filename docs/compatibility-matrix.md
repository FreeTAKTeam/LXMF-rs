# Compatibility Matrix

Last updated: 2026-02-09

## Python LXMF -> Rust LXMF-rs

Top-level module status is tracked here; method-level parity is tracked and enforced from `docs/plans/lxmf-parity-matrix.md`.

| Python module | Rust module | Status | Source of truth |
| --- | --- | --- | --- |
| `LXMF/LXMF.py` | `src/constants.rs`, `src/helpers.rs` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXMessage.py` | `src/message/*` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXMPeer.py` | `src/peer/mod.rs` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXMRouter.py` | `src/router/mod.rs` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/Handlers.py` | `src/handlers.rs` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXStamper.py` | `src/stamper.rs`, `src/ticket.rs` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/Utilities/lxmd.py` | `src/lxmd/*`, `src/bin/lxmd.rs` | done | `docs/plans/lxmf-parity-matrix.md` |

## Python Reticulum -> Rust Reticulum-rs

Detailed mapping and tests are tracked in `docs/plans/reticulum-parity-matrix.md`.
