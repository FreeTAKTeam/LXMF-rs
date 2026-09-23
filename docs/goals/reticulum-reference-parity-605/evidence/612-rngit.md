# #612 rngit permission/work/storage evidence

Status: **partial / unverified**. This records the bounded implementation and
the live Python Git/work service trace at the current forward-parity candidate.
The local concurrency/rollback update is sourced from commit
`0ad07dc054fa7ec21f114491bc0a96b29bb8b8cd`. It does not close #612 or #605.
The pinned-Python process-restart trace is sourced from commit
`409ef98e33efc0361cc6b90187ff51b7707ee3c4`.
The native Rust-client transport and its production compatibility bridge are
sourced from commit `3dcd5259`; reciprocal `/git/fetch` Resource handling and
exact bundle verification are sourced from commits `869b8c84` and `02b75605`;
reciprocal oversized `/git/push` and remote-ref verification are sourced from
`f9c5b81e`.
Native Python release list/view/latest/delete request-shape coverage is
sourced from `f26ce90d`; the full native `create/init` → `artifact` →
`finalize` sequence and raw artifact Resource handling are sourced from
`e0dedb0e`.

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
| Permission sidecars and companion roots | Canonical suffix paths (`.allowed`, `.work`, `.releases`); `repo` and `repo.git` are simultaneously registered and receive distinct service-written permission files, work documents, and release fixtures under isolated roots; the legacy `repo.git.with_extension("allowed")` path is shown to alias `repo.allowed` and is not used or migrated | local verified |
| Dynamic permissions | Executable node-owned resolvers, bounded stdout/stderr (64 KiB), two-second execution limit, UTF-8/exit-status failure propagation, no remote replacement, and failed resolver refresh preserving the loaded policy. A focused differential runs the same executable group `.allowed` with pinned Python `ReticulumGitNode.load_repository_group` / `resolve_permission` and Rust `ReticulumGitNode::load_repository_group` / `resolve_permission`; both allow the listed identity and deny an unlisted identity, with the exact resolver stdout `read:<listed-identity>\n` asserted | local verified on Unix; this ordinary resolver decision slice is differentially verified; broader execution parity remains unverified |
| Permission state | Identity aliases, strict remote content validation, configured-group merging, deny preservation, blocked identities, administrator fallback, atomic replacement, immediate in-memory refresh, and transactional configured-policy updates when sidecar reads fail. `repository_permission_set_takes_effect_before_handler_returns_without_restart` verifies an allowed decision before production `rperms` set and denial immediately after success on the same node; pinned Python's `_repository_set_permissions` likewise calls `update_repository_permissions` before returning success | local verified, including failed read/replacement rollback and the before/after service-handler transition; mixed-peer differential unverified |
| Work storage | Python-shaped root and response maps, binary identity/signature fields, integer IDs, Python-compatible integer timestamps for new records (while retaining legacy MessagePack float values on read), separate numeric comment files, 256 KiB document bound, atomic MessagePack writes, rejection of trailing bytes and non-map roots, node reload persistence, atomic work-directory reservation across independent writers, rollback when proposed-document permission setup fails, and pinned-Python process-restart persistence | local, pinned-Python restart, and bidirectional typed-value request/storage traces verified |
| Work operations | List/view/create/propose/edit/comment/delete/complete/activate/perms through `handle_work_request`, with scope/ID validation, document ownership/permissions, atomic transitions, canonical document permission files, and authenticated-peer signature validation for create/propose/edit | local and pinned-Python production-path verified; full service matrix unverified |
| Work document shape defaults and errors | Missing `meta` uses Python's `Untitled`/`markdown` defaults in view/list; missing `edited` defaults to zero even when `created` is present in document, list, and comment responses; malformed comment metadata is skipped, malformed root metadata returns `REMOTE_FAIL` / `Remote error` on view and is skipped in list, and an empty persisted root returns `REMOTE_FAIL` / `Error loading document` | local handler regression verified; malformed-shape mixed-peer behavior remains unverified |
| Malformed work requests and completion | Production `handle_request` rejects missing or wrongly typed repository keys and missing operations with exact response bodies and no filesystem changes; completing an existing work item with an unreadable root returns `REMOTE_FAIL` / `Error loading document` without moving it | local handler regression verified; new cases are not yet covered by a Python network trace |
| Permission alias casing | Permission names remain case-insensitive while special target aliases remain case-sensitive; `ALL` and `Everyone` do not grant access, matching the pinned Python parser's no-target result | local parser/allowed-input regression verified; broader permission differential remains unverified |
| Python work CLI | The pinned Python `rngit work` CLI runs create/list/view/edit/update/perms/complete/activate/propose/delete against the Rust service over real Reticulum Links; a deterministic editor and piped confirmation verify signed content, permission sidecars, transitions, and cleanup | local exact-reference trace verified; hosted result pending |
| Malformed work requests and storage | Pinned Python clients send invalid list scope, malformed document ID, and unknown operation through identified production Links; Rust rejects malformed persisted MessagePack roots and trailing bytes, and the Python `rngit work view` client receives `Remote error: Error loading document` | local unit and exact-reference mixed-peer trace; hosted result pending |
| Concurrent network work creation | Four independent pinned-Python processes simultaneously establish identified Links to one production Rust `rngit` server, create signed work documents, and verify unique numeric IDs each have persisted root files | local test and PR Verify automation added; hosted result pending |
| Production work authorization | The pinned Python `rngit work` CLI sets an explicit document `write:none` deny over a live Link, attempts a signed edit, receives a failed `Not allowed` response, and verifies both byte-identical persisted MessagePack and unchanged content from a subsequent service view; permissions are restored for remaining lifecycle checks | local production-network regression verified |
| Cross-language data | A MessagePack fixture generated with Python `msgpack` is loaded and rendered by Rust, retaining binary author/signature/identity values; pinned Python Link requests reach Rust `git.repositories` Git paths plus `/mgmt/perms` and `/mgmt/work`, verify invalid and valid signatures, round-trip binary work metadata, exercise list/view/comment/edit/perms/complete/activate/delete, verify the Git bundle, mutate refs, register repositories, synchronize a configured remote, and clone fork/mirror targets. The native Rust client now sends Python-compatible `/git/list`, `/git/fetch`, and oversized `/git/push` plus signed `/mgmt/work` and the multi-step release protocol to a pinned Python `git.repositories` server, including raw Git bundle and artifact Resource handling, exact `git bundle verify`, remote-ref verification after push, release creation/upload/finalization/list/view/latest/delete, and the production compatibility-client bridge. | fixture, both bounded request directions, integer timestamps and binary work values verified over Python↔Rust production Links and in storage; broader cross-process/network restart/concurrency/fault matrix unverified |

