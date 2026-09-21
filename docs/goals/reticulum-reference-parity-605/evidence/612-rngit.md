# #612 rngit permission/work/storage evidence

Status: **partial / unverified**. This records the bounded implementation and
the live Python Git/work service trace at the current forward-parity candidate.
The local concurrency/rollback update is sourced from commit
`0ad07dc054fa7ec21f114491bc0a96b29bb8b8cd`. It does not close #612 or #605.
The pinned-Python process-restart trace is sourced from commit
`409ef98e33efc0361cc6b90187ff51b7707ee3c4`.
The native Rust-client transport and its production compatibility bridge are
sourced from commit `3dcd5259`.

## Reference and ownership

- Forward reference: Reticulum `99de23c040d507e3fefca19e87b182302902725d`
  (`1.5.4-dev`), primarily `RNS/Utilities/rngit/server.py` and its client
  request shapes.
- Rust owner: `crates/apps/rns-tools/src/bin/rngit_parts` through the
  production `ReticulumGitNode` request handlers, the `git.repositories`
  destination, the existing local-client request seam, and the native
  `NativeRngitClient`/`ReticulumGitClient::attach_native_tcp` request path.
- The transport-neutral hash-only request seam remains available for local
  fixtures. The live Reticulum adapter now retains the identified peer's
  public key for work requests, which enables the Python signature contract on
  the production network path.

## Implemented behavior

| Area | Implemented and tested behavior | Status |
| --- | --- | --- |
| Permission sidecars | Canonical suffix paths (`.allowed`, `.work`, `.releases`), dotted repository names, ambiguous legacy file rejection, and legacy sidecar directories ignored | local verified |
| Dynamic permissions | Executable node-owned resolvers, bounded stdout/stderr (64 KiB), two-second execution limit, UTF-8/exit-status failure propagation, and no remote replacement | local verified on Unix; Python execution parity unverified |
| Permission state | Identity aliases, strict remote content validation, configured-group merging, deny preservation, blocked identities, administrator fallback, atomic replacement, and immediate in-memory refresh | local verified; differential parity unverified |
| Work storage | Python-shaped root and response maps, binary identity/signature fields, integer IDs, floating-point timestamps, separate numeric comment files, 256 KiB document bound, atomic MessagePack writes, node reload persistence, atomic work-directory reservation across independent writers, rollback when proposed-document permission setup fails, and pinned-Python process-restart persistence | local and pinned-Python restart trace verified |
| Work operations | List/view/create/propose/edit/comment/delete/complete/activate/perms through `handle_work_request`, with scope/ID validation, document ownership/permissions, atomic transitions, canonical document permission files, and authenticated-peer signature validation for create/propose/edit | local and pinned-Python production-path verified; full service matrix unverified |
| Cross-language data | A MessagePack fixture generated with Python `msgpack` is loaded and rendered by Rust, retaining binary author/signature/identity values; pinned Python Link requests reach Rust `git.repositories` Git paths plus `/mgmt/perms` and `/mgmt/work`, verify invalid and valid signatures, round-trip binary work metadata, exercise list/view/comment/edit/perms/complete/activate/delete, verify the Git bundle, mutate refs, register repositories, synchronize a configured remote, and clone fork/mirror targets. The native Rust client now sends Python-compatible `/git/list` and signed `/mgmt/work` requests to a pinned Python `git.repositories` server, including an oversized request Resource and production compatibility-client bridge. | fixture, both bounded request directions, and one process-restart persistence trace verified; broader cross-process/network restart/concurrency/fault matrix unverified |

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

```text
cargo fmt --all -- --check                                      PASS
cargo test -p rns-tools --bin rngit --all-features -- --nocapture PASS (39 tests)
cargo test -p rns-tools --tests                                  PASS
cargo clippy -p rns-tools --bin rngit --all-features --no-deps \
  -- -D warnings                                                 PASS
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --bin rngit \
  native_rust_client_requests_pinned_python_rngit -- --ignored --nocapture PASS
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_python_interop \
  -- --ignored --nocapture                                     PASS (page/media, Git paths, `/mgmt/perms`, and work lifecycle)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_python_interop \
  rngit_work_survives_process_restart_for_pinned_python_client \
  -- --ignored --nocapture                                     PASS (work create, Rust process restart, list/view persistence)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_python_interop \
  -- --ignored --nocapture --test-threads=1                      PASS (2 tests: page/media and work restart)
```

The focused `rngit` binary suite contains the resolver failure/remote-
replacement cases, configured access merging, dotted-name path safety, work
transitions, Python-produced MessagePack storage, an identified-peer
signature regression, independent-writer ID reservation, and proposed-work
rollback. The ignored Python traces passed after exercising both the Git and
work service paths through real Reticulum Links, including work persistence
across a Rust server process restart. The native Rust-client trace additionally
uses the production synchronous compatibility bridge, a direct request packet,
an oversized request Resource, identity identification, Python signature
verification, and Python response handling.

The repository module-size script still reports the pre-existing
`crates/libs/rns-transport/src/resource/manager.rs:555` over-budget baseline;
the changed `rngit.rs` test split and all new `rngit_parts` modules are within
the active 500-line module limit.

## Deliberate remaining gaps

- The transport-neutral local request seam still carries only the remote
  16-byte hash, so it intentionally cannot perform public-key verification.
  The native network adapter supplies the identified peer key and verifies
  create, propose, and edit signatures; comment requests remain unsigned as in
  the pinned Python client.
- The fixture proves Python-produced storage data, the reload regression proves
  work documents and document permission sidecars survive a fresh node load,
  the local regression proves independent node instances reserve distinct work
  directories and roll back failed proposed-document setup, and the live
  traces prove Python↔Rust Git and management request/response sessions in
  both directions through real Links, including one Rust process restart with
  list/view persistence. Broader cross-process/network restart behavior,
  malformed-document error transcripts, disk-fault injection, and the full
  Python CLI workflow remain unverified.
- Static malformed permission sidecars fail closed in Rust rather than being
  silently ignored like the pinned Python loader; this is an intentional safety
  difference and is not being called exact parity.
- The live service wiring now covers the pinned Python Git paths and
  `/mgmt/perms` plus `/mgmt/work` requests through `git.repositories`,
  including Link identification, the Python integer-key request shape, bundle
  validity, invalid-signature rejection, signed work creation/editing, binary
  metadata, work transitions, permission updates, and local-source fork and
  mirror cloning. The native Rust client now covers the reciprocal `/git/list`
  and signed `/mgmt/work` request direction, including an oversized request
  Resource. Reticulum-source cloning, release workflows,
  broader cross-process/network restart and concurrent-writer/fault transcripts,
  the full Python CLI workflow, and the broader #611 utility matrix remain
  open.
