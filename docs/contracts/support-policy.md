# Version Support and LTS Policy

Status: Active  
Policy version: `1`

## Scope
This policy defines support windows for `lxmf-sdk`, `rns-rpc`, and shipped app binaries in this repository.

## Release Channels

| Channel | Window | Patch scope | Security scope |
| --- | --- | --- | --- |
| `Current (N)` | from release date until next minor/major cut | full fixes and approved feature work | all security fixes |
| `Maintenance (N-1)` | after `N+1` ships, for 12 months | correctness and regression fixes only | all security fixes |
| `LTS` | designated release line, supported for 24 months | high-severity correctness fixes and backports only | all security fixes and advisories |
| `EOL` | after support window ends | no guaranteed fixes | advisories only, no patch SLA |

## LTS Selection Rules
1. LTS tags are designated explicitly in release notes and `docs/contracts/sdk-v2-migration.md`.
2. Only contract-compatible backports are allowed on LTS lines.
3. API/contract breaks are not allowed on LTS lines.

## Deprecation and Removal Policy
1. New deprecations must be documented in `docs/contracts/sdk-v2-migration.md`.
2. Removal of deprecated API/contract surfaces requires at least one full support window notice.
3. Any removal impacting external clients must include:
   - migration guidance,
   - contract/schema delta summary,
   - compatibility matrix impact note.

## Service-Level Expectations
1. Security issues for `Current` and `LTS` are triaged immediately and patched with highest priority.
2. `Maintenance` receives security and high-impact reliability fixes; non-critical enhancements are deferred.
3. `EOL` releases are unsupported for production use.

## Compliance Gates
Release readiness must include:
1. `cargo run -p xtask -- support-policy-check`
2. migration doc references to this policy
3. compatibility matrix and release notes aligned with declared support window
