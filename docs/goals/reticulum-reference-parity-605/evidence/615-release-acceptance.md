# #615 differential conformance and release-acceptance evidence

Status: **partial / unverified**. This records the bounded evidence-contract
increment, local release-gate repairs, exact-target inventory gate, and the
clean local Python/Rust matrix through candidate commit `fd999d2d` on
`codex/issue-605-parity`; it does not
claim completion of #615 or #605.

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

The new `tools/scripts/python_compat_matrix.py` runner turns the previously
individually ignored compatibility cases into an explicit evidence gate. It
checks the dispatch contract, requires clean exact-reference checkouts, records
the candidate and toolchain revisions, runs all 23 live Python/Rust cases plus
the seven deterministic local transport cases, rejects missing reports and
ignored/skipped tests, and writes per-case logs plus one aggregate JSON report.
The Verify workflow now checks out the exact forward Reticulum target beside
the existing 1.5.2 baseline checkout and runs this gate fail-closed on pull
requests, uploading the aggregate report and raw case evidence without
changing the baseline HIL lane.

The current candidate adds a separate Verify step that scans the exact
`Reticulum-parity` checkout with `python_surface_inventory.py`; newly exposed
or unmapped callable rows fail before the compatibility matrix runs. The
inventory validator also rejects repository-escaping evidence paths and
requires an existing artifact whenever a behavioral row is marked verified.
The same target scan is available to the `xtask` docs/release helper through
the paired `PYTHON_RNS_PARITY_PATH` and `PYTHON_LXMF_PARITY_PATH` variables.

The existing independent-implementation lane was also executed locally at
nightly level. Against pinned rns-rs `6c6d79b83516feff271d15c97d39dd1de7798afe`,
92 scenarios covered two-node, mixed and all-LXMF five-node, multi-hop,
routing, restart, shared-daemon, and deterministic loss/latency/
duplication/reordering topologies. It recorded 87 PASS, three explicitly
classified rns-rs peer divergences, and two dependent teardown blocks; the
explicit compatibility gate passed. Against pinned Reticulum-Go
`48f15178f6fbc34aeb69ad428679db9deddae7f4`, 22 scenarios passed and two large
peer-to-Rust Resource directions were recorded as `UNSUPPORTED` because the
peer control API has a documented 1 MiB inbound limit; its explicit gate also
passed. These are classified local evidence, not an unqualified claim that
every independent peer direction is complete.

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
PYTHON_RNS_PARITY_PATH=.tmp/python-refs/Reticulum/RNS \
PYTHON_LXMF_PARITY_PATH=.tmp/python-refs/LXMF/LXMF \
cargo xtask ci --stage doc                                             PASS
  (baseline and exact-target inventories both passed; target: 1,868 total,
   0 complete, 1,867 partial, 1 not-applicable)
python3 -m py_compile tools/scripts/python_surface_inventory.py              PASS
python3 tools/scripts/test_python_compat_matrix.py                            PASS
python3 tools/scripts/python_compat_matrix.py --all \
  --output target/interop/python-compat-matrix/full/matrix.json \
  --timeout 420                                                               PASS
  (candidate Rust `d4d533ad95776c032399e1078baf79d12b28a347`; Python
   Reticulum `99de23c040d507e3fefca19e87b182302902725d`; Python LXMF
   `727830cefda83d9c6e3982b48675425f3f988f9c`; 30/30 passed, 0 failed,
   0 blocked, 0 skipped, 0 ignored; 2026-09-21 UTC)
python3 tools/scripts/independent_interop.py --peer rns-rs --level nightly \
  --output target/interop/independent/issue-605-nightly --keep \
  --skip-build --peer-root target/interop/independent/external/rns-rs     CLASSIFIED
  (runner exit 1 for 3 allowlisted `peer_divergence` rows and 2 dependent
   teardown blocks; 87 PASS of 92 scenarios; candidate
   `4f968cb8af06661d9dd9831f8ec1cc29556f74df`; peer
   `6c6d79b83516feff271d15c97d39dd1de7798afe`)
python3 tools/scripts/independent_interop_gate.py \
  target/interop/independent/issue-605-nightly/independent-interop.json      PASS
python3 tools/scripts/independent_interop.py --peer reticulum-go \
  --level nightly --output target/interop/independent/issue-605-reticulum-go \
  --keep                                                                    PASS
  (22 PASS, 2 explicit `UNSUPPORTED` peer-surface rows; candidate
   `4f968cb8af06661d9dd9831f8ec1cc29556f74df`; peer
   `48f15178f6fbc34aeb69ad428679db9deddae7f4`)
python3 tools/scripts/independent_interop_gate.py \
  target/interop/independent/issue-605-reticulum-go/independent-interop.json  PASS
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

- The broader #615 differential surface beyond this 30-case Python/Rust matrix,
  including all-Rust/multi-hop scenarios, shared-daemon/restart/fault roles,
  and hosted exact-head evidence, has not been certified by this increment. The
  local independent lanes cover the declared local topologies and deterministic
  fault proxy cases, but the reports do not yet carry a reproducible seed for
  every fault run, and hosted exact-head evidence is still absent.
- The matrix's machine-readable report and raw per-case logs are generated
  under ignored `target/interop/python-compat-matrix/`; they are local evidence
  for this candidate and are not a hosted exact-head verdict.
- The local `cargo xtask release-check` now passes for this candidate, but it is
  not a hosted exact-head verdict and does not satisfy #616's physical,
  platform, external-client, or public-network evidence.
- No row is promoted to verified or complete here; missing runners, platform
  environments, Python/client roles, hardware, and operational evidence stay
  separate under #615/#616.
