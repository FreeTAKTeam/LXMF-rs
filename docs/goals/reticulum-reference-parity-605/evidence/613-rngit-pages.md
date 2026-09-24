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
| Pages | Index/group/repository/tree/blob/commits/commit/refs/stats/releases/release/work/work-doc paths, `var_*` query fields, ref/path validation, not-found/error rendering, custom static and bounded executable templates, binary-image `/media` markup | local verified; pinned Python production-Link regression now checks a nested image path containing a space against frozen `pages.py`'s `urllib.parse.quote_plus(file_path)` output; missing repository, invalid ref, missing blob are covered; visual/reference rendering remains incomplete |
| Access control | Repository read/stats/release checks, work-document read checks, and the frozen `pages.py` rule that renders `no_ident` only for an unidentified peer when the derived null-identity hash is blocked | unit cases cover blocked/unblocked anonymous and identified-blocked behavior; pinned Python real-Link traces cover both the unblocked front page and exact blocked `no_ident` response with private-content exclusion; denied repository trace remains covered |
| Media/files | `/media` key and path validation, URL decoding, ref/blob resolution, binary-safe filename metadata, download/artifact/work-doc endpoints, published-release filtering and absent-blob handling | local verified; pinned Python Resource payload/metadata and `/file/download` content/filename trace evidenced; same-Link production differential verifies a valid nested-path Resource control and scalar-False denials for missing key/path, malformed/insufficient/empty path, denied private access, absent blob, and invalid ref |
| WebP conversion | Backend preference and `RNGIT_MEDIA_BACKEND`, argv-only process construction, quality/max-dimension options, 8-second pipeline bound, bounded stderr and converted-output reads (32 MiB media-response cap), output validation, temporary-directory cleanup, raw fallback | local code/tests; deterministic software differential coverage matches pinned-Python backend selection and configured argv for all five backend families; pinned Python live `ffmpeg` conversion of a valid PNG returns validated WebP; exact-limit/over-limit converted-file regression; pinned-Python production-Link test with an explicitly unavailable backend returns the original `image.png` name and all 8,192 raw bytes unchanged; timeout regression verifies both pipeline children are terminated and reaped; real encoder operation for `magick`, `convert`, `gm`, and `avconv`, plus visual parity, remain unverified |
| Resource wire | Explicit outbound compression control, with `/media` responses sent uncompressed and a regression asserting no compressed advertisement | local verified; pinned Python inspects the production Resource advertisement and confirms no compression for a precompressed PNG |
| Link-scoped cleanup | Converted-media temp data is retained for an active Link and removed on `Closed`, `Stale`, or missing-link state; graceful teardown and in-flight `/media` Resource cancellation are exercised against pinned Python | deterministic cleanup tests plus ignored `rngit_python_interop::rngit_serves_pages_and_media_to_pinned_python_client` and `rngit_python_interop::rngit_cancels_in_flight_media_resource_on_python_link_teardown` | active, stale, closed, missing-link, graceful-disconnect, and synchronized partial-Resource cancellation paths verified; abrupt-process stale transition and other filesystem failures remain open |
| Removal errors | Failed directory deletion retains the Link registry entry; conversion-fallback and link-cleanup errors log path/link context for diagnosis and retry, even with `--silent` | deterministic `page_link_cleanup_retries_failed_removal` injects a failure then verifies successful retry on link cleanup | one deletion-error retry path verified; other filesystem fault paths remain open |

### Automatic WebP backend winner

The pinned `media.py::_selected_backend` caches the last automatically selected
backend in `_winner`, moves it to the front on later automatic selections,
continues using it while it remains available, and falls back to normal
preference order after it disappears. Rust previously restarted preference
order for every conversion. The issue-specific increment now retains the
automatic winner for the process lifetime; an explicit `RNGIT_MEDIA_BACKEND`
override still takes precedence without changing that cached automatic
winner. Rust unit coverage checks first selection, reuse after an earlier
preference becomes available, and fallback when the cached executable becomes
unavailable. An ignored integration regression imports the actual pinned
helper and injects executable availability to assert the same sequence.

```text
cargo test -p rns-tools --bin rngit --all-features automatic_backend_selection
  PASS (2 tests)
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  pinned_python_media_backend_selection_reuses_available_automatic_winner \
  -- --ignored --nocapture
  Requires the pinned local Python checkout; not run in this worktree.
```

