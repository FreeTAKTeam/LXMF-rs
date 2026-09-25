# #606 behavioral-reference contract evidence

Status: **partial / unverified**. This records the executable contract and
deliberate-failure coverage in `c2ec9cb5`; it does not close #606 or #605 and
does not promote callable-surface matches into behavioral parity.

## Frozen references and ownership

- Active baseline: Reticulum-Python `1.5.2` at
  `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`.
- Forward target: Reticulum-Python `1.5.4-dev` at
  `99de23c040d507e3fefca19e87b182302902725d`.
- LXMF reference: `727830cefda83d9c6e3982b48675425f3f988f9`.
- Owner: `tools/scripts/python_surface_inventory.py`,
  `docs/status/python-surface-mapping.json`, and the generated parity
  artifacts consumed by the documentation and release gates.

The mapping's behavioral contract is materialized into each generated
inventory with the exact revisions used for that inventory. The generated
check rejects stale materialization, malformed references, missing required
fields, and verified rows whose evidence artifact is absent. Callable mapping
rules remain useful historical classification for the active baseline; when a
forward-target scan matches a `complete` wildcard rule, the result is
demoted to `partial` and annotated as provisional.

## Deliberate failure and status coverage

The generator self-test now exercises:

- malformed behavioral requirements and contradictory `complete` coverage;
- distinct `blocked` and `hardware-unverified` coverage/evidence statuses;
- stale generated reference revisions and generated contract drift;
- missing and present verified evidence artifacts;
- an unmapped forward callable;
- variable inventory item counts whose summaries are derived from the items,
  rather than compared with a fixed expected total; and
- forward wildcard demotion while retaining the active-baseline mapping.

These fixtures make the intended boundaries executable: unavailable hardware
or a blocked runner cannot become success, a new target callable cannot be
silently omitted, and a matching symbol is not behavioral evidence by itself.

The finite verification inventory is declared in
[`606-support-matrix.md`](606-support-matrix.md). It is grounded in configured
hosted workflow OS families, daemon interface/HIL profile names, the pinned
Reticulum 1.5.4-dev target, and the configured independent peers. Hosted runner
architectures are not pinned, so the matrix explicitly records architecture as
not declared/not verified rather than inferring it. Physical and direct
external-network rows are **NOT RUN / excluded** from this software goal under
the explicit #616 scope decision in `GOAL.md`; the matrix is not operational
acceptance and does not close #606 or #616.

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

```text
python3 tools/scripts/python_surface_inventory.py --self-test                 PASS
python3 -m py_compile tools/scripts/python_surface_inventory.py               PASS
python3 tools/scripts/python_surface_inventory.py --check \
  --json-out docs/status/python-surface-parity.json                             PASS
python3 -m json.tool docs/status/python-surface-mapping.json >/dev/null         PASS
python3 -m json.tool docs/status/python-surface-parity.json >/dev/null          PASS
git diff --check                                                               PASS
```

The generated inventory check still reports the existing active-baseline
classification (`1,858 total / 1,857 complete / 0 partial /
1 not-applicable`). The forward candidate remains provisional and partial; no
behavioral requirement is marked verified by this increment.

## Remaining acceptance boundary

The checker, local deliberate-failure fixtures, hosted exact-head execution,
and finite verification inventory are in place. Full behavioral evidence for
every contract row and independent review of the target delta remain
outstanding. Platform runtime and physical/client operation remain separately
unverified under #616. The parent issue therefore remains open, and later
implementation evidence must link each row to observed behavior before any
status promotion.

## Public SDK/RPC advisory increment

Branch `corvo/issue-606-behavioral-advisory`, based on merged main
`a649f51e9671007c08aeff469877038e2db7a716`, adds the forward behavioral
checkpoint to the typed SDK/RPC advisory without changing the active 1.5.2
callable counts. The serialized member is optional for old payloads and is
omitted when absent. OpenRPC, the projected RPC schema, the valid negotiation
fixture, current status documentation, and the public API baseline now agree.
This increment does not close #606: a complete behavior-by-behavior audit and
independent target-delta review remain outstanding.

The active inventory was regenerated from Reticulum
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6` and LXMF
`727830cefda83d9c6e3982b48675425f3f988f9c`. The separately pinned behavioral
checkpoint reports RNS `1.5.4-dev` at
`99de23c040d507e3fefca19e87b182302902725d`: `partial` / `incomplete`, 10
requirements, 9 applicable, and 1 verified (#615 release acceptance). The
eight behavior-owner rows (#607–#614) remain unverified; this advisory status
does not claim full parity.

The inventory validator now restricts this `not-applicable` exception to the
#616 operational requirement, issue owner, and `hardware-unverified` evidence
status. Its task reference, review basis, and rationale must match the unique
decision row under the designated `Explicit user scope decision` heading in
the canonical goal record. Fenced Markdown examples do not count as rows. The
validator rejects conflicting rows, unrelated sections/requirements, path
traversal or symlink escape, missing records, malformed NUL paths, and missing
provenance. The reference supports human audit; CI checks consistency, while
approval remains subject to PR review. #616 stays hardware-unverified rather
than being promoted to software success. CI invokes the self-test before
accepting exact-target inventory output.

### Local validation of the provenance follow-up

These checks were rerun in the `corvo/issue-606-behavioral-advisory` worktree
after the provenance-gate edits. Reference checkouts resolved to the exact
active baseline Reticulum `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, LXMF
`727830cefda83d9c6e3982b48675425f3f988f9c`, and forward Reticulum target
`99de23c040d507e3fefca19e87b182302902725d`.

