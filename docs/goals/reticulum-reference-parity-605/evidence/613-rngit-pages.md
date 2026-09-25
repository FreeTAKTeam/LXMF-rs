# #613 rngit NomadNet pages, media, and link cleanup evidence

Status: **partial / unverified**. This records the bounded implementation and
live pinned-Python trace at candidate commit `e41189c8` on
`codex/issue-605-parity`; it does not claim the
full #613 or #605 acceptance gate. The current issue-specific increment adds
deterministic failure-injection regressions for temporary-directory and
pipeline-child cleanup, plus a pinned-Python cancellation trace for an
in-flight `/media` Resource. A
matched abrupt-client-exit comparison also found and fixed a narrower Rust
lifecycle difference: the pinned Python server cleans on disconnect, while
Rust kept the page Link active after a failed Resource response.

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
| Pages | Index/group/repository/tree/blob/commits/commit/refs/stats/releases/release/work/work-doc paths, `var_*` query fields, ref/path validation, not-found/error rendering, custom static and bounded executable templates, binary-image `/media` markup | local verified; pinned Python production-Link regression checks nested file-path encoding; unit regression confirms that only `file_path` receives `quote_plus`, while group/repository/ref remain literal as in frozen `pages.py`; missing repository, invalid ref, missing blob are covered; visual/reference rendering remains incomplete |
| Access control | Repository read/stats/release checks, work-document read checks, and the frozen `pages.py` rule that renders `no_ident` only for an unidentified peer when the derived null-identity hash is blocked | unit cases cover blocked/unblocked anonymous and identified-blocked behavior; pinned Python real-Link traces cover both the unblocked front page and exact blocked `no_ident` response with private-content exclusion; denied repository trace remains covered |
| Media/files | `/media` key and path validation, URL decoding, ref/blob resolution, binary-safe filename metadata, download/artifact/work-doc endpoints, published-release filtering and absent-blob handling | local verified; pinned Python Resource payload/metadata and `/file/download` content/filename trace evidenced; same-Link production differential verifies a valid nested-path Resource control and scalar-False denials for missing key/path, malformed/insufficient/empty path, denied private access, absent blob, and invalid ref |
| WebP conversion | Backend preference and `RNGIT_MEDIA_BACKEND`, argv-only process construction, quality/max-dimension options, 8-second pipeline bound, bounded stderr and streaming encoder-output capture (32 MiB disk cap), output validation, temporary-directory cleanup, raw fallback | Existing production-Link encoding and exact raw-fallback regressions remain; the deterministic oversized fake-backend case checks prompt termination, reaping of both pipeline children, and removal of partial output. Pinned-Python Link regressions prove raw fallback when the forced backend is unavailable and when media temp-directory creation fails because `TMPDIR` resolves to a regular file. A pinned-Python Link regression also forces recognized-but-unavailable `magick` while an `ffmpeg` sentinel is available, proving no encoder fallthrough; local validation passed at PR commit `e7563fca`, and Verify runs that regression. ImageMagick 7 `magick` now also passes locally through the checksum-pinned AppImage and pinned-Python production-Link fixture; `avconv` and visual parity remain unverified. |
| Resource wire | Explicit outbound compression control, with `/media` responses sent uncompressed and a regression asserting no compressed advertisement | local verified; pinned Python inspects the production Resource advertisement and confirms no compression for a precompressed PNG |
| Link-scoped cleanup | Converted-media temp data is retained for an active Link and removed on `Closed`, `Stale`, or missing-link state; graceful teardown, response-send-detected abrupt client exit, periodic cleanup after silent Link disappearance, in-flight `/media` Resource cancellation, pipeline child-status errors, and stale-sweep removal retry are exercised | deterministic `periodic_sweep_removes_media_for_silently_disappeared_link` test plus ignored `rngit_python_interop::rngit_serves_pages_and_media_to_pinned_python_client`, `rngit_python_interop::rngit_cancels_in_flight_media_resource_on_python_link_teardown`, `rngit_python_interop::rngit_cleans_media_after_response_fails_on_abrupt_client_exit`, and `rngit_python_interop::issue_613_cleanup_isolation::rngit_disconnect_cleanup_preserves_an_independent_active_media_response` | active, stale, closed, missing-link, graceful-disconnect, synchronized partial-Resource cancellation, abrupt client exit detected by a failed response, periodic sweep after silent Link disappearance, cross-Link cleanup/active-response isolation, child-status-error termination/reaping, and one stale-sweep filesystem removal failure/retry verified; other filesystem/media lifecycle fault paths remain open |
| Removal errors | Failed directory deletion retains the Link registry entry; conversion-fallback and link-cleanup errors log path/link context for diagnosis and retry, even with `--silent` | `page_link_cleanup_retries_failed_removal` injects a failure then verifies retry on close; `stale_page_link_sweep_retries_filesystem_removal_failure` induces a real filesystem error in the stale-sweep helper, confirms tracking is retained, then verifies a later retry removes the restored directory | explicit close-path retry and one stale-sweep helper retry verified; other filesystem fault paths remain open |

