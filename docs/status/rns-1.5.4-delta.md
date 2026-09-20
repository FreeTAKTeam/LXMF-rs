# Forward RNS 1.5.4 parity candidate

Status: **open and incomplete**. This ledger is the forward candidate for issue
[#605](https://github.com/FreeTAKTeam/LXMF-rs/issues/605); it does not replace the
active RNS 1.5.2 release baseline in [`rns-1.5-delta.md`](rns-1.5-delta.md),
and it does not claim that PR #604 completes the epic.

## Frozen reference boundary

| Role | Implementation | Version | Revision | Authority |
| --- | --- | --- | --- | --- |
| Active release baseline | Reticulum-Python | 1.5.2 | `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6` | Existing release gates and historical status |
| Forward parity candidate | Reticulum-Python | `1.5.4-dev` | `99de23c040d507e3fefca19e87b182302902725d` | `tools/interop/independent-implementations.toml` `[parity_target]` |
| LXMF companion reference | LXMF-Python | pinned active revision | `727830cefda83d9c6e3982b48675425f3f988f9c` | Generated inventory input |

The forward RNS revision is an immutable development commit, not a release tag.
Changing it requires a reviewed update to the canonical manifest and this
ledger. The active release pin remains unchanged until the exact candidate
acceptance gate is complete.

## Callable inventory measurement

The existing generator was run against the exact detached checkouts with:

```text
python3 tools/scripts/python_surface_inventory.py \
  --python-rns-path .tmp/python-refs/Reticulum/RNS \
  --python-lxmf-path .tmp/python-refs/LXMF/LXMF \
  --mapping docs/status/python-surface-mapping.json \
  --json-out target/issue-605/python-surface-parity-1.5.4.json
```

The candidate scan found **1,868 callable/manual rows**: 1,867 provisional
partial rows and one provenance-backed `CRNS` not-applicable row. It adds ten
callable IDs relative to the 1,858-row active baseline:

- `RNS.Interfaces.util.HDLC.HDLC.frame`
- `RNS.Utilities.rngit.media.available_backends`
- `RNS.Utilities.rngit.media.convert_file_to_webp`
- `RNS.Utilities.rngit.media.convert_to_webp`
- `RNS.Utilities.rngit.pages.NomadNetworkNode.clean_links`
- `RNS.Utilities.rngit.pages.NomadNetworkNode.cleanup_link_temporary_resources`
- `RNS.Utilities.rngit.pages.NomadNetworkNode.get_webp_stream`
- `RNS.Utilities.rngit.pages.NomadNetworkNode.serve_media`
- `RNS.Utilities.rngit.server.ReticulumGitNode.load_allowed_permissions`
- `RNS.Utilities.rngit.server.ReticulumGitNode.update_repository_permissions`

The candidate output deliberately demotes inherited callable mappings to
`partial`. Existing wildcard rules are useful navigation hints, but a symbol
match is not behavioral proof and cannot promote a forward row to complete.
The generated active-baseline JSON remains 1,857 complete, zero partial, and
one not-applicable for historical release compatibility; its separate
`behavioral_contract` reports the current forward status as incomplete.

## Behavioral contract and child work

The machine-checked contract is stored in
[`python-surface-mapping.json`](python-surface-mapping.json) and materialized
in the generated [`python-surface-parity.json`](python-surface-parity.json).
It requires every forward requirement to name its Python reference path, exact
reference commit, Rust owner surface, implementation status, evidence status,
test command, evidence artifact, and owning issue. It currently contains ten
incomplete requirements:

| Owner | Requirement | Current status |
| ---: | --- | --- |
| #607 | Review and integrate the initial PR increment | partial / unverified |
| #608 | Wire IFAC into production carrier ingress and egress | partial / unverified |
| #609 | Close transport, local-client, and shared-instance gaps | partial / unverified |
| #610 | Prove Resource collision, stream, and mixed-peer behavior | partial / unverified |
| #611 | Exercise every reference utility through real network workflows | partial / unverified |
| #612 | Match rngit permission, resolver, work, storage, and wire schemas | partial / unverified |
| #613 | Match rngit NomadNet pages, media, and link cleanup | partial / unverified |
| #614 | Validate native interface runtimes and Windows BLE behavior | partial / hardware-unverified |
| #615 | Run differential conformance and exact-candidate software release acceptance | partial / unverified |
| #616 | Maintain separate physical, platform, client, network-soak, and operational evidence | not-applicable to software / hardware-unverified |

The generated Rust constants expose the forward behavioral level and target
revision for runtime consumers without changing the existing advisory SDK/RPC
schema before the contract is proven. No row is promoted by a local parser,
mock, attached-node-only check, or old release artifact.

## Acceptance gate

This candidate is not release-complete until the contract coverage is complete,
all applicable rows have verified evidence, the exact candidate CI/release gate
passes, and the independent child issues have been reviewed. Hardware and
public-network evidence remain a separate axis and cannot be inferred from
software tests. The `--require-behavioral-complete` generator mode is expected
to fail while this ledger is incomplete.
