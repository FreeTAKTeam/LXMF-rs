# #613 rngit NomadNet pages, media, and link cleanup evidence

Status: **partial / unverified**. This records the bounded implementation and
live pinned-Python trace at candidate commit `fc34e5b4` on
`codex/issue-605-parity`; it does not claim the
full #613 or #605 acceptance gate.

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
| WebP conversion | Backend preference and `RNGIT_MEDIA_BACKEND`, argv-only process construction, quality/max-dimension options, 8-second pipeline bound, bounded stderr, output validation, temporary-directory cleanup, raw fallback | local code/tests; real encoder success and image fixture unverified |
| Resource wire | Explicit outbound compression control, with `/media` responses sent uncompressed and a regression asserting no compressed advertisement | local verified; live Python Resource delivery and metadata evidenced |

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

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
The fixture runs with conversion disabled, so this trace validates the raw
binary path and the reference `auto_compress=False` Resource boundary; it does
not promote encoder-success or visual-rendering parity.

The module-size script now reports only the existing
`crates/libs/rns-transport/src/resource/manager.rs:555` over-budget baseline;
the changed `rngit` and Resource sender files are within the active 500-line
limit.

## Deliberate remaining gaps

- The pinned Python/NomadNet-compatible client now completes one real TCP
  Reticulum link, identifies, exercises successful and negative page/file
  requests, and downloads raw media as a Resource with deterministic
  metadata/content checks. This is one bounded role trace, not proof of every
  page, file, or public-network path.
- The Rust page rendering is intentionally a compact service implementation;
  full Markdown highlighting, pagination, diff rendering, signed work-document
  presentation, and every reference template detail remain open.
- A live encoder-success fixture was not run in this environment; invalid
  conversion fallback, WebP header validation, timeout/cleanup code paths, and
  argument construction are covered locally.
- Missing-key/malformed-media live failure transcripts, Reticulum public-key
  work-document signature verification, restart/concurrent-writer/fault
  transcripts, and end-to-end rngit Git/work network workflows remain open
  under #612/#613.