This is one backend-selection state-parity increment; encoding by the other
backend families, visual parity, and the broader #613 acceptance remain open.

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

The same production-Link trace now requests the binary image at
`assets/nested image.png`. It checks the exact Micron `/media/` markup against
the pinned `pages.py` `urllib.parse.quote_plus(file_path)` expression and the
Python standard-library result (`assets%2Fnested+image.png`). This confirms
nested-path and space encoding for this case only; it does not establish full
page-rendering parity.

```text
TMPDIR="$PWD/target/tmp" \
RETICULUM_PY_REPO="$PWD/target/tmp/pinned-reticulum" \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_serves_pages_and_media_to_pinned_python_client -- --ignored --exact --nocapture
  PASS (1 test; Reticulum 99de23c040d507e3fefca19e87b182302902725d)
```

The focused precompressed-media regression uses the deterministic, valid 1x1
PNG already committed as `valid.png` in the interop fixture, with server-side
image conversion disabled so the original media reaches the Resource
boundary. The pinned reference registers `PATH_MEDIA` with
`auto_compress=False`; the Rust production handler passes `false` to
`send_response_resource_with_compression`. A real pinned-Python TCP Link
observes the response Resource advertisement before acceptance and confirms
its compressed flag is false, then verifies the `valid.png` metadata, all 68
original bytes, and SHA-256
`431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460`.
This is distinct from the unavailable-backend raw-fallback regression and
proves the emitted Resource is not compressed; it does not assert whether an
internal compression attempt occurred.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  issue_613_media_compression::rngit_media_resource_preserves_precompressed_png_without_resource_compression \
  -- --ignored --nocapture                                             PASS
```

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

Comparing the exact pinned `pages.py::serve_media` path to Rust exposed an
edge-slash difference: Python's `get_blob_info` and `get_blob_stream` each
apply `path.strip("/")` after decoding. Rust had rejected a leading slash in
the decoded file tail. The production-Link URL regression now requests
`/media/group/repo/HEAD//assets%2Fspace+name.bin` and verifies the same binary
Resource filename, size, and SHA-256 as the ordinary path. Rust strips only
leading/trailing slashes before existing path validation; interior empty
components remain invalid. This focused delta does not complete #613.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_media_decodes_encoded_path_and_returns_resource_metadata \
  -- --ignored --nocapture                                      PASS (1 test)
```

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
later `page_blob` read failure remains no-response. A production-TCP-service
fault-injection test now allows `cat-file -s` to resolve the committed object,
then fails `cat-file blob`; the pinned Python Link observes neither a response
nor a failed callback before its request timeout. This matches the frozen
`pages.py::serve_media` `None` return and `Link.py` send behavior. Rust already
matches, so no production behavior change was needed. The Unix-only regression
passed against pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`.
Verify is configured to run this exact ignored regression against its pinned
`Reticulum-parity` checkout.
The whole #613 acceptance remains partial pending other named page/media and
network-workflow conditions.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_media_validation_denials_return_false_over_python_link \
  -- --ignored --nocapture                                      PASS (1 test)
```

```text
TMPDIR=/dev/shm RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  issue_613_media_read_failure::rngit_media_content_read_failure_has_no_python_link_response \
  -- --ignored --exact --nocapture --test-threads=1              PASS (1 test)
```

The follow-up also reran the updated encoded-path, private-access, and main
pinned-Python page/media interop cases against the exact reference checkout at
`99de23c040d507e3fefca19e87b182302902725d`; all four focused tests passed.

The media validation differential now also covers a literal malformed escape
in a repository filename (`literal%zz.bin`). Python's
`urllib.parse.unquote_plus` preserves malformed `%` sequences; the Rust media
decoder now matches that behavior locally, while retaining the existing path
validation and the stricter decoder used by other page routes. The response
matches the Python Link Resource filename metadata and exact binary payload.
This does not remove Rust's bounded 32 MiB media-response limit; the pinned
Python stream path has no corresponding explicit size cap.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  issue_613_media_invalid_ref::rngit_media_validation_denials_return_false_over_python_link \
  -- --ignored --nocapture --test-threads=1                     PASS (1 test)
```

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

