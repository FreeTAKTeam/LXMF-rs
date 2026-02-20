# Compatibility Matrix

Last updated: 2026-02-19

## Python LXMF -> Rust LXMF-rs

Top-level module status is tracked here; method-level parity is tracked and enforced from `docs/plans/lxmf-parity-matrix.md`.

| Python module | Rust module | Status | Source of truth |
| --- | --- | --- | --- |
| `LXMF/LXMF.py` | `crates/libs/lxmf-core` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXMessage.py` | `crates/libs/lxmf-core` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXMPeer.py` | `crates/libs/lxmf-sdk` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXMRouter.py` | `crates/libs/rns-rpc` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/Handlers.py` | `crates/apps/reticulumd` + `crates/libs/rns-rpc` | done | `docs/plans/lxmf-parity-matrix.md` |
| `LXMF/LXStamper.py` | `crates/libs/lxmf-core` | done | `docs/plans/lxmf-parity-matrix.md` |

## Python Reticulum -> Rust Reticulum-rs

Detailed mapping and tests are tracked in `docs/plans/reticulum-parity-matrix.md`.

Release-track compatibility: `lxmf 0.3.0` targets `reticulum-rs 0.1.3` today (or pinned branch revisions during refactor).