The focused `rngit_work_storage_preserves_msgpack_binary_and_integer_types_both_directions`
regression seeds a Rust MessagePack work record and has the pinned Python client
verify the production `/mgmt/work` response types and exact values. The pinned
Python process-restart creator also exposes its identity hash, public key, and
signature so Rust verifies their exact persisted bytes and integer timestamps.
This differential found Rust-generated timestamps were floating-point seconds,
where frozen Python `server.py` stores `int(time.time())`; new Rust timestamps
now use integer seconds. Existing records are not migrated, and the Python
float-valued fixture remains covered as a readable legacy representation.

The dedicated `rngit_work_view_preserves_explicit_nil_optional_signature`
regression stores a MessagePack `nil` for the optional work signature, submits
a Rust-encoded production `/mgmt/work` view request through
`ReticulumGitNode::handle_request`, and passes the production response payload
to the frozen Reticulum checkout's vendored `RNS.vendor.umsgpack` decoder (the
codec imported as `mp` by rngit's server). Python decodes the field as `None`,
confirming explicit nil remains distinct from a byte string. The binary identity
hash is already covered by the bidirectional production-Link regression above.

## Commands and results

The following validation ran in the current #612 follow-up worktree:

```text
cargo fmt --all -- --check                                      PASS
git diff --check                                                PASS
cargo test -p rns-tools --bin rngit --all-features              PASS (53 passed, 2 ignored)
cargo test -p rns-tools                                          PASS (default package suite; pinned-Python tests ignored)
cargo clippy -p rns-tools --all-targets --all-features --no-deps \
  -- -D warnings                                                 PASS
tools/scripts/check-boundaries.sh                               PASS
tools/scripts/check-module-size.sh                              PASS
cargo run -p xtask -- architecture-checks                        PASS
```