The pinned `pages.py::serve_media` decodes only the joined file-path tail with
`urllib.parse.unquote_plus`; group, repository, and ref components are literal.
The Rust parser now preserves those components verbatim and decodes only the
file path. The live differential adds `/media/%67roup/repo/main/...`: Python
treats `%67roup` literally and returns scalar `False` instead of resolving the
`group` repository. This is one focused media-parity increment; #613 remains
partial.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_media_validation_denials_return_false_over_python_link \
  -- --ignored --nocapture                                  PASS (1 test)
```

## Deliberate remaining gaps

### Frozen `no_ident` page behavior

The exact target `99de23c040d507e3fefca19e87b182302902725d` implements this
page guard in `RNS/Utilities/rngit/pages.py`: render `no_ident` only when the
remote identity is absent and `self.null_ident.hash` is in
`owner.blocked_identities`. `RNS.Identity.from_bytes(bytes(64)).hash` at that
target is `d7db22f63b453c23bb0688dde565b7c1`. Therefore an unblocked anonymous
client receives the normal front page, while an identified-but-blocked client
does not receive `no_ident` and is still filtered by normal permission checks.
Rust now follows that condition exactly. Unit regressions cover all three
states, including no repository/document marker in the blocked-anonymous
response; the real-Link Python trace covers the unblocked anonymous case.

Validation against the exact frozen checkout at
`/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0` (HEAD
`99de23c040d507e3fefca19e87b182302902725d`) derived the null hash with
`PYTHONPATH=<checkout> python3 -c 'import RNS; print(RNS.Identity.from_bytes(bytes(64)).hash.hex())'`
and returned `d7db22f63b453c23bb0688dde565b7c1`. The three Rust handler cases
passed: exact-hash blocked anonymous returns the no-identity template without
repository markers; unblocked anonymous receives the front page; identified
blocked receives no no-identity template and no repository content. A real
Python Link against the same frozen checkout confirms the normal unblocked
anonymous front page and visible group are preserved.

A second production-Link differential configures the Rust `rngit` process with
the frozen null-identity hash in its blocked set, then requests the front page
from a pinned Python client that has not identified its Link. The Python side
independently derives and checks the same null-identity hash, and the test
asserts request status `READY`, the exact complete rendered `no_ident` page
including the base template/footer, and absence of seeded private content. The
test fixture commits `private-canary: secret-repository content must not be
disclosed` into the private bare repository, whose repository-level policy is
`read:none`. This tests the process, request dispatch, access gate, and
response framing; the acceptance item remains partial beyond this behavior.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  issue_613_no_ident::rngit_returns_reference_no_ident_page_to_blocked_anonymous_python_link \
  -- --ignored --exact --nocapture --test-threads=1                     PASS (1 test)
```

```text
cargo fmt --all -- --check                                           PASS
cargo test -p rns-tools --bin rngit anonymous -- --nocapture         PASS (2 tests)
cargo test -p rns-tools --bin rngit identified_blocked_client -- --nocapture PASS (1 test)
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_preserves_unblocked_anonymous_front_page_for_python_link \
  -- --ignored --nocapture --test-threads=1                          PASS (1 test)
git diff --check                                                     PASS
```

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
  remain unverified. Invalid conversion fallback, explicitly unavailable
  backend raw fallback over a pinned-Python production Link, WebP header
  validation, timeout/cleanup code paths, and argument construction are also
  covered.

The unavailable-backend case starts the Rust service with
`RNGIT_MEDIA_BACKEND=codex-test-backend-that-does-not-exist` while leaving media
conversion enabled. A client using pinned Reticulum
`99de23c040d507e3fefca19e87b182302902725d` requests the valid-path
`/media/group/repo/HEAD/image.png` Resource and verifies the original filename,
8,192-byte size, and byte-for-byte match against the committed fixture. This
supports only disabled-backend raw fallback; other encoder families and the
broader image corpus remain unverified.
Verify runs this exact ignored regression against its pinned
`Reticulum-parity` checkout.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_returns_raw_media_when_webp_backend_is_unavailable \
  -- --ignored --exact --nocapture                                  PASS (1 test)
