# Issue #605 scope decision record

Status: closed. The selected RNS 1.5.4-dev feature update was completed on the
RNS 1.5.2-compatible release baseline. The broader forward-behavior contract
and physical/platform/client evidence remain separate and incomplete; this
record does not claim full RNS 1.5.4 parity.

This file stays at its current path because Python surface inventory tooling
and CI use it to bind the explicit scope decision below. For current status and
evidence, see the [feature delta](../../status/rns-1.5.4-delta.md),
[Reticulum parity matrix](../../status/reticulum-parity-matrix.md), and
[acceptance evidence](evidence/615-release-acceptance.md). Original issue:
[#605](https://github.com/FreeTAKTeam/LXMF-rs/issues/605).

## Explicit user scope decision

The explicit user scope decision to exclude the #616 operational evidence axis
is recorded for inventory provenance. The task reference aids human audit; CI
checks consistency and scope binding, while approval remains subject to PR
review rather than being cryptographically attested by the validator.

| Requirement | Decision | Review basis | Approval reference | Rationale |
| --- | --- | --- | --- | --- |
| reticulum-605-616-operational-platform-evidence | exclude-from-software-goal | explicit-user-scope-direction | codex-task:01a0bf74-9050-70e1-b8bf-0e528184c9ad | The software goal excludes physical/platform/client/network-soak acceptance and tracks those requirements under issue 616 as hardware-unverified. |
