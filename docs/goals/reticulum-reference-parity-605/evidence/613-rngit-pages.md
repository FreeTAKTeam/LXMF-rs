# #613 rngit NomadNet pages, media, and link cleanup evidence

Status: **partial / unverified**. This records the bounded implementation and
live pinned-Python trace at candidate commit `e41189c8` on
`codex/issue-605-parity`; it does not claim the
full #613 or #605 acceptance gate. The current issue-specific increment adds a
production-link teardown assertion for conversion temporary files.

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
| Media/files | `/media` key and path validation, URL decoding, ref/blob resolution, binary-safe filename metadata, download/artifact/work-doc endpoints, published-release filtering and absent-blob handling | local verified; pinned Python Resource payload/metadata and `/file/download` content/filename trace evidenced |
| WebP conversion | Backend preference and `RNGIT_MEDIA_BACKEND`, argv-only process construction, quality/max-dimension options, 8-second pipeline bound, bounded stderr, output validation, temporary-directory cleanup, raw fallback | local code/tests; pinned Python live `ffmpeg` conversion of a valid PNG returns validated WebP; other backends and visual parity remain unverified |
| Resource wire | Explicit outbound compression control, with `/media` responses sent uncompressed and a regression asserting no compressed advertisement | local verified; live Python Resource delivery and metadata evidenced |
| Link-scoped cleanup | Converted-media temp data is retained for an active Link and removed on `Closed`, `Stale`, or missing-link state; graceful teardown is exercised against a pinned Python peer | deterministic `stale_page_links_are_cleaned_while_active_links_keep_media` plus ignored `rngit_python_interop::rngit_serves_pages_and_media_to_pinned_python_client` | status-to-cleanup behavior and graceful production teardown verified; abrupt-process stale transition and fault/cancellation cleanup remain open |

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
components; each receives no response/failure callback rather than an
unexpected payload, proving the live malformed-request boundary fails closed.
This does not promote visual-rendering parity.

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
cargo test -p rns-tools --bin rngit --all-features              PASS (40 passed, 1 ignored)
cargo test -p rns-tools --bin rngit \
  stale_page_links_are_cleaned_while_active_links_keep_media     PASS
cargo clippy -p rns-tools --all-targets --all-features --no-deps \
  -- -D warnings                                                 PASS
RETICULUM_PY_REPO=<checkout at 99de23c040d507e3fefca19e87b182302902725d> \
LXMF_PYTHON_BIN=python3 PYTHONPATH=<pinned-checkout> cargo test \
  -p rns-tools --test rngit_python_interop \
  rngit_serves_pages_and_media_to_pinned_python_client \
  -- --ignored --nocapture                                      PASS (1 test)
git diff --check; tools/scripts/check-module-size.sh             PASS
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

The module-size script now reports only the existing
`crates/libs/rns-transport/src/resource/manager.rs:555` over-budget baseline;
the changed `rngit` and Resource sender files are within the active 500-line
limit.

## Deliberate remaining gaps

- The pinned Python/NomadNet-compatible client now completes one real TCP
  Reticulum link, identifies, exercises successful and negative page/file
  requests, and downloads raw media as a Resource with deterministic
  metadata/content checks. It also proves one generated conversion directory
  is present during the active link and gone after graceful client teardown.
  This is one bounded role trace, not proof of every page, file, or
  public-network path. Status-driven stale cleanup is deterministically
  covered, but abrupt-process stale transition, fault/cancellation cleanup,
  and the complete media lifecycle remain open.
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
