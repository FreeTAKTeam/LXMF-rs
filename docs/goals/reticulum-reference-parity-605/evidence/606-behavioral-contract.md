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

The finite software, platform, physical-interface, external-client, network,
and operational support matrix is declared separately in
`docs/status/current-roadmap.md` and
`docs/status/reticulum-parity-matrix.md`. Hardware-unverified and unavailable
rows remain separate from software-contract results.

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

The checker and local deliberate-failure fixtures are in place, but hosted
exact-head execution, full behavioral evidence for every contract row, and
independent review of the target delta remain outstanding. The parent issue
therefore remains open, and later implementation evidence must link each row
to observed behavior before any status promotion.

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
requirements, 9 applicable, and 0 verified. This is an advisory status, not a
claim that any behavioral row is complete.

```text
python3 tools/scripts/python_surface_inventory.py --self-test                   PASS
python3 tools/scripts/python_surface_inventory.py --check \
  --json-out docs/status/python-surface-parity.json                              PASS
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