### Stale-link sweep retries a filesystem removal failure

`stale_page_link_sweep_retries_filesystem_removal_failure` starts with a
tracked media directory, replaces its path with a regular file, and runs
`clean_stale_page_links`. The real `remove_dir_all` failure is reported and the
path remains registered. The test then replaces the file with a directory and
marker, retries the stale sweep, and verifies the directory and Link registry
entry are removed. This exercises the cleanup helper called by the periodic
stale-link path; it does not exercise the timer itself or cover all filesystem
failure modes.

```text
cargo test -p rns-tools --bin rngit --all-features \
  stale_page_link_sweep_retries_filesystem_removal_failure -- --nocapture PASS (1 test)
```

### Media temp-directory creation failure falls back to the original Resource

The ignored production-Link regression
`issue_613_temp_directory_failure::rngit_media_temp_directory_creation_failure_returns_raw_resource`
sets the Rust service's `TMPDIR` to a regular file, so `next_media_directory`
fails at the real `create_dir` call during `/media` conversion setup. The
pinned Python Reticulum client still receives the original `image.png` Resource
with its expected filename, 8192-byte size, and SHA-256. This confirms existing
raw fallback behavior; no production change was needed. It covers temp-path
creation only, not output-file creation/write, stat/open races, or Resource
stream-open failure.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  issue_613_temp_directory_failure::rngit_media_temp_directory_creation_failure_returns_raw_resource \
  -- --ignored --exact --nocapture --test-threads=1 PASS (1 test)
```

### Disconnect cancels an in-flight conversion

The production page request now polls the requesting Link while the WebP
pipeline runs. When the Link is no longer active, the cancellation path is
treated as a conversion failure, both live child processes are terminated and
reaped, and the failed converted output is removed before raw fallback. The
deterministic Unix test
`webp_pipeline_disconnect_cancellation_terminates_and_reaps_children` passed
with the sibling status-error and timeout process tests (3 tests total). This
is process-supervisor evidence; by itself it does not establish active-response
isolation or the complete Link/temp-directory lifecycle.

### Oversized encoder output is bounded during capture

Encoder stdout is now piped through a capture worker that writes no more than
32 MiB to the temporary WebP file and probes at most one additional byte to
distinguish exact-limit output from overflow. Overflow signals the existing
pipeline supervisor, which terminates and reaps both children; the partial
output is removed and the caller receives conversion failure for its existing
raw-fallback path. The deterministic capture test feeds 32 MiB plus one byte
and verifies the file remains exactly 32 MiB. A fake `ffmpeg` process regression
emits the same oversized stream while the input process remains live; it checks
prompt return, both child processes reaped, and partial-output removal.
The separate production page-handler regression continues to verify byte-exact
raw fallback and Link-scoped temporary-directory cleanup after conversion
failure. Both new regressions, the existing process timeout/configuration test,
and the production raw-fallback test passed locally with:

```text
cargo test -p rns-tools --bin rngit --all-features bounded_capture_never_writes_the_overflow_byte -- --nocapture --test-threads=1
cargo test -p rns-tools --bin rngit --all-features oversized_fake_backend_is_bounded_terminated_and_cleaned_up -- --nocapture --test-threads=1
cargo test -p rns-tools --bin rngit --all-features converter_process_boundary_honors_configuration_output_and_failures -- --nocapture --test-threads=1
cargo test -p rns-tools --bin rngit --all-features media_conversion_failure_falls_back_to_raw_and_link_cleanup_removes_temp_files -- --nocapture --test-threads=1
```

Formatting, scoped Clippy, module-size, and diff checks also passed. These local
checks do not change the #613 acceptance checklist or issue status.

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
TMPDIR=/dev/shm RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  issue_613_media_compression -- --ignored --nocapture --test-threads=1 \
  PASS (2 tests, including the pinned-helper backend-selection regression)
```

