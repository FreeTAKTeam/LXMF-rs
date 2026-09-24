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
| Dynamic permissions | Executable node-owned resolvers, bounded stdout/stderr (64 KiB), two-second execution limit, UTF-8/exit-status failure propagation, no remote replacement, and failed resolver refresh preserving the loaded policy. A focused differential runs the same executable group `.allowed` through pinned Python `ReticulumGitNode.load_repository_group` and Rust `ReticulumGitNode::load_repository_group`; it captures and compares actual resolver stdout, then verifies both allow the listed identity and deny an unlisted identity | local verified on Unix; this ordinary resolver decision slice is differentially verified; broader execution parity remains unverified |
| Permission state | Identity aliases, strict remote content validation, configured-group merging, deny preservation, blocked identities, administrator fallback, atomic replacement, immediate in-memory refresh, and transactional configured-policy updates when sidecar reads fail. Production Rust and pinned Python `handle_perms` / `resolve_permission` differentially verify an allowed read, successful `read:none` update, immediate denial, and blocked-identity denial. `configured_group_access_merges_with_sidecar_like_pinned_python` differentially verifies configured group read and sidecar administrator grants; admin inheritance also makes the sidecar admin identity readable. `group_permission_refresh_preserves_configured_access_like_pinned_python` adds production `gperms` coverage for retaining configured read access and sidecar admin inheritance after an update; its pinned-reference run passed against Reticulum `99de23c040d507e3fefca19e87b182302902725d`. Repository `rperms` also verifies immediate denial after a successful update | local and pinned-Python differential verified for initial configured access merge, group update retention/admin inheritance, repository revocation, blocked identity, and immediate refresh; failed read/replacement rollback verified locally; broader permission combinations remain unverified |
| Blocked former repository administrator response | With repository `read:none` and `admin:all`, a blocked identity requests `/mgmt/perms` `rperms/get` through Rust's production request dispatcher. The exact pinned Python `handle_perms` response is `NOT_FOUND` / `Not found` because blocked identities resolve to neither read nor admin; Rust now preserves the same response precedence. `blocked_former_repository_admin_gets_pinned_python_not_found_response` asserts byte-for-byte parity against Reticulum `99de23c040d507e3fefca19e87b182302902725d`. | focused production-handler differential verified; this exact repository permission-read case only |
| Blocked identity at work-handler boundary | A production `list` request is denied with the exact `Not found` response when the identity is blocked despite group `read:all`; the same `handle_work` response matches pinned Python | local handler and pinned-Python production-handler differential verified; broader operation/permission combinations remain unverified |
| Work storage | Python-shaped root and response maps, binary identity/signature fields, integer IDs, Python-compatible integer timestamps for new records (while retaining legacy MessagePack float values on read), separate numeric comment files, 256 KiB document bound, atomic MessagePack writes, rejection of trailing bytes and non-map roots, node reload persistence, atomic work-directory reservation across independent writers, rollback when proposed-document permission setup fails, and pinned-Python process-restart persistence | local, pinned-Python restart, and bidirectional typed-value request/storage traces verified |
| Work operations | List/view/create/propose/edit/comment/delete/complete/activate/perms through `handle_work_request`, with scope/ID validation, document ownership/permissions, atomic transitions, canonical document permission files, and authenticated-peer signature validation for create/propose/edit | local and pinned-Python production-path verified; full service matrix unverified |
| Work delete without permission sidecar | Frozen Python `_work_delete` unconditionally unlinks `<work-root>/<id>.allowed` and maps a missing file to `REMOTE_FAIL` / `Remote error` before removing the document. Rust now preserves that ordering and response instead of treating a missing sidecar as success. `work_delete_missing_permission_sidecar_matches_pinned_python` submits the same authorized delete through each production handler and compares exact status/body while confirming each document remains present. | Focused local pinned-Python production-handler differential verified against Reticulum `99de23c040d507e3fefca19e87b182302902725d`; broader work-operation matrix remains open |
| Work view missing document ID | The ignored `work_view_missing_document_id_matches_pinned_python_error` regression calls pinned RNS 1.5.4 `ReticulumGitNode.handle_work` and Rust `handle_work_request` with an authorized `view` request that omits `doc_id`. Both return `INVALID_REQ` with the exact body `No document ID specified`; neither creates a work directory. Rust now preserves that operation-specific error instead of returning the generic `Invalid document request`. | focused local pinned-Python production-handler differential verified against Reticulum `99de23c040d507e3fefca19e87b182302902725d`; this one request-shape case does not close the broader work-operation matrix |
| Work edit missing document ID | An authorized `edit` request with non-empty content and a valid signature for the identified peer, but no `doc_id`, reaches Python's `_work_edit` and returns `INVALID_REQ / No document ID specified`. Rust now checks edit authorization and the valid request signature before returning the same operation-specific missing-ID response; neither handler creates or mutates a work directory. | focused local pinned-Python production-handler differential verified against Reticulum `99de23c040d507e3fefca19e87b182302902725d`; only the valid-signature/missing-ID shape is covered, and the broader operation matrix remains open |
| Work view negative document ID | An authorized production `view` request with MessagePack integer `doc_id: -1` returns `NOT_FOUND` / `Not found` in pinned Python after integer coercion and lookup; Rust now returns the same response instead of rejecting the value early as an invalid document request. Neither handler creates a work directory. This is one numeric request-shape edge only. | focused local pinned-Python production-handler differential verified against Reticulum `99de23c040d507e3fefca19e87b182302902725d`; other malformed-ID shapes remain outside this case |
| Work scope selection for view/edit/comment | The ignored `work_view_edit_comment_ignore_valid_scope_in_pinned_python_order` differential uses pinned RNS 1.5.4 `ReticulumGitNode.handle_work` and Rust `handle_work_request_with_peer_identity`. For a document present only in `active` and each request's valid scope set to `completed`, both views report `active`; both signed edits use an Ed25519 signature valid for the edited content, return status-only success, and change only the active root; both comments create ID 1 beside the active root and return matching payloads. | focused local pinned-Python handler differential verified; other scopes, denied/error paths, and the broad #612 matrix remain open |
| Work activation destination creation | The ignored `activating_completed_work_without_destination_scope_matches_pinned_python` differential uses pinned RNS 1.5.4 `ReticulumGitNode.handle_work` and Rust `handle_work_request`: item 4 is activated from `completed` while the work root has no `active` directory. Both fixtures explicitly authorize the request to isolate the activation state transition; Python's permission resolver is stubbed, so this case does not establish authorization-resolver parity. Both handlers return success with byte-identical MessagePack `{id: 4, scope: "active"}`, remove `completed/4`, and create `active/4`. | focused local pinned-Python production-handler differential verified at Reticulum `99de23c040d507e3fefca19e87b182302902725d`; other activation authorization/error cases and broad #612 acceptance remain open |
| Work document shape defaults and errors | Missing `meta` uses Python's `Untitled`/`markdown` defaults in view/list; missing `edited` defaults to zero even when `created` is present in document, list, and comment responses; malformed comment metadata is skipped, malformed root metadata returns `REMOTE_FAIL` / `Remote error` on view and is skipped in list, and an empty persisted root returns `REMOTE_FAIL` / `Error loading document` | local handler regression verified; malformed-shape mixed-peer behavior remains unverified |
| Malformed work requests and completion | Production `handle_request` rejects missing or wrongly typed repository keys and missing operations with exact response bodies and no filesystem changes. A missing operation is rejected as `Invalid request` before repository read authorization, matching the pinned Python `handle_work` ordering; the regression removes read and administrator grants at both group and repository scope. Completing an existing work item with an unreadable root returns `REMOTE_FAIL` / `Error loading document` without moving it | local handler regression verified; the new authorization-order case has not been covered by a Python network trace |
| Permission alias casing | Permission names remain case-insensitive while special target aliases remain case-sensitive; `ALL` and `Everyone` do not grant access, matching the pinned Python parser's no-target result | local parser/allowed-input regression verified; broader permission differential remains unverified |
| Python work CLI | The pinned Python `rngit work` CLI runs create/list/view/edit/update/perms/complete/activate/propose/delete against the Rust service over real Reticulum Links; a deterministic editor and piped confirmation verify signed content, permission sidecars, transitions, and cleanup | local exact-reference trace verified; hosted result pending |
| Malformed work requests and storage | Pinned Python clients send invalid list scope, malformed document ID, and unknown operation through identified production Links; Rust rejects malformed persisted MessagePack roots and trailing bytes, and the Python `rngit work view` client receives `Remote error: Error loading document` | local unit and exact-reference mixed-peer trace; hosted result pending |
| Concurrent network work creation | Four independent pinned-Python processes simultaneously establish identified Links to one production Rust `rngit` server, create signed work documents, and verify unique numeric IDs each have persisted root files | local test and PR Verify automation added; hosted result pending |
| Concurrent work mutation contract | Pinned RNS 1.5.4 `Link.handle_request` dispatches each packet and completed request Resource by starting a daemon thread (`RNS/Link.py`); `ReticulumGitNode.handle_work` has no work-storage lock, `_work_get_next_id` and `_work_get_next_comment_id` scan then choose the next integer, and `_work_save_document` writes the fixed `<path>.tmp` before `os.rename`. This gives per-replacement behavior only when writers do not collide; it does not define linearizable concurrent edits/comments, collision-free IDs, or recovery from two writers sharing the same temporary path. Rust's production `process_request` holds `runtime.node`'s Tokio mutex over the full Git handler call, serializing all requests handled by one Rust server process. Rust's create path additionally reserves each candidate directory with `create_dir` and retries `AlreadyExists`; the unit concurrency test exercises eight independently cloned node instances and verifies eight distinct persisted roots. | Rust single-process request serialization and create reservation are verified locally; reference dispatch and storage guarantee inspected at pinned RNS 1.5.4. Concurrent same-document mutation semantics and concurrent comment-ID allocation are not promised by the Python implementation. Multiple processes sharing one work root are unsupported/undefined: neither implementation provides a cross-process storage-wide lock or a cross-process transaction contract. |
| Production work authorization | The pinned Python `rngit work` CLI sets an explicit document `write:none` deny over a live Link, attempts a signed edit, receives a failed `Not allowed` response, and verifies both byte-identical persisted MessagePack and unchanged content from a subsequent service view; permissions are restored for remaining lifecycle checks | local production-network regression verified |
| Same-Link permission revocation | The pinned Python client and Rust production service share one established Reticulum Link. The client reads the repository, sets `read:none` while retaining admin access through `/mgmt/perms`, and immediately issues `/git/list` on that same Link; Rust denies it as `NOT_FOUND`. Restoring `read:all` makes the next same-Link request succeed. Pinned Reticulum 1.5.4 `resolve_permission` reads current in-memory permission lists for each handler call, while `handle_perms` delegates each request to its permission handler; Rust's `process_request` likewise calls `handle_request_with_peer_identity` for each packet and the successful sidecar replacement updates loaded repository permissions before returning. | focused local Python-client/Rust-service production-Link regression verified; reference dispatch/resolution path inspected at `99de23c040d507e3fefca19e87b182302902725d`; no mismatch found |
| Administrator view of document-denied work | With group `read:all`, repository `admin:<identity>`, and document `read:none`, pinned Python `handle_work` allows that administrator to view the document by combining document-read resolution with repository-admin access. Rust now applies the same admin fallback only to `view`; an ignored pinned-Python production-handler differential compares status and response bytes. The regression is included in Verify's pinned-reference test list. | focused local pinned-Python differential verified; other document permissions and work operations remain open |
| Administrator document-read gate for comment/edit/delete/perms | Four production-handler differentials use group `read:all`, repository administrator access, and a document `read:none` sidecar. They grant `interact` for comment (with group `write:none`) and write/interact for edit and delete. For `perms`, repository admin is the operation-specific right and necessarily also satisfies the shared admin fallback, so those two gates cannot be varied independently. Pinned Python and Rust match response bytes and side effects for edit/delete/perms. Comment exposed a mismatch: Python's pre-operation `document-read OR repository-admin` gate allows the comment with interact access, while Rust incorrectly required document read or write. Rust now includes repository-admin fallback in comment's read predicate while retaining the interact requirement. | four focused local pinned-Python production-handler differentials verified against Reticulum `99de23c040d507e3fefca19e87b182302902725d`; remaining permission/operation combinations and broad #612 acceptance stay open |
| Document read gate precedes work mutations | The focused pinned-Python production `handle_work` differential grants the document author write and interact on item 7 while its `.allowed` sidecar explicitly denies read. Python returns `NOT_FOUND / Document not found` before edit dispatch and leaves content unchanged; Rust previously permitted the signed edit. The Rust dispatcher now applies the shared document-read-or-repository-admin gate before view/comment/edit/delete/perms dispatch. | focused local production-handler differential verified against Reticulum `99de23c040d507e3fefca19e87b182302902725d`; only edit with valid ID and this permission combination is covered; other gate combinations and live-Link version remain unverified |
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

