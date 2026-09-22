# #615 differential conformance and release-acceptance evidence

Status: **local software gate green; hosted exact-head and publication acceptance
pending**. This records the bounded software evidence for the tested code
candidate `dcab8327ee9bf91c9ada1ed4a7041dfc790d7ca1` and the follow-up generated
inventory commit `b61d86389937ac2f4519ab5a1465492eff9b7257`. It does not claim
completion of #615 or #605.

Physical carriers, platform certification, external-client validation,
public-network operation, and long-running physical soak are explicitly
excluded from this goal. They remain open under #616.

## Local release gate

The complete `cargo xtask release-check` ran on 2026-09-22 against the exact
software candidate above and exited with code 0. Its release scorecard records
overall `PASS`, soak `pass` with zero E2E and mesh failures, 11 security pass
rows, eight supply-chain artifacts, and performance `SKIPPED`/advisory. The
release test phase reports 2,679 tests passed and one skipped; the Miri phase
reports 29 passed and 14 intentionally ignored. The scorecard records the
tested code candidate's full commit and a 77-second soak interval.

The active pinned-baseline inventory is regenerated and checked at
`1,858 total / 1,857 complete / 0 partial / 1 not-applicable`. The forward
Reticulum target remains separately classified as
`1,868 total / 0 complete / 1,867 partial / 1 not-applicable`; it is not
silently promoted into the baseline.

This is local candidate evidence. It is not a hosted exact-head verdict and
does not certify #616's physical, platform, client, public-network, or soak
requirements.

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

- Hosted exact-head workflows have not been run against the final published
  branch head, and the machine-readable result/publication path has not been
  completed. Therefore #615 remains open.
- The candidate branch is currently local-only. A non-mutating push probe was
  rejected by GitHub with `403 Permission denied to giu-platania`; the current
  account has `READ` permission on `FreeTAKTeam/LXMF-rs`. This is an external
  publication-authority gap, not a software-test failure.
- The local evidence above does not certify physical carriers, platform
  combinations, external clients, public-network behavior, or physical/long
  soak. Those requirements are intentionally excluded here and remain open
  under #616.
- Historical independent-peer traces and earlier candidate reports remain
  tied to their recorded commits; they are not silently relabeled as hosted
  exact-head evidence for this candidate.
