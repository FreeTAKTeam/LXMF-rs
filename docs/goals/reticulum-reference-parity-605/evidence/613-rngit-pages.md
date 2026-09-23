# #613 rngit NomadNet pages, media, and link cleanup evidence

Status: **partial / unverified**. This records the bounded implementation and
live pinned-Python trace at candidate commit `e41189c8` on
`codex/issue-605-parity`; it does not claim the
full #613 or #605 acceptance gate. The current issue-specific increment adds a
deterministic failure-injection regression for temporary-directory cleanup
and a pinned-Python cancellation trace for an in-flight `/media` Resource.

## Reference and ownership

- Forward reference: Reticulum `99de23c040d507e3fefca19e87b182302902725d`
  (`1.5.4-dev`), primarily `RNS/Utilities/rngit/pages.py` and
  `RNS/Utilities/rngit/media.py`.
- Rust owner: `crates/apps/rns-tools/src/bin/rngit_parts` through the
  production `rngit` network mode and the existing `ReticulumGitNode` service
  boundary.
- Shared wire owner: `crates/libs/rns-transport` Resource sender/manager. The
  new response option is shared so `/media` can preserve the Python
  `auto_compress=False` contract without weakening other Resource callers.

## Implemented behavior

| Area | Implemented and tested behavior | Status |
| --- | --- | --- |
| NomadNet node | Persistent/seeded identity, `nomadnetwork.node` destination, TCP listen/connect interfaces, periodic announce app data, request-path decoding, page/file dispatch, packet or Resource response selection | local verified; pinned Python live TCP page/media trace evidenced |
| Pages | Index/group/repository/tree/blob/commits/commit/refs/stats/releases/release/work/work-doc paths, `var_*` query fields, ref/path validation, not-found/error rendering, custom static and bounded executable templates, binary-image `/media` markup | local verified; pinned Python live trace covers missing repository, invalid ref, missing blob, and visual/reference rendering remains incomplete |
| Access control | Repository read/stats/release checks, work-document read checks, blocked unidentified-client no-identity template, malformed/denied/missing request paths fail closed | local verified; pinned Python live trace covers a denied repository |
| Media/files | `/media` key and path validation, URL decoding, ref/blob resolution, binary-safe filename metadata, download/artifact/work-doc endpoints, published-release filtering and absent-blob handling | local verified; pinned Python Resource payload/metadata and `/file/download` content/filename trace evidenced; same-Link production differential verifies a valid nested-path Resource control and scalar-False denials for missing key/path, malformed/insufficient/empty path, denied private access, absent blob, and invalid ref |
| WebP conversion | Backend preference and `RNGIT_MEDIA_BACKEND`, argv-only process construction, quality/max-dimension options, 8-second pipeline bound, bounded stderr, output validation, temporary-directory cleanup, raw fallback | local code/tests; pinned Python live `ffmpeg` conversion of a valid PNG returns validated WebP; timeout regression verifies both pipeline children are terminated and reaped; other backends and visual parity remain unverified |
| Resource wire | Explicit outbound compression control, with `/media` responses sent uncompressed and a regression asserting no compressed advertisement | local verified; live Python Resource delivery and metadata evidenced |
| Link-scoped cleanup | Converted-media temp data is retained for an active Link and removed on `Closed`, `Stale`, or missing-link state; graceful teardown and in-flight `/media` Resource cancellation are exercised against pinned Python | deterministic cleanup tests plus ignored `rngit_python_interop::rngit_serves_pages_and_media_to_pinned_python_client` and `rngit_python_interop::rngit_cancels_in_flight_media_resource_on_python_link_teardown` | active, stale, closed, missing-link, graceful-disconnect, and synchronized partial-Resource cancellation paths verified; abrupt-process stale transition and other filesystem failures remain open |
| Removal errors | Failed directory deletion retains the Link registry entry; conversion-fallback and link-cleanup errors log path/link context for diagnosis and retry, even with `--silent` | deterministic `page_link_cleanup_retries_failed_removal` injects a failure then verifies successful retry on link cleanup | one deletion-error retry path verified; other filesystem fault paths remain open |

