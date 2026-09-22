# #607 PR #604 review and integration evidence

Status: **partial / unverified**. PR #604 is merged and its hosted checks
passed, but this record does not substitute for independent human review or
physical-device evidence, and it does not close #607 or #605.

## Merged increment

- PR: [#604](https://github.com/FreeTAKTeam/LXMF-rs/pull/604)
- Implementation commits: `4665086bbafc789a79d523e8b1d679e245e303c4` and
  `c2f83a0e48266411f4adaf2ef49e4f8a5ddc9836`.
- Merge commit: `3ed5932da4420e2dd1b9d36283b0e72a364e3ebe`.
- Merged: 2026-09-21 UTC.

The merged increment covers BLE lifecycle/EOF handling, HDLC framing vectors,
and rngit permission/work-transition behavior. The recorded review payload for
the implementation commit contains no inline blocking finding. That is review
evidence, not an approval claim; a human maintainer still owns the independent
review boundary.

## Hosted checks on the actual PR

The following checks passed for the merged PR head:

- [CI quality](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35520690650/job/106104130674),
  [architecture checks](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35520690650/job/106104130623),
  [stable build matrix](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35520690650/job/106104130720),
  [contracts](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35520690650/job/106104130726),
  and [nextest unit tests](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35520690650/job/106104130689);
- [rns-rs independent interoperability](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35520690643/job/106104130422); and
- [PR-level HIL](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35520690588/job/106104130294).

The publish and `peer-lifecycle-essential` jobs were skipped, so they are not
counted as passes. The merged status is therefore a bounded software/hosted
integration result, not complete operational acceptance.

## Preserved local coverage

The exact-head release gate includes the merged increment's focused BLE,
bearer-idle, and HDLC test modules under
`crates/libs/rns-transport/tests/`, the pinned HDLC fixture under
`crates/libs/test-support/fixtures/rns_1_5_4_hdlc.json`, and the rngit
permission/work transition tests under
`crates/apps/rns-tools/src/bin/rngit_parts/rns_1_5_4_tests.rs`. The associated
delta remains recorded in `docs/status/rns-1.5.4-delta.md` and the parity
matrix; physical BLE/RNode and platform-specific evidence remain
`hardware-unverified`.

## Evidence boundary

This confirms that the reviewed PR was merged through the normal process and
that its hosted software checks completed successfully. It does not certify
the full #606 behavioral contract, native Windows/BLE behavior on hardware,
the broader #614 matrix, or parent #605 completion.
