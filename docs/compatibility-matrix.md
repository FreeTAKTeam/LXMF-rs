# Compatibility Matrix

Last updated: 2026-02-09

## Python LXMF -> Rust LXMF-rs

| Python module | Rust module | Status |
| --- | --- | --- |
| `LXMF/LXMF.py` | `src/constants.rs`, `src/helpers.rs` | done |
| `LXMF/LXMessage.py` | `src/message/*` | partial |
| `LXMF/LXMPeer.py` | `src/peer/mod.rs` | partial |
| `LXMF/LXMRouter.py` | `src/router/mod.rs` | partial |
| `LXMF/Handlers.py` | `src/handlers.rs` | partial |
| `LXMF/LXStamper.py` | `src/stamper.rs`, `src/ticket.rs` | partial |
| `LXMF/Utilities/lxmd.py` | `src/bin/lxmd.rs` | partial |

## Python Reticulum -> Rust Reticulum-rs

Detailed mapping and tests are tracked in `docs/plans/reticulum-parity-matrix.md`.