The recorded baseline commands below ran in the isolated
`codex/issue-605-parity` worktree. The new concurrent trace ran in
`corvo/issue-612-concurrent-python-work` against pinned Python Reticulum
`99de23c040d507e3fefca19e87b182302902725d` and passed three consecutive runs.

```text
cargo fmt --all -- --check                                      PASS
cargo test -p rns-tools --bin rngit --all-features                  PASS (44 passed, 1 ignored)
cargo test -p rns-tools --bin rngit canonical_companion_roots_isolate_repo_and_repo_git_through_service_reload --all-features PASS
cargo test -p rns-tools --tests                                  PASS
cargo clippy -p rns-tools --bin rngit --all-features --no-deps \
  -- -D warnings                                                 PASS
tools/scripts/check-module-size.sh                               PASS
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --bin rngit \
  native_rust_client_requests_pinned_python_rngit -- --ignored --nocapture PASS \
  (list, exact fetch bundle verification, oversized push/ref verification, \
   signed work, release create/upload/finalize/list/view/fetch/latest/delete)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_python_interop \
  -- --ignored --nocapture                                     PASS (page/media, Git paths, `/mgmt/perms`, and work lifecycle)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_python_interop \
  typed_msgpack::python_work_binary_values_and_integer_timestamps_survive_rust_storage_restart \
  -- --ignored --nocapture --exact --test-threads=1             PASS (exact binary/timestamp storage checks and comment persistence across restart)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_concurrent_python_interop \
  -- --ignored --nocapture --test-threads=1                     PASS (3 runs; 3 invalid requests per client and 4 concurrent signed clients with unique persisted IDs)
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools \
  --test rngit_concurrent_python_interop \
  pinned_python_rngit_work_cli_round_trips_production_service_lifecycle \
  -- --ignored --nocapture --exact --test-threads=1                PASS (create/list/view/edit/comment/perms/complete/activate/propose/delete)
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  typed_msgpack::rngit_work_storage_preserves_msgpack_binary_and_integer_types_both_directions \
  -- --ignored --nocapture --exact --test-threads=1                PASS (Rust-seeded storage to Python Link response)
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools \
  --test rngit_concurrent_python_interop \
  pinned_python_rngit_work_cli_round_trips_production_service_lifecycle \
  -- --ignored --nocapture --exact --test-threads=1                PASS (denied signed edit reports Not allowed; stored bytes and subsequent view unchanged)
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools \
  --test rngit_concurrent_python_interop \
  -- --ignored --nocapture --test-threads=1                         PASS (2 tests: concurrency and CLI/storage error response)
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --bin rngit \
  executable_allowed_resolver_matches_pinned_python_permission_decisions \
  -- --ignored --nocapture                                          PASS (same executable group resolver; listed/unlisted read decisions match)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_python_interop \
  -- --ignored --nocapture --test-threads=1                      PASS (2 tests: page/media and work restart)
```

The focused `rngit` binary suite contains the resolver failure/remote-
replacement cases, configured access merging, dotted-name path safety, work
transitions, Python-produced MessagePack storage, an identified-peer
signature regression, independent-writer ID reservation, and proposed-work
rollback. The companion collision regression drives both canonical permission
files through the permission service, creates work and release data for both
repositories, reloads the group, and verifies the roots remain separate without
legacy-path migration. `issue_612_permission_failure_tests` additionally
verifies that a failed configured-policy refresh after a malformed sidecar read is not applied
after repair/reload, a resolver execution failure preserves loaded permissions,
and a failed sidecar replacement does not alter cached permission state. The
ignored Python traces passed after exercising both the Git and
work service paths through real Reticulum Links, including work and numbered
comment persistence across a Rust server process restart and four concurrent
Python clients creating distinct persisted work items through one Rust service
process. The restart check verifies the comment ID and content in Python's
`work_view` response after restart. The native
Rust-client trace additionally
uses the production synchronous compatibility bridge, a direct request packet,
an oversized request Resource, identity identification, Python signature
verification, Python response handling, and `/git/fetch`'s raw Resource bundle
shape, and `/git/push`'s bundle-backed ref update. The fetched payload is
checked with `git bundle verify` and contains the expected `refs/heads/main`
ref; the pushed `refs/heads/feature` is checked against the source repository's
new commit. The same session creates a Python-compatible release through the
`create/init`, `create/artifact`, and `create/finalize` steps, verifies list and
view responses, fetches the uploaded artifact through a raw Resource, updates
latest, and deletes the release directory.

