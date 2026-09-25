# Goal: Reticulum 1.5.4-dev feature update

Update the tested, working RNS 1.5.2-compatible LXMF-rs system for features
and changed behavior in the pinned RNS 1.5.4 development snapshot. Follow
[`PLAN.md`](PLAN.md) and use the existing
[`rns-1.5.4-delta.md`](../../status/rns-1.5.4-delta.md) to identify the update.
This goal does not require full parity with every inherited Python behavior.

Keep both reference revisions pinned, preserve the active release baseline,
and avoid regressions in LXMF, SDK, ZeroMQ, daemon, and utility paths. Close
#605 after the selected feature delta is integrated, affected-path regressions
and normal exact-head CI pass, and remaining gaps are accurately documented.
Open parity follow-ups #608, #609, and #611–#614 are not automatic blockers;
#616's physical, client, and public-network evidence is outside this goal.
No new release is required solely to close #605. A bounded child PR should
not use `Closes #605` unless it actually completes this scope.

## Explicit user scope decision

The explicit user scope decision to exclude the #616 operational evidence axis
is recorded for inventory provenance. The task reference aids human audit; CI
checks consistency and scope binding, while approval remains subject to PR
review rather than being cryptographically attested by the validator.

| Requirement | Decision | Review basis | Approval reference | Rationale |
| --- | --- | --- | --- | --- |
| reticulum-605-616-operational-platform-evidence | exclude-from-software-goal | explicit-user-scope-direction | codex-task:01a0bf74-9050-70e1-b8bf-0e528184c9ad | The software goal excludes physical/platform/client/network-soak acceptance and tracks those requirements under issue 616 as hardware-unverified. |