## Commands and results

The original bounded implementation commands below ran in the isolated
`codex/issue-605-parity` worktree; the #613 cleanup follow-up ran in its own
`corvo/issue-613-rngit-link-cleanup` worktree.

```text
cargo fmt --all -- --check                                      PASS
cargo test -p rns-tools --all-features                          PASS
cargo test -p rns-tools --bin rngit --all-features              PASS (35 tests)
cargo clippy -p rns-tools --all-targets --all-features --no-deps \
  -- -D warnings                                                 PASS
cargo test -p reticulum-rs-transport --lib resource:: --all-features PASS (63 tests)
cargo clippy -p reticulum-rs-transport --lib --all-features \
  --no-deps -- -D warnings                                       PASS
git diff --check                                                 PASS
cargo run -q -p rns-tools --bin rngit -- --root /tmp \
  --identity-seed issue-605 --print-identity --silent             PASS
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rngit_python_interop \
  -- --ignored --nocapture                                     PASS (1 test)
cargo clippy -p rns-tools --test rngit_python_interop --all-features \
  --no-deps -- -D warnings                                     PASS
```

The live test starts the production Rust `rngit` service over a TCP interface,
uses the pinned Python RNS runtime as a NomadNet-compatible client, resolves
the announced `nomadnetwork.node` destination, establishes and identifies a
Reticulum Link, and requests a repository page, missing repository, invalid
ref, missing blob, denied repository, and `/file/download`. The error pages
remain not-found responses without private repository content, and the file
response preserves `README.md` metadata and bytes. The binary media response
arrives as a Python Resource with `name=image.png`, size `8192`, and SHA-256
`f8e920545e99cdc9bbc2650eb8282344e8971a7ff0c397c91355d0fcaf6c61fa`.
The same trace requests a valid PNG with `RNGIT_MEDIA_BACKEND=ffmpeg`; it
returns `name=valid.webp` and a validated `RIFF/WEBP` payload. The invalid
image fixture still follows raw fallback, and both Resource responses use the
explicit `auto_compress=False` boundary. The same pinned client then submits
media requests with a missing key, missing path, and insufficient path
components. Each receives the reference's scalar `False` response, with no
Resource metadata or media bytes. This does not promote visual-rendering
parity.

For the cleanup assertion, the Rust server uses an isolated `TMPDIR`/`TEMP`.
The Python client observes exactly one `rngit-media-*` directory after the
successful WebP response and before calling `link.teardown()`. After the client
process exits, the Rust test polls the isolated temp root and requires it to be
empty. This exercises the production `LinkEvent::Closed` cleanup path rather
than only calling `page_link_closed` directly. The pinned checkout is
`99de23c040d507e3fefca19e87b182302902725d`; the live case passed twice.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools \
  --test rngit_python_interop \
  rngit_serves_pages_and_media_to_pinned_python_client \
  -- --ignored --nocapture                            PASS (prior run)
```

The #613 follow-up validation in `corvo/issue-613-rngit-link-cleanup`:

```text
cargo fmt --all -- --check                                      PASS
cargo test -p rns-tools --bin rngit --all-features              PASS (41 passed, 1 ignored)
cargo test -p rns-tools --bin rngit \
  stale_page_links_are_cleaned_while_active_links_keep_media     PASS
cargo test -p rns-tools --bin rngit --all-features \
  page_link_cleanup_retries_failed_removal                       PASS (1 test)
cargo clippy -p rns-tools --all-targets --all-features --no-deps \
  -- -D warnings                                                 PASS
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 PYTHONPATH=<pinned-checkout> cargo test \
  -p rns-tools --test rngit_python_interop \
  rngit_serves_pages_and_media_to_pinned_python_client \
  -- --ignored --nocapture                                      PASS (1 test)
