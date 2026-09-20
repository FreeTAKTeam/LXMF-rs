# #613 rngit NomadNet pages, media, and link cleanup evidence

Status: **partial / unverified**. This records the bounded implementation at
candidate commit `9edc4a42` on `codex/issue-605-parity`; it does not claim the
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
| NomadNet node | Persistent/seeded identity, `nomadnetwork.node` destination, TCP listen/connect interfaces, periodic announce app data, request-path decoding, page/file dispatch, packet or Resource response selection | local verified; live link/client unverified |
| Pages | Index/group/repository/tree/blob/commits/commit/refs/stats/releases/release/work/work-doc paths, `var_*` query fields, ref/path validation, not-found/error rendering, custom static and bounded executable templates, binary-image `/media` markup | local verified; visual/reference rendering parity incomplete |
| Access control | Repository read/stats/release checks, work-document read checks, blocked unidentified-client no-identity template, malformed/denied/missing request paths fail closed | local verified |
| Media/files | `/media` key and path validation, URL decoding, ref/blob resolution, binary-safe filename metadata, download/artifact/work-doc endpoints, published-release filtering and absent-blob handling | local verified |
| WebP conversion | Backend preference and `RNGIT_MEDIA_BACKEND`, argv-only process construction, quality/max-dimension options, 8-second pipeline bound, bounded stderr, output validation, temporary-directory cleanup, raw fallback | local code/tests; real encoder success and image fixture unverified |
| Resource wire | Explicit outbound compression control, with `/media` responses sent uncompressed and a regression asserting no compressed advertisement | local verified |

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

```text
cargo fmt --all -- --check                                      PASS
cargo test -p rns-tools --all-features                          PASS
cargo test -p rns-tools --bin rngit --all-features              PASS (27 tests)
cargo clippy -p rns-tools --all-targets --all-features --no-deps \
  -- -D warnings                                                 PASS
cargo test -p reticulum-rs-transport --lib resource:: --all-features PASS (63 tests)
cargo clippy -p reticulum-rs-transport --lib --all-features \
  --no-deps -- -D warnings                                       PASS
git diff --check                                                 PASS
cargo run -q -p rns-tools --bin rngit -- --root /tmp \
  --identity-seed issue-605 --print-identity --silent             PASS
```

The module-size script now reports only the existing
`crates/libs/rns-transport/src/resource/manager.rs:555` over-budget baseline;
the changed `rngit` and Resource sender files are within the active 500-line
limit.

## Deliberate remaining gaps

- A pinned Python/NomadNet-compatible client has not yet completed a real
  Reticulum link, page request, media Resource download, and checksum/fixture
  transcript against this Rust service. The direct handler tests and the
  `rngit --print-identity` startup check are not substitutes for that role.
- The Rust page rendering is intentionally a compact service implementation;
  full Markdown highlighting, pagination, diff rendering, signed work-document
  presentation, and every reference template detail remain open.
- A live encoder-success fixture was not run in this environment; invalid
  conversion fallback, WebP header validation, timeout/cleanup code paths, and
  argument construction are covered locally.
- Reticulum public-key work-document signature verification, restart/concurrent
  writer/fault transcripts, and end-to-end rngit Git/work network workflows
  remain open under #612/#613.
