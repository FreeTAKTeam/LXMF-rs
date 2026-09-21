# #612 rngit permission/work/storage evidence

Status: **partial / unverified**. This records the bounded implementation and
the live Python Git/work service trace at the current forward-parity candidate.
It does not close #612 or #605.

## Reference and ownership

- Forward reference: Reticulum `99de23c040d507e3fefca19e87b182302902725d`
  (`1.5.4-dev`), primarily `RNS/Utilities/rngit/server.py` and its client
  request shapes.
- Rust owner: `crates/apps/rns-tools/src/bin/rngit_parts` through the
  production `ReticulumGitNode` request handlers, the `git.repositories`
  destination, and the existing local-client request seam.
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
| Work storage | Python-shaped root and response maps, binary identity/signature fields, integer IDs, floating-point timestamps, separate numeric comment files, 256 KiB document bound, and atomic MessagePack writes | local verified |
| Work operations | List/view/create/propose/edit/comment/delete/complete/activate/perms through `handle_work_request`, with scope/ID validation, document ownership/permissions, atomic transitions, canonical document permission files, and authenticated-peer signature validation for create/propose/edit | local and pinned-Python production-path verified; full service matrix unverified |
| Cross-language data | A MessagePack fixture generated with Python `msgpack` is loaded and rendered by Rust, retaining binary author/signature/identity values; pinned Python Link requests reach Rust `git.repositories` Git paths plus `/mgmt/perms` and `/mgmt/work`, verify invalid and valid signatures, round-trip binary work metadata, exercise list/view/comment/edit/perms/complete/activate/delete, verify the Git bundle, mutate refs, register repositories, synchronize a configured remote, and clone fork/mirror targets | fixture and bounded request/response verified; restart/concurrency/fault matrix unverified |

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

```text
cargo fmt --all -- --check                                      PASS
cargo test -p rns-tools --all-features                          PASS
cargo clippy -p rns-tools --bin rngit --all-features --no-deps \
  -- -D warnings                                                 PASS
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_python_interop \
  -- --ignored --nocapture                                     PASS (page/media, Git paths, `/mgmt/perms`, and work lifecycle)
```

The focused `rngit` binary suite contains the resolver failure/remote-
replacement cases, configured access merging, dotted-name path safety, work
transitions, Python-produced MessagePack storage, and an identified-peer
signature regression. The ignored Python trace passed after exercising both
the Git and work service paths through real Reticulum Links.

The repository module-size script still reports the pre-existing
`crates/libs/rns-transport/src/resource/manager.rs:555` over-budget baseline;
the changed `rngit.rs` test split and all new `rngit_parts` modules are within
the active 500-line module limit.

## Deliberate remaining gaps

- The compatibility/local request seam still carries only the remote 16-byte
  hash, so it intentionally cannot perform public-key verification. The live
  network adapter supplies the identified peer key and verifies create,
  propose, and edit signatures; comment requests remain unsigned as in the
  pinned Python client.
- The fixture proves Python-produced storage data, and the live trace proves
  Python↔Rust Git and management request/response sessions through a raw
  Python `RNS.Link`. Restart reload, malformed-document error transcripts,
  concurrent writers, disk-fault injection, and the full Python CLI workflow
  remain unverified.
- Static malformed permission sidecars fail closed in Rust rather than being
  silently ignored like the pinned Python loader; this is an intentional safety
  difference and is not being called exact parity.
- The live service wiring now covers the pinned Python Git paths and
  `/mgmt/perms` plus `/mgmt/work` requests through `git.repositories`,
  including Link identification, the Python integer-key request shape, bundle
  validity, invalid-signature rejection, signed work creation/editing, binary
  metadata, work transitions, permission updates, and local-source fork and
  mirror cloning. Reticulum-source cloning, release workflows, restart/
  concurrent-writer/fault transcripts, the full Python CLI workflow, and the
  broader #611 utility matrix remain open.