This is one backend-selection state-parity increment; encoding by the other
backend families, visual parity, and the broader #613 acceptance remain open.

### Runtime WebP CLI configuration reaches the selected backend

An ignored production-Link regression now starts `rngit` with the explicit
`ffmpeg` backend and non-default `--media-quality 37` and
`--media-max-dimension 321` settings. A deterministic executable stub records
its argv and emits a minimal valid WebP; a client using pinned Reticulum
`99de23c040d507e3fefca19e87b182302902725d` verifies the converted Resource
metadata, while the test verifies the selected executable received both
configured options and the expected scale filter. This covers runtime wiring
from CLI configuration through backend selection and serving, beyond the
existing argv-construction unit cases. No production behavior change was
needed; the broader #613 acceptance remains open.

```text
TMPDIR="$PWD/target/tmp" RETICULUM_PY_REPO="$PWD/target/tmp/pinned-reticulum" \
  cargo test -p rns-tools --test rngit_python_interop \
  rngit_passes_media_cli_options_to_the_selected_webp_backend \
  -- --ignored --nocapture --test-threads=1                    PASS (1 test)
```

### Configured real backend through the production page service

The installed `ffmpeg` backend is explicitly selected with
`RNGIT_MEDIA_BACKEND=ffmpeg` and exercised through the production `/media`
page handler and a Python Link using the exact pinned Reticulum source. The
test prepends a failing `magick` executable (the earlier automatic preference)
to the server's `PATH` and confirms it is not invoked, proving the configured
backend wins. The regression saves the returned Resource bytes and verifies
them with `ffprobe` as a 1x1 WebP image, observes the conversion directory while
the Link is active, and verifies Link teardown removes it. A second request
uses deliberately invalid PNG bytes and checks that the successful Link
response preserves the original `image.png` metadata, length, and SHA-256
rather than presenting conversion failure as a failed Resource. This
establishes real `ffmpeg` selection and binary validity, not visual equivalence
or parity for every encoder family. Hosted Verify coverage for real ImageMagick
`convert` and GraphicsMagick `gm` is recorded below.

Existing bounded-process regressions separately cover timeout and nonzero
child-status handling, including terminating and reaping pipeline children;
the injected encoder-failure regression checks raw fallback, conversion
directory cleanup, and diagnostic detail. Hosted Verify also exercised real
ImageMagick `convert` and GraphicsMagick `gm` over pinned-Python production
Links at that hosted revision. ImageMagick 7 `magick`, `avconv`,
visual/reference parity, and other filesystem/media lifecycle fault paths were
not covered by that run.

### Hosted real ImageMagick and GraphicsMagick backends

At PR #633 head `317cc142dde34ebcdf30a3c007f7d5a5568557aa`, Verify run
`36024299709` passed the fail-closed production-Link step for both ImageMagick
`convert` and GraphicsMagick `gm`. Each real executable received the configured
quality and resize arguments; the pinned Python client decoded the returned
8x4-to-1x1 WebP and checked response metadata, raw fallback for invalid image
data, and Link-scoped cleanup. The same run passed the complete hosted check
suite. This section records those two installed command families at that
historical head; later ImageMagick 7 evidence follows.

### Pinned ImageMagick 7 AppImage production-Link validation

