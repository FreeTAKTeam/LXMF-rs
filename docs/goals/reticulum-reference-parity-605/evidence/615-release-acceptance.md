# #615 differential conformance and release-acceptance evidence

Status: **software acceptance complete for the scoped #615 goal**. This records
the bounded software evidence for final PR #626 head
`559e314c71306148352050a463d3db10407c89f0`; it does not claim completion of
#605 or the excluded operational axis in #616.

Physical carriers, platform certification, external-client validation,
public-network operation, and long-running physical soak are explicitly
excluded from this goal. They remain open under #616.

## Hosted software acceptance

The final published PR head passed every hosted software gate:

- [Verify run 35759162501](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35759162501): PR HIL `23/23` cases passed and the exact-target Python compatibility matrix reported `30/30` passed, `0` failed, `0` blocked, and `0` skipped.
- [Independent interoperability run 35759162507](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35759162507): passed.
- [CI run 35759162664](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35759162664): passed.

The hosted matrix used Reticulum `1.5.4-dev`
(`99de23c040d507e3fefca19e87b182302902725d`) and LXMF
(`727830cefda83d9c6e3982b48675425f3f988f9c`), with no preflight errors. The
Verify workflow also records the four reference checkouts as auxiliary paths
so the clean candidate commit check is meaningful.

## Local release gate

The complete `cargo xtask release-check` ran on 2026-09-22 against the final
software candidate above and exited with code 0. Its release scorecard records
overall `PASS`, soak `pass` with zero E2E and mesh failures, 11 security pass
rows, eight supply-chain artifacts, and performance `SKIPPED`/advisory. The
release test phase reports 2,684 tests passed and one skipped; the Miri phase
reports 29 passed and 14 intentionally ignored. The scorecard provenance is
the final candidate commit and records a 77-second soak interval.

The active pinned-baseline inventory is regenerated and checked at
`1,858 total / 1,857 complete / 0 partial / 1 not-applicable`. The forward
Reticulum target remains separately classified as
`1,868 total / 0 complete / 1,867 partial / 1 not-applicable`; it is not
silently promoted into the baseline.

This is software evidence only. It does not certify #616's physical, platform,
client, public-network, or long-running physical-soak requirements.

## Reference and ownership

- Forward target: Reticulum-Python `1.5.4-dev` at
  `99de23c040d507e3fefca19e87b182302902725d`.
- Active baseline: Reticulum-Python `1.5.2` at
  `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`.
- LXMF-Python checkout: `727830cefda83d9c6e3982b48675425f3f988f9c`.
- Inventory owners: `tools/scripts/python_surface_inventory.py`,
  `docs/status/python-surface-mapping.json`, generated parity artifacts, and
  the existing `xtask` release pipeline.

## Verified local software evidence

The current candidate includes the Resource-link watchdog correction: resource
proofs and resource data now refresh the owning link's inbound activity just
like ordinary link traffic. This removes the observed long-transfer teardown
at the link timeout boundary. The same candidate corrects invalid empty-packet
fixtures rejected by the strict packet decoder and makes resource-fault tests
fail fast on terminal failure events.

The exact local software lanes completed as follows:

- Full workspace nextest: `3,176` passed, `107` skipped.
- Full serialized `rns-transport` suite: `979` passed.
- Pinned-Python channel interoperability: `48/48` passed, including both
  bidirectional 50 MiB resource transfers, fault/cancellation/shutdown paths,
  and memory-budget checks.
- Pinned-Python compatibility matrix: `30/30` passed; four non-selected
  target-level tests were skipped by the test binary.
- Pinned-Python paper interoperability: `12/12` passed.
- LXMD remote relay interoperability: `6/6` passed.
- `rns-tools` ignored Python interoperability lane: `10/10` passed.
- SDK HTTP-versus-ZeroMQ transport stress: `1/1` passed at 1,000 iterations
  with the matching local daemon configuration.

The supporting repository gates also passed: format and diff checks, strict
workspace Clippy, workspace boundaries, module-size policy, architecture
checks, the issue-369 diagnostic scanner, wire-conformance artifacts, the
active-baseline inventory check, and the complete release-check above.

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

```text
python3 tools/scripts/python_surface_inventory.py --check \
  --json-out docs/status/python-surface-parity.json --require-complete       PASS
  (active baseline: 1,858 total; 1,857 complete; 0 partial; 1 not-applicable)

CARGO_INCREMENTAL=0 cargo nextest run --workspace --test-threads 1 \
  --no-fail-fast --status-level fail --final-status-level fail              PASS
  (3,176 passed; 107 skipped)

CARGO_INCREMENTAL=0 RETICULUM_PY_REPO=.tmp/python-refs/Reticulum \
  LXMF_PY_REPO=.tmp/python-refs/LXMF LXMF_PYTHON_BIN=python3 \
  cargo nextest run -p reticulumd --test python_channel_interop -j 1 \
  --no-fail-fast --status-level fail --final-status-level fail -- \
  --ignored --nocapture                                                       PASS
  (48/48)

CARGO_INCREMENTAL=0 cargo +nightly miri test -p lxmf-wire --lib -- \
  --nocapture                                                               PASS
  (29 passed; 14 intentionally ignored)

CARGO_INCREMENTAL=0 cargo xtask release-check                               PASS
  (2,679 passed; 1 skipped; scorecard overall PASS; code candidate
   dcab8327ee9bf91c9ada1ed4a7041dfc790d7ca1)
```

## Remaining acceptance gaps

- There is no remaining software acceptance gap for the scoped #615 goal.
- Physical carriers, platform combinations, external clients, public-network
  behavior, and long-running physical soak are intentionally excluded here and
  remain open under #616.
- Historical independent-peer traces and earlier candidate reports remain
  tied to their recorded commits; they are not silently relabeled as hosted
  exact-head evidence for this candidate.