### Concurrent mutation guarantee (acceptance row 3)

The pinned source is Reticulum revision
`99de23c040d507e3fefca19e87b182302902725d` (`RNS._version.__version__ ==
"1.5.4"`). In `RNS/Link.py`, both a packet-sized request and a completed
request Resource call `threading.Thread(...).start()` for
`handle_request`. That method invokes the destination's response handler
directly on the per-request thread. In `RNS/Utilities/rngit/server.py`,
`handle_work` routes into work operations without taking a work/storage lock;
the `Lock` instances on `ReticulumGitNode` are scoped to active links, stats,
sync, and permissions, not work documents. ID selection is scan-then-use and
document persistence uses the deterministic sibling name `<path>.tmp` followed
by `os.rename`. Consequently, the reference does not promise serialized or
transactional concurrent work updates. `os.rename` protects a completed
replacement from exposing a partially written destination in the non-collision
case; it does not make the scan/write sequence atomic or prevent collisions on
the shared temporary path. Concurrent creates/edits/comments can therefore
race; no collision-safe multi-writer contract is defined. This describes the
reference write path without asserting atomic-rename behavior on every
filesystem or platform.

Rust's network adapter takes `runtime.node.lock()` before
`handle_request_with_peer_identity` and retains it through the handler call,
so Git/work requests to one Rust server process are serialized. Creation also
uses exclusive directory creation and retries a colliding numeric ID; the
focused local test runs eight concurrent cloned nodes against one root and
checks eight persisted document roots. The pinned-Python process test covers
four concurrent clients creating against this single Rust process, not
concurrent writes to one existing document. This stronger Rust single-process
ordering is safe and does not assert a Python guarantee. A shared work root
written by multiple server processes has no cross-process lock or transaction
contract in either implementation and is unsupported/undefined. No behavior or
test was added for speculative cross-process coordination.

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
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --bin rngit \
  permission_handler_revocation_and_blocked_identity_match_pinned_python \
  -- --ignored --exact --nocapture                                  PASS (permission revocation refreshes immediately; blocked identity denied)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_python_interop \
  -- --ignored --nocapture --test-threads=1                      PASS (2 tests: page/media and work restart)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum-99de23c LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --bin rngit \
  configured_group_access_merges_with_sidecar_like_pinned_python \
  -- --ignored --nocapture                                           PASS (configured read, sidecar admin, and admin read inheritance match)
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --bin rngit \
  blocked_work_handler_response_matches_pinned_python \
  -- --ignored --nocapture                                          PASS (blocked identity denied by production work handler despite read:all)
