# #612 rngit permission/work/storage evidence

Status: **partial / unverified**. This records the bounded implementation and
the live Python `/git/list`, `/git/fetch`, and `/git/push` service trace at
candidate commit `83ee0ce0` on the forward parity branch. It does not close
#612 or #605.

## Reference and ownership

- Forward reference: Reticulum `99de23c040d507e3fefca19e87b182302902725d`
  (`1.5.4-dev`), primarily `RNS/Utilities/rngit/server.py` and its client
  request shapes.
- Rust owner: `crates/apps/rns-tools/src/bin/rngit_parts` through the
  production `ReticulumGitNode` request handlers, the `git.repositories`
  destination, and the existing local-client request seam.
- The implementation preserves the existing transport-neutral boundary. A
  live Reticulum adapter still has to supply the identified peer's public key
  if work signatures are to be validated.

## Implemented behavior

| Area | Implemented and tested behavior | Status |
| --- | --- | --- |
| Permission sidecars | Canonical suffix paths (`.allowed`, `.work`, `.releases`), dotted repository names, ambiguous legacy file rejection, and legacy sidecar directories ignored | local verified |
| Dynamic permissions | Executable node-owned resolvers, bounded stdout/stderr (64 KiB), two-second execution limit, UTF-8/exit-status failure propagation, and no remote replacement | local verified on Unix; Python execution parity unverified |
| Permission state | Identity aliases, strict remote content validation, configured-group merging, deny preservation, blocked identities, administrator fallback, atomic replacement, and immediate in-memory refresh | local verified; differential parity unverified |
| Work storage | Python-shaped root and response maps, binary identity/signature fields, integer IDs, floating-point timestamps, separate numeric comment files, 256 KiB document bound, and atomic MessagePack writes | local verified |
| Work operations | List/view/create/propose/edit/comment/delete/complete/activate/perms through `handle_work_request`, with scope/ID validation, document ownership/permissions, atomic transitions, and canonical document permission files | local verified through attached-node production handlers |
| Cross-language data | A MessagePack fixture generated with Python `msgpack` is loaded and rendered by Rust, retaining binary author/signature/identity values; pinned Python Link requests reach Rust `git.repositories` `/git/list`, `/git/fetch`, and `/git/push`, return the main ref, verify the bundle, and create a new remote ref | fixture and bounded request/response verified; full service matrix unverified |

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

```text
cargo fmt --all -- --check                                      PASS
cargo test -p rns-tools --all-features                          PASS
cargo clippy -p rns-tools --bin rngit --all-features --no-deps \
  -- -D warnings                                                 PASS
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_python_interop \
  -- --ignored --nocapture                                     PASS (page/media plus `/git/list`, `/git/fetch`, and `/git/push`)
```

The package test suite completed three repeated full runs after the resolver
spawn retry was added; all passed. The focused `rngit` binary suite contains
19 passing tests, including resolver failure/remote-replacement cases,
configured access merging, dotted-name path safety, work transitions, and the
Python-produced MessagePack fixture.

The repository module-size script still reports the pre-existing
`crates/libs/rns-transport/src/resource/manager.rs:555` over-budget baseline;
the changed `rngit.rs` test split and all new `rngit_parts` modules are within
the active 500-line module limit.

## Deliberate remaining gaps

- Rust's current rngit request seam carries the remote 16-byte hash, not the
  identified peer's Reticulum public signing key. Work signatures and identity
  fields are persisted and returned, but create/propose/edit/comment requests
  are not yet cryptographically verified as Python does.
- The fixture proves Python-produced storage data, and the live trace proves
  Python↔Rust `/git/list`, `/git/fetch`, and `/git/push` request/response
  sessions. Restart reload, malformed-document error transcripts, concurrent
  writers, and disk-fault injection remain unverified.
- Static malformed permission sidecars fail closed in Rust rather than being
  silently ignored like the pinned Python loader; this is an intentional safety
  difference and is not being called exact parity.
- The live service wiring now covers the pinned Python `/git/list`,
  `/git/fetch`, and `/git/push` requests through `git.repositories`, including
  Link identification, the Python integer-key request shape, bundle validity,
  write permission, and remote-ref creation. Delete/create/fork/sync/mirror,
  release/work network workflows, signature verification, restart/concurrent
  writer/fault transcripts, and the broader #611 utility matrix remain open.
