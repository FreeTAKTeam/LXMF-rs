# #615 differential conformance and release-acceptance evidence

Status: **partial / unverified**. This records the bounded evidence-contract
increment and local release-gate repairs through candidate commit `78b98aed` on
`codex/issue-605-parity`; it does not claim completion of #615 or #605.

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

The release-gate repair also moved link-scoped ResourceManager cleanup into its
own included module. This preserves the existing split-resource terminal-event
behavior while keeping each regular Rust module within the repository's
500-line policy.

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
cargo test -p reticulum-rs-transport --lib resource                           PASS
  (73 resource tests)
cargo test -p reticulumd --test code_quality_issue_369                       PASS
cargo test -p rns-tools --bin rngit --all-features                            PASS
  (27 tests)
cargo run -p xtask -- interop-artifacts                                       PASS
cargo xtask release-check                                                      PASS
  (local candidate run, 2026-09-21 UTC; nextest 2,639 passed; Miri 29 passed /
   14 ignored; backup/restore, packaging, audit, boundary, license/source,
   reproducible-build, embedded-footprint, and SDK gates passed; soak/mesh
   reported zero failures; module-size checks passed)
```

The aggregate run reported only non-failing environment notices: the measured
loopback TCP opportunistic-delivery dispersion stayed in the warning band
(Rust 18.19%, Python 16.77%), and the boundary report retained the repository's
existing allowed legacy-shim notices. Packaging also emitted expected dry-run
upload and already-published-crate warnings.

## Deliberate remaining gaps

- The full Python↔Rust and Rust↔Python differential matrix, all-Rust/multi-hop
  scenarios, shared-daemon/restart/fault roles, exact HDLC provenance gate,
  and hosted exact-head evidence have not been certified by this increment.
- The local `cargo xtask release-check` now passes for this candidate, but it is
  not a hosted exact-head verdict and does not satisfy #616's physical,
  platform, external-client, or public-network evidence.
- No row is promoted to verified or complete here; missing runners, platform
  environments, Python/client roles, hardware, and operational evidence stay
  separate under #615/#616.