git diff --check; tools/scripts/check-module-size.sh             PASS
```

The separate media URL acceptance slice starts the production `rngit` binary
and uses the frozen Python RNS client over a real TCP Reticulum Link. It
requests `/media/group/repo/HEAD/assets%2Fspace+name.bin` and verifies the
Resource metadata name `space name.bin`, 29 binary bytes, and SHA-256
`78acd6db2006e4da7531327f95f5b00b97c53d18e59326e90dacdee2cba1e1a7`. A
second request for an absent encoded blob returns scalar `False` without
Resource metadata or media bytes. The pinned
`pages.py` decodes the file-path tail with `urllib.parse.unquote_plus` but does
not decode the group, repository, or ref components; accordingly this case
uses the literal ref `HEAD`. This is a focused acceptance slice only and does
not establish complete `/media` parity or #613 completion.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_media_decodes_encoded_path_and_returns_resource_metadata \
  -- --ignored --nocapture                                      PASS (1 test)
```

A separate pinned-Python differential case exercises `/media` authorization
over a production Rust TCP Link. The fixture contains a committed
`secret.bin` in a repository whose `read:none` policy denies access; the Python
client leaves its Link unidentified and requests that valid blob at
`/media/private/repo/HEAD/secret.bin`. Rust returns scalar `False`, with no
Resource metadata, media bytes, or private canary. This matches the pinned
`pages.py` denial and `Link.py` scalar-response behavior. The focused case does
not by itself complete the `/media` acceptance set or overall #613 acceptance.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_denies_private_media_over_unidentified_python_link \
  -- --ignored --nocapture                                      PASS (1 test)
```

The expanded media-validation differential uses one production Rust service
and one pinned Python TCP Link. It first fetches
`/media/group/repo/main/assets%2Fspace+name.bin` and verifies the Resource name
`space name.bin` and exact bytes
(`70657263656e74206465636f646564206d65646961207061746800ff0a`). On that same
Link it then requests missing key, missing path, insufficient and malformed
paths, an empty file path, a valid blob denied by private-repository policy, an
absent blob, and `no-such-ref-613`. Each denial returns scalar `False` with no
Resource metadata or media bytes; the private case also verifies that the
committed canary does not leak. The cases correspond to the pinned
`pages.py::serve_media` false-return branches, and pinned `Link.py` sends each
non-`None` value as a scalar response. Rust checks object presence before
reading media bytes: failed object-info resolution returns `False`, while a
later `page_blob` read failure remains no-response. That latter failure path
is preserved in code but is not separately fault-injected by this suite. The
whole #613 acceptance remains partial pending other named page/media and
network-workflow conditions.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_media_validation_denials_return_false_over_python_link \
  -- --ignored --nocapture                                      PASS (1 test)
```

The follow-up also reran the updated encoded-path, private-access, and main
pinned-Python page/media interop cases against the exact reference checkout at
`99de23c040d507e3fefca19e87b182302902725d`; all four focused tests passed.

The periodic service sweep now treats `LinkStatus::Stale` the same as `Closed`
and a missing transport link, matching pinned Python `clean_links()`, which
removes tracked links whose status is not `ACTIVE`. A deterministic service
regression gives separate media directories to active, stale, closed, and
missing links; only the active link's directory remains. The ignored
pinned-Python process test runs in Verify CI and covers the real graceful
`LinkEvent::Closed` path. Two abrupt-process-exit traces did not observe cleanup
within 90 or 150 seconds; the latter observed the Rust Link still `Active` after
104 seconds without inbound traffic. This does not prove the transport's
eventual stale transition, so abrupt-exit timing remains explicitly unverified.
The new failure-injection regression proves that a failed `remove_dir_all`
leaves its directory tracked and the subsequent Link cleanup removes it. The
conversion-fallback and link-cleanup handlers log path and Link ID on failure,
including when routine output is silent. A focused Unix regression now holds
both pipeline subprocesses open past a 50 ms test deadline and observes the
production timeout helper terminate and reap each child. This proves bounded
subprocess cleanup only; cancellation of a live Resource response and other
filesystem fault paths remain open.