```
- Reticulum public-key work-document signature verification,
  restart/concurrent-writer/fault
  transcripts, and end-to-end rngit Git/work network workflows remain open
  under #612/#613.

### `/media` key presence

At frozen Reticulum `99de23c040d507e3fefca19e87b182302902725d`,
`pages.py::serve_media` validates `"key" in data`; it does not validate the
value. The production-Link regression therefore sends a valid media path with
`key: None` and verifies the same Resource filename (`space name.bin`) and
exact binary payload as the ordinary valid-key request. The absent-key control
still returns scalar `False`, without metadata or media bytes. Rust already
matches, so this slice changes only regression coverage and evidence.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_media_validation_denials_return_false_over_python_link \
  -- --ignored --nocapture --test-threads=1                       PASS (1 test)
```

### Media backend selection and configured argv

The pinned helper at Reticulum
`99de23c040d507e3fefca19e87b182302902725d`,
`RNS/Utilities/rngit/media.py::_selected_backend` and
`_configured_backend`, defines the five-family preference order, explicit
`RNGIT_MEDIA_BACKEND` behavior, and backend-specific quality and dimension
arguments. Software-only Rust unit cases inject available programs, so they
cover each automatic preference position and explicit selection without
installing or launching image tools. Exact argv vectors cover quality 85 and a
640-pixel maximum for all five families, with separate checks for clamping and
omitted nonpositive dimensions. Comparison exposed two mismatches: an empty
backend environment value did not follow Python's automatic-selection behavior,
and ffmpeg-family argv used `-q:v` instead of the reference `-quality`; both
were corrected. This establishes argument and selection parity only, not
successful encoding by every backend.

The local environment reports `/usr/bin/ffmpeg`; `magick`, `convert`, `gm`, and
`avconv` are unavailable. The pinned helper source was read from the exact
GitHub commit because its local reference checkout was absent. The installed
ffmpeg help lists `-quality`; this check did not run a conversion.

The same pinned backend selector uses `shutil.which`, which ignores regular
files on `PATH` that lack executable permission. Rust's initial availability
check accepted any regular file, potentially selecting an unusable earlier
backend instead of a later executable one. The check now requires executable
permission on Unix, and a focused filesystem regression covers both modes;
this does not claim live encoding by each backend family.

```text
cargo test -p rns-tools --bin rngit --all-features media_backend_tests -- --nocapture
  PASS (10 passed; includes executable-permission eligibility)
TMPDIR="$PWD/target/tmp" cargo test -p rns-tools --bin rngit --all-features -- --test-threads=1
  PASS (55 passed, 2 ignored)
cargo clippy -p rns-tools --bin rngit --all-features --no-deps -- -D warnings
  PASS
cargo fmt --all -- --check; tools/scripts/check-module-size.sh; git diff --check
  PASS
```

### Exact invalid-reference page rendering over the pinned Link

The production `rngit` page/media integration now retains the complete page
body received by the pinned Python `RNS.Link` and checks the invalid-reference
response against a deterministic rendered fixture. The assertion includes the
navigation links, `Not Found` heading, exact reference error, base template,
version footer, and local generation marker. This closes the prior gap where
the live trace checked only that an error marker appeared. The focused run used
Reticulum `99de23c040d507e3fefca19e87b182302902725d` and passed both matching
integration-test declarations; no production behavior change was needed.

```text
TMPDIR=$PWD/target/tmp \
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools \
  --test rngit_python_interop \
  rngit_serves_pages_and_media_to_pinned_python_client \
  -- --ignored --nocapture --test-threads=1                       PASS (2 tests)
```

### Encoded dot-segment path denial over the pinned Link

The production Rust `rngit` service was queried by the pinned Python Link with
`/media/group/repo/HEAD/assets%2F..%2FREADME.md`. The pinned
`pages.py::serve_media` decodes only the file tail with `unquote_plus`, then
passes `assets/../README.md` to `get_blob_info`; the reference Git object lookup
returns no blob. Rust denies the decoded dot segment before blob lookup. The
Link receives scalar `False`, with no Resource metadata or media bytes. This
adds one traversal-shaped validation case and does not establish complete
path-validation parity or close #613.

```text
TMPDIR=/dev/shm \
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs/.tmp/python-refs/Reticulum-99de23c \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_media_denies_percent_decoded_dot_segment_over_python_link \
  -- --ignored --nocapture                                      PASS (1 test)
```
