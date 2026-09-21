# #615 differential conformance and release-acceptance evidence

Status: **partial / unverified**. This records the bounded evidence-contract
increment at candidate commit `ee50a509` on `codex/issue-605-parity`; it does
not claim the full #615 or #605 software gate.

## Reference and ownership

- Forward target: Reticulum-Python `1.5.4-dev` at
  `99de23c040d507e3fefca19e87b182302902725d`, with the active 1.5.2 baseline
  retained separately at `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`.
- Rust/tooling owners: `tools/scripts/python_surface_inventory.py`,
  `docs/status/python-surface-mapping.json`, generated parity artifacts, and
  the existing `xtask` release pipeline.

## Implemented behavior

The inventory check now loads the authoritative behavioral mapping during
`--check`, materializes it with the generated inventory's exact reference
revisions, and rejects a generated JSON artifact whose behavioral contract is
stale. The self-test covers both an equal materialization and a deliberate
coverage-status drift. This closes the specific loophole where a changed
mapping could leave an internally valid but outdated generated contract that
still passed the check.

The active-baseline generated JSON was regenerated from the pinned
`Reticulum-1.5.2` and LXMF checkouts, and the forward candidate was generated
separately under `target/issue-605/`; the candidate remains provisional and
partial rather than being promoted into the active release baseline.

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

```text
python3 tools/scripts/python_surface_inventory.py --self-test              PASS
python3 tools/scripts/python_surface_inventory.py --check \
  --json-out docs/status/python-surface-parity.json                          PASS
  (active baseline: 1,858 total; 1,857 complete; 0 partial; 1 not-applicable)
python3 tools/scripts/python_surface_inventory.py \
  --python-rns-path .tmp/python-refs/Reticulum/RNS \
  --python-lxmf-path .tmp/python-refs/LXMF/LXMF \
  --json-out target/issue-605/python-surface-parity-1.5.4.json \
  --rust-out target/issue-605/python_software_parity-1.5.4.rs                  PASS
  (forward candidate: 1,868 total; 0 complete; 1,867 partial; 1 not-applicable)
python3 -m py_compile tools/scripts/python_surface_inventory.py              PASS
git diff --check                                                               PASS
```

## Deliberate remaining gaps

- The full Python↔Rust and Rust↔Python differential matrix, all-Rust/multi-hop
  scenarios, shared-daemon/restart/fault roles, exact HDLC provenance gate,
  issue-369 diagnostics lane, dependency/license/security checks, and hosted
  exact-head evidence have not been certified by this increment.
- `cargo xtask release-check` remains the aggregate software gate and must be
  run on the final candidate after #607–#614 implementation rows settle. A
  local inventory pass is not a release verdict.
- No row is promoted to verified or complete here; missing runners, platform
  environments, Python/client roles, hardware, and operational evidence stay
  separate under #615/#616.