```text
python3 tools/scripts/python_surface_inventory.py --self-test                     PASS
python3 tools/scripts/python_surface_inventory.py \
  --python-rns-path /tmp/lxmf-606-parity-refs.hv0vPX/Reticulum/RNS \
  --python-lxmf-path /tmp/lxmf-606-parity-refs.hv0vPX/LXMF/LXMF \
  --mapping docs/status/python-surface-mapping.json \
  --json-out docs/status/python-surface-parity.json --check                        PASS (1,858 / 1,857 complete)
python3 tools/scripts/python_surface_inventory.py --check                         PASS
python3 tools/scripts/python_surface_inventory.py \
  --python-rns-path /tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0/RNS \
  --python-lxmf-path /tmp/lxmf-606-parity-refs.hv0vPX/LXMF/LXMF \
  --mapping docs/status/python-surface-mapping.json \
  --json-out /tmp/lxmf-606-inventory.ugj10O/forward.json \
  --rust-out /tmp/lxmf-606-inventory.ugj10O/forward.rs                            PASS (1,868 / 1,867 partial)
cargo fmt --all -- --check                                                       PASS
cargo test -p lxmf-reference --lib                                               PASS (2)
python3 -m json.tool docs/status/python-surface-mapping.json                     PASS
python3 -m json.tool docs/status/python-surface-parity.json                      PASS
git diff --check                                                                 PASS
```

The forward-target generation wrote only to a temporary output directory; the
committed parity artifact intentionally remains the canonical 1.5.2 active
baseline. These commands record the local pre-push validation. The pushed PR
head and successful hosted run on that exact SHA are recorded below.

```text
forward reference: 99de23c040d507e3fefca19e87b182302902725d
callable inventory: 1,868 total / 0 complete / 1,867 partial / 1 not-applicable
behavioral contract: 10 requirements / 9 applicable / 1 verified (#615 release acceptance)
behavior-owner rows: 8 / 8 unverified (#607–#614)
```

```text
python3 tools/scripts/python_surface_inventory.py \
  --python-rns-path Reticulum-parity/RNS \
  --python-lxmf-path LXMF/LXMF \
  --mapping docs/status/python-surface-mapping.json \
  --json-out target/issue-605/python-surface-parity-1.5.4.json \
  --rust-out target/issue-605/python_software_parity-1.5.4.rs                    PASS (1,868)
cargo fmt --all -- --check                                                      PASS
cargo test -p lxmf-reference --lib                                              PASS (2)
cargo test -p lxmf-sdk --lib                                                    PASS (238)
cargo test -p reticulum-rs-rpc --lib                                            PASS (750)
cargo run -p xtask -- sdk-schema-check                                          PASS (12)
cargo run -p xtask -- sdk-api-break                                             PASS
cargo run -p xtask -- sdk-docs-check                                            PASS
cargo run -p xtask -- schema-client-generate --check                            PASS
cargo clippy -p lxmf-reference -p lxmf-sdk -p reticulum-rs-rpc \
  --all-targets --all-features --no-deps -- -D warnings                         PASS
```

### Hosted exact-head validation

PR #627 head `7593a1847d01128b9b6e23397e5449828e59b03f` completed the hosted
Verify workflow successfully on 2026-09-22. The runs used that exact head SHA:

```text
HIL PR level                 PASS  run 35779677720
evidence (rns-rs)            PASS  run 35779677718
architecture-checks         PASS  run 35779677777
build-matrix (stable)        PASS  run 35779677777
contracts                    PASS  run 35779677777
quality                      PASS  run 35779677777
test-nextest-unit            PASS  run 35779677777
publish                      SKIPPED
peer-lifecycle-essential     SKIPPED
```

The workflow ran the behavioral-inventory self-test and exact-target inventory
generation against the pinned reference. This closes the hosted-execution gap
for the contract gate; it does not provide missing behavioral observations,
freeze the external support matrix, or replace independent review of the target
delta.

### Reference-pin mirror corruption fixtures

Source commit `2190571f` extends the exact-pin checker self-test. It builds a
temporary repository fixture from the canonical manifest and mirror templates,
confirms the intact fixture passes, then corrupts the forward-target workflow
pin and the active-baseline Rust source pin independently. Each corrupted
mirror must be reported by `verify()` as missing the canonical revision. The
fixtures therefore exercise the same checker used by CI rather than only
asserting that the expected template strings exist.

```text
python3 -m py_compile tools/scripts/check_python_reference_pins.py             PASS
python3 tools/scripts/check_python_reference_pins.py --self-test               PASS
python3 tools/scripts/check_python_reference_pins.py                           PASS
python3 tools/scripts/python_surface_inventory.py --self-test                  PASS
git diff --check                                                                PASS
```

The checker hardening does not change the frozen pins or promote a behavioral
requirement. The finite verification inventory is recorded in
[`606-support-matrix.md`](606-support-matrix.md); it makes no platform-support
claim beyond configured workflow coverage and claims no physical/client
result. Full behavioral observations and independent target-delta review
remain open; #606 and #605 remain open, and #616 remains excluded and
hardware-unverified.