The official ImageMagick 7.1.2-31 GCC x86_64 AppImage was downloaded from the
[official release](https://github.com/ImageMagick/ImageMagick/releases/tag/7.1.2-31)
and matched published SHA-256
`b22dee096a68e7eb6771a6f98c16490ea78d399ffa27f34ca2ec4f02e707fd9c`. The
SquashFS payload was extracted at the AppImage-reported offset and invoked
through its bundled `AppRun`. The runtime smoke encoded an 8x4 PNG to WebP,
decoded it back to PNG, and `ffprobe` reported `codec_name=webp`, width 8,
height 4; ImageMagick then reported the decoded dimensions as 8x4.

The existing configured-backend integration fixture passed against the pinned
Reticulum Python client with `RNGIT_TEST_WEBP_BACKEND=magick`. It verifies the
production Link response, configured argv, decoded/resized WebP metadata,
raw-media fallback, and cleanup. The current Verify workflow now pins and
checks the same AppImage, repeats the codec smoke, and runs this Link fixture;
that hosted run is pending and is not claimed as passed here. Real `avconv`
encoding and visual/reference rendering parity remain unverified.

### Explicit avconv selection through the production Link path (stubbed encoder)

No `avconv` executable or locally available `libav-tools` package is present
in this environment, so no external binary was installed. A deterministic
executable named `avconv` instead exercises the production `rngit` conversion
path under `RNGIT_MEDIA_BACKEND=avconv`. It records each received argument as
NUL-delimited data, emits a minimal WebP-shaped byte sequence, and the pinned
Python client verifies the resulting `/media` Resource name and signature.
The regression compares the entire argv vector, including the quoted scale
filter containing shell-significant quotes and `>`, at quality 37 and maximum
dimension 321. This proves Rust's selected-backend process wiring and argument
boundaries for the tested request; it does not execute libav/avconv, validate
real encoding/decoding, or establish visual parity. The acceptance checkbox
therefore remains incomplete. Verify runs the exact ignored process test.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  issue_613_media_options::rngit_invokes_selected_avconv_backend_with_argument_safe_options \
  -- --ignored --exact --nocapture --test-threads=1                    PASS (1 test)
```

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
LXMF_PYTHON_BIN=python3 RNGIT_TEST_WEBP_BACKEND=magick \
  cargo test -p rns-tools --test rngit_python_interop \
  issue_613_configured_backend::configured_backend_serves_decodable_webp_and_falls_back_to_raw_media \
  -- --ignored --exact --nocapture --test-threads=1                    PASS (1 test)
```

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  configured_ffmpeg_serves_decodable_webp_and_falls_back_to_raw_media \
  -- --ignored --nocapture --test-threads=1                    PASS (1 test)
```

## Commands and results

### Image markup quotes only the file path

Inspection of pinned `pages.py` at Reticulum
`99de23c040d507e3fefca19e87b182302902725d` shows the image markup is built as
`/media/{group_name}/{repo_name}/{ref}/{quote_plus(file_path)}`: the first three
fields are interpolated literally, and only the file path is URL-encoded. Rust
previously encoded all four fields. The focused unit regression now verifies a
`+` in the group, a space in the repository, a slash in the ref, and the
reference `quote_plus` result for a nested file path. The implementation now
preserves those first three fields and continues encoding only the file path.
This establishes markup-construction parity for the tested characters, not
full page rendering or successful media retrieval for unusual names.

A second exact fixture covers pinned `urllib.parse.quote_plus` behavior for
`images/café+50% #1.png`: UTF-8 bytes become `%C3%A9`, literal `+`, `%`, and
`#` are escaped, and the space becomes `+`. The Rust result matches the frozen
Python fixture, so this increment found no production mismatch. Verify runs the
focused unit test; this does not establish end-to-end retrieval for unusual
filenames or visual parity.

```text
cargo test -p rns-tools --bin rngit --all-features \
  tests::image_markup_matches_pinned_python_quote_plus_for_utf8_and_reserved_path_bytes \
  -- --exact  PASS (1 test)
```

```text
cargo test -p rns-tools --bin rngit --all-features \
  image_markup_quotes_only_the_file_path_like_pinned_python  PASS (1 test)
```

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

The in-process `pages_accept_nomadnet_var_fields_and_render_not_found_errors`
regression now independently asserts the exact image markup for
`assets/nested image.png`, including `%2F` path encoding and `+` space encoding.
That test passed locally; the pinned production-Link page/media and automatic
backend-winner regressions also passed against Reticulum
`99de23c040d507e3fefca19e87b182302902725d`. No conversion or markup behavior
mismatch was confirmed, so this increment adds coverage only.

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

The new cross-Link regression uses the production `rngit` handler and frozen
Python client: while Link B is paused at partial progress for a 32 KiB raw-media
Resource, Link A disconnects after receiving a converted WebP. The test waits
for Link A's only temporary directory to disappear before resuming Link B, then
checks the complete response length and SHA-256. This proves one Link's cleanup
does not remove another Link's data or corrupt that in-flight response; it does
not close the compound #613 cleanup criterion.

```text
TMPDIR=<writable temporary directory> \
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  issue_613_cleanup_isolation::rngit_disconnect_cleanup_preserves_an_independent_active_media_response \
  -- --ignored --exact                                        PASS (1 test)
```

The periodic service sweep now treats `LinkStatus::Stale` the same as `Closed`
and a missing transport link, matching pinned Python `clean_links()`, which
removes tracked links whose status is not `ACTIVE`. A deterministic service
regression gives separate media directories to active, stale, closed, and
missing links; only the active link's directory remains. The ignored
pinned-Python process test runs in Verify CI and covers the real graceful
`LinkEvent::Closed` path. Earlier abrupt-process traces did not observe cleanup
within 90 or 150 seconds; the latter observed the Rust Link still `Active` after
104 seconds without inbound traffic. A later matched comparison isolated the
cause under the response-send path: a pinned Python client called `os._exit(0)`
after observing the active media directory, and pinned Python `remote_disconnected`
cleaned it; under the same local TCP Link/process-exit pattern, Rust attempted
the media Resource after client exit, got `ConnectionError`, logged the failed
send, and still had an `ACTIVE` Link at the 60-second sweep (the directory
remained beyond 70 seconds). This establishes a concrete lifecycle difference,
not a claim about eventual transport staleness for a silent peer. Rust now maps
that Resource `ConnectionError` to `NotConnected` and closes the corresponding
page Link, publishing the existing `LinkEvent::Closed` cleanup event. The
regression gates fake conversion on a parent-controlled marker, releases it
only after the Python client exits, and asserts directory removal; it failed
before the fix and passes after it. Commit `21bc314e` also adds a deterministic
test of the production page-service loop: a Link disappears without a close
event or failed response, then the scheduled 60-second cleanup sweep removes
its media directory and stale registry entry. This proves the periodic fallback
path; real transport timing for a silently exited remote process remains an
environment-dependent lifecycle case.
The new failure-injection regression proves that a failed `remove_dir_all`
leaves its directory tracked and the subsequent Link cleanup removes it. The
conversion-fallback and link-cleanup handlers log path and Link ID on failure,
including when routine output is silent. A focused Unix regression now holds
both pipeline subprocesses open past a 50 ms test deadline and observes the
production timeout helper terminate and reap each child. This proves bounded
subprocess cleanup only; cancellation of a live Resource response and other
filesystem fault paths remain open.

The conversion supervisor also previously broke out when either child's
`try_wait()` returned an error, then joined stderr readers while leaving both
children alive. A child retaining its stderr pipe could block that join beyond
the conversion deadline and prevent fallback/temp cleanup. The new Unix
failure-injection regression starts both children, injects the status error on
the first poll, and proves the supervisor kills and reaps both before joining
their readers. This is local process-supervision evidence; it does not claim
that disconnect cancels an in-progress conversion, or cover every OS-level
failure. The pinned helper's `_await` explicitly handles timeout but does not
terminate children for arbitrary wait exceptions, so this is bounded Rust
failure cleanup rather than a claimed Python parity difference.

```text
TMPDIR=/dev/shm cargo test -p rns-tools --bin rngit --all-features \
  webp_pipeline_status_error_terminates_and_reaps_both_processes -- --nocapture
  PASS (1 test; injected status error terminates and reaps both children)
```

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

```text
TMPDIR=/dev/shm RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 \
cargo test -p rns-tools --test rngit_python_interop \
  rngit_cleans_media_after_response_fails_on_abrupt_client_exit \
  -- --ignored --nocapture                                  PASS (2 runs)
```

- The pinned Python/NomadNet-compatible client now completes one real TCP
  Reticulum link, identifies, exercises successful and negative page/file
  requests, and downloads raw media as a Resource with deterministic
  metadata/content checks. It also proves one generated conversion directory
  is present during the active link and gone after graceful client teardown.
  This is one bounded role trace, not proof of every page, file, or
  public-network path. Status-driven stale cleanup, the periodic sweep for a
  silently disappeared Link, synchronized in-flight Resource cancellation, and
  response-send-detected cleanup after abrupt client exit are covered; live
  transport timing for an actual silently exited remote process, other
  filesystem-failure paths, and the complete media lifecycle remain open. The
  timeout regression establishes only that both conversion subprocesses are
  terminated and reaped.
- The Rust page rendering is intentionally a compact service implementation;
  full Markdown highlighting, pagination, diff rendering, signed work-document
  presentation, and every reference template detail remain open.
- Live encoder-success fixtures cover `ffmpeg`, ImageMagick `convert`,
  GraphicsMagick `gm`, and ImageMagick 7 `magick` via its checksum-pinned
  AppImage; `avconv`, visual-rendering parity, and the full image corpus remain
  unverified. Invalid conversion
  fallback, unknown-backend raw fallback over a pinned-Python production Link, WebP header
  validation, timeout/cleanup code paths, and argument construction are also
  covered.

The unavailable-backend case starts the Rust service with
`RNGIT_MEDIA_BACKEND=codex-test-backend-that-does-not-exist` while leaving media
conversion enabled. A client using pinned Reticulum
`99de23c040d507e3fefca19e87b182302902725d` requests the valid-path
`/media/group/repo/HEAD/image.png` Resource and verifies the original filename,
8,192-byte size, and byte-for-byte match against the committed fixture. This
supports only disabled-backend raw fallback; successful encoding is evidenced
separately for `ffmpeg`, `convert`, and `gm`, while `magick`, `avconv`, and the
broader image corpus remain unverified.
Verify runs this exact ignored regression against its pinned
`Reticulum-parity` checkout.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_returns_raw_media_when_webp_backend_is_unavailable \
  -- --ignored --exact --nocapture                                  PASS (1 test)
```

### Recognized but unavailable forced backend does not fall through

The pinned helper treats a recognized `RNGIT_MEDIA_BACKEND` override whose
executable is unavailable as no backend; it must not silently select another
installed encoder. The new Unix-only production-Link regression gives the
Rust service an isolated `PATH` containing a `git` symlink and an executable
`ffmpeg` sentinel, but no `magick`, then forces `RNGIT_MEDIA_BACKEND=magick`.
The pinned Python client receives the original `image.png` name and all 8,192
fixture bytes, and the test verifies that the alternate encoder sentinel was
never invoked. This checks the unavailable recognized-override branch, distinct
from the unknown-backend raw-fallback case above; it does not claim successful
encoding by `magick` or `avconv`.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs/.tmp/python-refs/Reticulum-99de23c \
  LXMF_PYTHON_BIN=python3 PYTHONPATH=/home/pgiuseppe/Documents/LXMF-rs/.tmp/python-refs/Reticulum-99de23c \
  cargo test -p rns-tools --test rngit_python_interop \
  rngit_does_not_fall_through_from_unavailable_forced_backend \
  -- --ignored --exact --nocapture --test-threads=1                 PASS (1 test)
```
The same exact test is invoked by `.github/workflows/verify.yml`.

### Configured WebP encoder failure fallback and cleanup

At pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`,
`media.py::convert_to_webp` returns failure when its configured encoder exits
nonzero; the page handler then serves the original media. The production
`rngit` test injects a deterministic `ffmpeg` stub that exits 23 with a known
stderr diagnostic, requests the image through a pinned-Python Link, and
verifies the original 68-byte PNG (`name=valid.png`, SHA-256
`431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460`). It
also checks that the backend diagnostic is retained and the failed conversion
directory is removed. This is one failure/fallback case for the `ffmpeg`
selection only; real production-Link conversion is separately evidenced for
`ffmpeg`, `convert`, and `gm`; `magick`, `avconv`, visual parity, and broader
page/media acceptance remain open.

```text
TMPDIR=/dev/shm RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rngit_python_interop \
  rngit_failed_webp_encoder_returns_raw_media_and_cleans_conversion_directory \
  -- --ignored --nocapture --test-threads=1                       PASS (1 test)
```

### Verify coverage for media argv forwarding and encoder-failure cleanup

At PR #633 base head `5b66d3b43c33fd18558017b70a6ab1e4975fce9a`, both
existing Unix process tests passed locally against Reticulum
`99de23c040d507e3fefca19e87b182302902725d`. The argv test starts the production
`rngit` process with configured quality and maximum dimensions, then a pinned
Python Link confirms the selected stub backend received those exact argument
pairs. The failure test injects a nonzero encoder exit and checks byte-exact raw
PNG fallback, filename metadata, diagnostic retention, and removal of the
conversion directory. `.github/workflows/verify.yml` now invokes each test
with its exact test filter, `--ignored --exact`, and the pinned
`Reticulum-parity` checkout. Local exact-filter runs passed; hosted Verify for
this workflow change is pending. These two software cases do not establish the
remaining #613 lifecycle, filesystem, backend-family, or rendering acceptance.

```text
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 PYTHONPATH=<same checkout> \
cargo test -p rns-tools --test rngit_python_interop \
  issue_613_media_options::rngit_passes_media_cli_options_to_the_selected_webp_backend \
  -- --ignored --exact --nocapture --test-threads=1                  PASS (1 test)

RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 PYTHONPATH=<same checkout> \
cargo test -p rns-tools --test rngit_python_interop \
  issue_613_media_options::rngit_failed_webp_encoder_returns_raw_media_and_cleans_conversion_directory \
  -- --ignored --exact --nocapture --test-threads=1                  PASS (1 test)
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

An additional ignored Rust differential imports the pinned Python helper,
injects each backend as available, calls `_configured_backend(quality=85,
max_dimension=640)`, and compares its returned name and complete argv against
Rust's configured vector for all five backend families. This verifies actual
helper output rather than relying only on copied expected vectors; it does not
exercise those encoder binaries. The focused differential passed with the
pinned checkout at
`/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum`:

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --bin rngit --all-features \
  configured_backend_argv_matches_pinned_python_helper -- --ignored --nocapture
  PASS (1 test)
```

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

### Controlled converter process boundary

`issue_613_converter_process_tests::converter_process_boundary_honors_configuration_output_and_failures`
launches an executable fake `ffmpeg` through the real converter process path.
It verifies the configured quality and dimension options arrive in argv, source
bytes reach the helper's stdin, and a valid WebP header is accepted. It then
has the helper emit the same valid-looking bytes but exit nonzero and verifies
conversion fails and the output is removed; a final invocation sleeps beyond a
100 ms deadline and verifies bounded failure and output removal. The fake
helper does not encode an image, so this is process/configuration/status/header
plumbing evidence only—not successful codec operation, dimensions, or visual
parity. Separate production-Link fixtures cover successful `ffmpeg`,
ImageMagick `convert`, GraphicsMagick `gm`, and ImageMagick 7 `magick` operation;
`avconv` remains unverified.

The relevant pinned Python semantics were inspected at Reticulum
`99de23c040d507e3fefca19e87b182302902725d`: `media.py::convert_to_webp`
uses `_await`'s shared timeout and rejects unsuccessful child status, then
checks `_valid_webp` before success. The existing pinned helper differential
continues to cover backend selection/configured argv; this process test does
not duplicate that differential or execute Python as a second oracle.

```text
cargo test -p rns-tools --bin rngit --all-features \
  converter_process_boundary_honors_configuration_output_and_failures -- --nocapture
  PASS (1 test)
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

### Periodic stale-Link media cleanup after a silent exit

The production page service retains its 5-second announce and 60-second
cleanup cadence; a private interval seam lets a paused-time async regression
advance only test intervals. The test consumes the immediate first sweep, then
creates tracked temporary media for a Link whose
transport entry is absent, modeling a silent client exit with no close event
and no failed response write. The next timed sweep removes the directory and
its stale Link bookkeeping. The existing
`stale_page_links_are_cleaned_while_active_links_keep_media` unit test also
confirms active-Link media is preserved. This proves the periodic stale-Link
path only; it does not establish broader filesystem-failure coverage or
complete #613.

```text
cargo test -p rns-tools --bin rngit periodic_sweep_removes_media_for_silently_disappeared_link -- --nocapture
  PASS (1 test)
```

### `[pages].media_conversion` through the rngit configuration directory

Pinned `pages.py` defaults `media_conversion` to enabled and reads its optional
boolean from the `[pages]` section of the rngit config. Rust now accepts
`--config <directory>` and reads `<directory>/config` with the same supported
boolean forms; a missing setting keeps conversion enabled, malformed values
fail with an explicit configuration error, and `--no-media-conversion`
continues to force conversion off. A production TCP Link test starts rngit
with `[pages] media_conversion = no` and confirms the pinned Python client gets
the original PNG bytes and filename rather than a converted Resource. Existing
backend tests separately cover automatic winner/fallback behavior and
argument-safe bounded conversion; this adds the previously missing config-file
path and does not close the broader #613 page/media matrix.

```text
TMPDIR=/dev/shm cargo test -p rns-tools --bin rngit --all-features media_config -- --nocapture
  PASS (2 tests; default/boolean forms and malformed value)
TMPDIR=/dev/shm RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
cargo test -p rns-tools --test rngit_python_interop issue_613_media_compression \
  -- --ignored --nocapture --test-threads=1
  PASS (2 tests; pinned helper selection and production-Link configured raw-media behavior)
```
