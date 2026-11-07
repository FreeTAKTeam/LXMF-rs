# Automatic CI and main protection

Ordinary PRs and pushes to main run the existing security policy without labels.
The Monday scan runs security only. Audit exceptions remain in the existing
xtask stage and `deny.toml`; tool versions live in the shared security action.
A failed install, policy check or advisory fetch fails validation.

`tools/scripts/ci_selection.py` uses Cargo package ownership and transitive
reverse dependencies, including optional, target, development and build edges.
Manifest/lock changes, shared fixtures, vendored code, workflow/xtask/tooling
changes and unknown paths select the broad software set. Missing history also
broadens coverage; unavailable metadata falls back to workspace tests. Renames
and copies consider both paths. Only explicitly allowlisted prose uses the
documentation path. Executable decision documents remain software inputs.

Integration runs enumerate Cargo test targets, enabling their required features.
Existing nextest lib/bin coverage and explicit mobile-feature checks remain.
Four libtest threads bound independent fixtures sharing the process-wide ZeroMQ
connection budget; workloads inside tests and production limits are unchanged.
Labels can widen selection but are never necessary for correctness.

Every CI job is a dependency of `CI required`. The gate verifies the selection
schema and requires successful selected jobs, including security. Only explicitly
inapplicable jobs may skip. Missing, failed, cancelled or unexpectedly skipped
required work cannot pass. A cancelled whole workflow or invalid workflow that
never produces the context remains blocked by native protection.

The selected suites and outcomes appear in job summaries; the selection JSON is
also retained as an Actions artifact. Verify and independent interoperability
remain separate required workflows, preserving their existing tests without a
second copy. Their current runtime can exceed the approximate 15-minute target;
the target does not justify dropping applicable checks. Hardware, long soak and
optional release jobs retain their existing triggers.

## One-time setup

Use existing maintainer `gh` authentication locally. PR jobs have read-only
repository permissions, no administrative credentials and no `pull_request_target`
execution. Fork and Dependabot changes use the same selection policy.

```sh
python3 tools/scripts/branch_protection.py check
python3 tools/scripts/branch_protection.py apply --head <reviewed-PR-head-SHA>
python3 tools/scripts/branch_protection.py check
```

`check` is read-only. It reports `correct`, `missing`, `conflicting` or
`inaccessible`; permission errors and ambiguous 404s never mean unprotected.
`apply` creates the policy only when main is explicitly unprotected, no branch
ruleset exists, and all configured contexts have passed on the supplied head
from the configured GitHub Actions app. Repeated application of the same policy
does nothing. Any differing existing protection, stronger control, bypass list
or branch ruleset is preserved and reported as a conflict. Readback verifies the
applied state. No organization settings or merge strategies are changed.

The single configuration is `.github/branch-protection.json`. It requires a PR,
an up-to-date branch and three Actions-bound checks: `CI required`, `HIL PR level`
and `evidence (rns-rs)`. Force pushes and deletion are forbidden. There are zero
mandatory human approvals and no mandatory CODEOWNER, last-push approval, queue
or global auto-merge setting. This adopts only #244's lightweight control subset;
it does not adopt contributor tiers, PR-size approvals or manual ceremonies.

Administrators are subject to the checks during ordinary merging. Existing
administrators retain GitHub's native ability to inspect and repair repository
protection settings during recovery; this helper does not add bypass actors or
alter permissions. GitHub's native audit history records administrative changes.

After initial application, prove native enforcement on the implementation PR:
observe GitHub blocking a failed/cancelled required run, then restore successful
checks on that same reviewed head and verify merge eligibility with zero required
approvals. Local negative tests alone do not establish native enforcement.