```text
cargo test -p rns-tools --bin rngit --all-features \
  webp_pipeline_timeout_terminates_and_reaps_both_processes -- --nocapture
  PASS (1 test; both timeout child processes reaped)
```

The module-size script now reports only the existing
`crates/libs/rns-transport/src/resource/manager.rs:555` over-budget baseline;
the changed `rngit` and Resource sender files are within the active 500-line
limit.

## Deliberate remaining gaps

The new cancellation trace uses the pinned Python `Link.request` progress
callback as its synchronization point. The callback holds the client at
observed partial response progress while the main client thread tears down the
real TCP Link; this is not a payload-size or log-timing assumption. The
response completion callback remains uncalled, Python reports no incoming
Resource or Resource storage file after teardown, and the isolated Rust media
temp root is empty. On Linux the test also inspects the live `rngit` process
child list at that point and requires it empty; conversion is synchronous and
has waited/reaped its children before advertising the Resource. The transport
regression `resource_manager_removes_link_scoped_state_on_link_close`
separately asserts that outbound, pending, and inbound Resource maps are empty
after link cleanup. The trace found no Rust behavior mismatch, so the
increment changes tests and parity evidence only.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  cargo test -p rns-tools --test rngit_python_interop \
  rngit_cancels_in_flight_media_resource_on_python_link_teardown \
  -- --ignored --nocapture                                      PASS (1 test)
TMPDIR=$PWD/target/tmp cargo test -p rns-tools --bin rngit --all-features \
  -- --test-threads=1                                          PASS (42 passed, 1 ignored)
TMPDIR=$PWD/target/tmp RETICULUM_PY_REPO=<pinned checkout> \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools \
  --test rngit_python_interop \
  rngit_serves_pages_and_media_to_pinned_python_client \
  -- --ignored --nocapture                                      PASS (1 test)
RETICULUM_PY_REPO=<pinned checkout> cargo clippy -p rns-tools \
  --test rngit_python_interop --all-features --no-deps -- -D warnings PASS
cargo fmt --all -- --check; tools/scripts/check-module-size.sh; \
  git diff --check                                              PASS
cargo test -p reticulum-rs-transport --lib \
  resource_manager_removes_link_scoped_state_on_link_close       PASS (1 test)
cargo test -p rns-tools --bin rngit \
  webp_pipeline_timeout_terminates_and_reaps_both_processes      PASS (1 test)
cargo test -p rns-tools --bin rngit stale_page_links_are_cleaned_while_active_links_keep_media PASS
cargo test -p rns-tools --bin rngit page_link_cleanup_retries_failed_removal PASS (1 test)
```

- The pinned Python/NomadNet-compatible client now completes one real TCP
  Reticulum link, identifies, exercises successful and negative page/file
  requests, and downloads raw media as a Resource with deterministic
  metadata/content checks. It also proves one generated conversion directory
  is present during the active link and gone after graceful client teardown.
  This is one bounded role trace, not proof of every page, file, or
  public-network path. Status-driven stale cleanup and one synchronized
  in-flight Resource cancellation are covered, but abrupt-process stale
  transition, other filesystem-failure paths, and the complete media lifecycle
  remain open. The timeout regression establishes only that both conversion
  subprocesses are terminated and reaped.
- The Rust page rendering is intentionally a compact service implementation;
  full Markdown highlighting, pagination, diff rendering, signed work-document
  presentation, and every reference template detail remain open.
- Only the `ffmpeg` backend has a live encoder-success fixture; the other
  configured backend families, visual-rendering parity, and full image corpus
  remain unverified. Invalid conversion fallback, WebP header validation,
  timeout/cleanup code paths, and argument construction are also covered
  locally.
- Reticulum public-key work-document signature verification,
  restart/concurrent-writer/fault
  transcripts, and end-to-end rngit Git/work network workflows remain open
  under #612/#613.
