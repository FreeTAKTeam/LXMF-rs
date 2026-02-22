# Security Policy

## Supported Versions

Security fixes are prioritized according to the support policy in
`docs/contracts/support-policy.md`.

| Release channel | Security support status |
| --- | --- |
| Current (N) | Full security fixes and advisories |
| Maintenance (N-1) | Critical/high fixes and selected medium fixes |
| LTS | Security and stability fixes per LTS policy |
| EOL | No security fixes (upgrade required) |

## Reporting a Vulnerability

Please do not open public issues for suspected vulnerabilities.

Report privately to: `security@freetakteam.org` with:

1. Affected component(s) and version/commit.
2. Reproduction details or proof-of-concept.
3. Impact statement and expected security boundary.
4. Any suggested mitigations.

## Coordinated Disclosure Workflow

Initial response targets:

1. Acknowledge receipt within 24 hours.
2. Complete initial triage within 72 hours.
3. Provide remediation plan or mitigation guidance within 7 days.

Coordinated disclosure steps:

1. Intake and severity assignment.
2. Reproduction and impact confirmation.
3. Patch and backport planning.
4. Advisory publication and migration guidance.

Detailed execution runbook:

- `docs/runbooks/cve-response-workflow.md`

## CVE and Advisory Handling

For confirmed vulnerabilities:

1. Assign internal incident ID and tracking issue.
2. Request CVE identifier when required.
3. Publish remediation advisory with fixed versions and upgrade path.
4. Update migration notes and release notes if contract/behavior changes.

## Scope Notes

- Security issues in third-party dependencies are tracked through dependency advisories and
  triaged for impact on supported releases.
- Reports that only affect unsupported/EOL versions may be closed with upgrade guidance.