```

Permission-focused rerun on 2026-09-24 used the existing PR #639 worktree and
the pinned Reticulum checkout at
`/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0`:

```text
cargo test -p rns-tools --bin rngit --all-features permission_handler_revocation_and_blocked_identity_match_pinned_python -- --ignored --nocapture --test-threads=1 PASS
cargo test -p rns-tools --bin rngit --all-features configured_group_access_merges_with_sidecar_like_pinned_python -- --ignored --nocapture --test-threads=1 PASS
cargo test -p rns-tools --bin rngit --all-features group_permission_refresh_preserves_configured_access_like_pinned_python -- --ignored --nocapture --test-threads=1 PASS
cargo test -p rns-tools --bin rngit --all-features blocked_work_handler_response_matches_pinned_python -- --ignored --nocapture --test-threads=1 PASS
cargo test -p rns-tools --bin rngit --all-features repository_admin_ -- --ignored --nocapture --test-threads=1 PASS (5 production-handler document-operation differentials)
cargo test -p rns-tools --bin rngit --all-features permission_resolution_obeys_repository_group_and_admin_fallbacks -- --nocapture PASS
cargo test -p rns-tools --bin rngit --all-features repository_permission_set_takes_effect_before_handler_returns_without_restart -- --nocapture PASS
```

The edit request-shape differential ran on 2026-09-24 in the existing PR #639
worktree against the pinned Python checkout above:

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --bin rngit --all-features work_edit_missing_document_id_matches_pinned_python_error -- --ignored --nocapture --test-threads=1 PASS
```