The focused `dotted_repository_release_requests_isolate_colliding_sibling_companion`
regression is an authorized production `handle_release_request` test for the
dotted `group/repo.git` repository. It pre-seeds the colliding sibling
`repo.releases` with a published `v-collision` release and latest marker, then
creates `v-dotted` and exercises `list`, `view`, and `latest`. It asserts the
new metadata and latest marker are under `repo.git.releases`, the sibling latest
marker remains byte-for-byte unchanged, the dotted list/latest responses contain
only the canonical release, and viewing the sibling-only tag returns
`RES_NOT_FOUND`. At PR head `b98511388da52315bf2b9f0570c2487ad6b17378`, the
handler already routes these operations through `companion_path`; this test
exposes no production behavior bug, so no implementation change was needed.
This deterministic local filesystem regression does not establish pinned-Python
or mixed-peer behavior for the new collision case, nor non-Unix filesystem
semantics.

The module-size gate passes. The new permission-failure regressions are split
into `issue_612_permission_failure_tests.rs`; all changed `rngit_parts` modules
remain within the active 500-line module limit.

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
  concurrent-client trace proves distinct network work IDs and persisted roots;
  the other live traces prove Python↔Rust Git and management request/response
  sessions in both directions through real Links, including one Rust process
  restart with list/view persistence. The concurrent trace also covers invalid
  list scope, malformed document ID, and unknown operation responses before
  valid creates. A separate exact-reference Python CLI trace covers the
  work-document lifecycle over real production Links with deterministic editor
  input. Local regressions reject invalid/trailing MessagePack, verify Python's
  missing/malformed metadata defaults and error behavior, and pin exact
  malformed top-level request and corrupt-completion responses without state
  changes. The pinned Python CLI observes the reference-compatible remote
  failure for a corrupt persisted root. The new shape and completion cases are
  not yet exercised over a mixed-peer Link. Other malformed-document
  operations, broader cross-process/network
  restart behavior, disk-fault injection beyond the permission refresh and
  replacement cases, non-work Python CLI commands, and the complete utility
  workflow remain unverified.
- Static malformed permission sidecars fail closed in Rust rather than being
  silently ignored like the pinned Python loader; this is an intentional safety
  difference and is not being called exact parity.
- Special permission target aliases are case-sensitive as in the pinned
  parser: case variants such as `ALL` and `Everyone` produce no Rust grant.
  Rust's strict remote-content validator rejects such invalid lines instead of
  accepting and silently skipping them as Python does; this fail-closed
  validation difference is intentional and is not called exact parity.
- The executable-permission differential covers ordinary successful stdout
  interpretation and one allowed plus one denied identity through each
  implementation's production loader and permission resolver. It does not
  establish parity for timeout, output bounds, decoding/exit failures, or all
  permission combinations; those broader acceptance criteria remain open.
- The live service wiring now covers the pinned Python Git paths and
  `/mgmt/perms` plus `/mgmt/work` requests through `git.repositories`,
  including Link identification, the Python integer-key request shape, bundle
  validity, invalid-signature rejection, signed work creation/editing, binary
  metadata, work transitions, permission updates, and local-source fork and
  mirror cloning. The native Rust client now covers the reciprocal `/git/list`
  and `/git/fetch` request direction plus signed `/mgmt/work`, including exact
  `git bundle verify`, an oversized request Resource, and an oversized bundle
  push with remote-ref verification. Native release creation/upload/finalization
  and artifact fetch are covered for one bounded fixture. Reticulum-source
  cloning and the remaining release workflows,
  broader cross-process/network restart and concurrent-writer/fault transcripts,
  non-work Python CLI command paths, and the broader #611 utility matrix remain
  open.