These runs verify the existing focused permission evidence; they did not expose
a new mismatch. The broad group/repository/document permission criterion remains
open because these cases do not exhaust the authorization combinations.

The Verify workflow has a dedicated step for ignored permission differentials.
It verifies that `Reticulum-parity` HEAD equals
`PYTHON_RETICULUM_PARITY_REF` (`99de23c040d507e3fefca19e87b182302902725d`)
before running the exact `rngit` binary test names. The revocation/block
differential passes locally against that pinned revision; hosted results for
the updated PR head are pending.

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

Two bounded #612 follow-ups are now included in this PR. The ignored production
handler differential `work_list_unknown_scope_matches_pinned_python_empty_result`
compares the exact response status and MessagePack body against pinned RNS
1.5.4; an unknown `list` scope returns `RES_OK` with empty `active`,
`completed`, and `proposed` arrays in both implementations. It passed with
`RETICULUM_PY_REPO` at `99de23c040d507e3fefca19e87b182302902725d` and
`TMPDIR=/dev/shm`. The local production-handler regression
`document_admin_can_complete_work_through_production_handler` creates work
under repository work rights, applies a document-admin sidecar, and verifies
the admin can complete it and move the document to `completed/`; this is
unit/source evidence, not a Python differential. Both tests cover narrow
request/permission seams only, so the broad #612 operation and permission
criteria remain open.

The ignored pinned-Python differential
`work_complete_denial_and_request_errors_match_pinned_python_without_mutation`
drives both implementations' production `handle_work` paths for three cases:
valid-ID completion denied by missing repository write permission, missing
`doc_id`, and malformed `doc_id`. It compares exact response status/body and
active/completed directory state. All three cases passed against Reticulum
`99de23c040d507e3fefca19e87b182302902725d`. This covers failure behavior for
`complete` only; other operation gates and the broad
#612 request/authorization/scope/metadata/side-effect matrix remain open.

The ignored pinned-Python differential
`work_complete_success_matches_pinned_python_response_and_directory_transition`
drives successful `complete` through both production handlers. Each fixture
grants repository read, write, and interact access and seeds active document 7
with the requesting identity as author. The handlers return byte-identical
status/body, remove `active/7`, and create `completed/7`; the Python reference
is pinned to Reticulum `99de23c040d507e3fefca19e87b182302902725d`. This is one
successful completion case only; the broad #612 operation and permission
matrix remains open.

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
### Document read gate before edit

At PR #639 base `0ce2f13d3e8d09dc300e25830d08637543f7546f`, the focused
differential seeded item 7 with `read:none` plus author-specific `write` and
`interact` grants. Pinned RNS 1.5.4 `ReticulumGitNode.handle_work` returned
status 3 and `Document not found`, without changing the body; Rust's production
work dispatcher returned success and edited the body. Python's shared
document-read-or-repository-admin check therefore precedes operation-specific
edit rights. Rust now performs that shared check before dispatching view,
comment, edit, delete, or permission operations when a valid document ID is
present. The regression compares exact status/body, existence, and persisted
content. This is an internal production-handler differential; it does not
claim a live Link test or exhaustive authorization parity.

```text
RETICULUM_PY_REPO=/tmp/reticulum-rngit-99de23c LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --bin rngit --all-features \
  document_write_access_without_read_matches_pinned_python_edit_gate \
  -- --ignored --nocapture --test-threads=1 PASS (after fix)
RETICULUM_PY_REPO=/tmp/reticulum-rngit-99de23c LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --bin rngit --all-features repository_admin_ \
  -- --ignored --nocapture --test-threads=1 PASS (6 differentials)
cargo fmt --all -- --check PASS
cargo clippy -p rns-tools --bin rngit --all-features --no-deps -- -D warnings PASS
tools/scripts/check-boundaries.sh PASS
tools/scripts/check-module-size.sh PASS
git diff --check PASS
```

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
  implementation's production loader and permission resolver, and compares
  stdout captured from each resolver execution. The pinned Python server uses
  `subprocess.run(..., stdout=PIPE)` without a timeout or output cap; Rust's
  two-second deadline and 64 KiB stdout/stderr bounds are intentional safety
  differences, not exact execution-parity claims. The differential does not
  establish parity for timeouts, oversized output, decoding/exit failures, or
  all permission combinations; those broader acceptance criteria remain open.
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
