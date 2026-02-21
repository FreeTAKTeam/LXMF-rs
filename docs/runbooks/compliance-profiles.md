# Compliance Deployment Profiles

Status: Draft, operational gate target

This runbook defines regulated deployment profiles for SDK/RPC operators that require hardened
security posture, auditability, and predictable operational controls.

## Objectives

1. Provide pre-approved hardened configuration baselines.
2. Standardize evidence artifacts required for release and audits.
3. Keep compliance controls aligned with SDK contract semantics and CI gates.

## Profile: Regulated Baseline

Use when compliance controls are required but strict tenant isolation is not.

Required runtime controls:

- `bind_mode=remote` only with `auth_mode=token` or `auth_mode=mtls`
- token replay rejection enabled (`jti`)
- redaction enabled (`redaction.enabled=true`)
- event stream limits set to contract defaults or stricter
- `store_forward.capacity_policy` explicitly set
- key management uses `sdk.capability.key_management` with approved backend

Required operational controls:

- weekly key rotation procedure documented
- incident playbooks mapped to on-call escalation
- backup/restore drill evidence retained

## Profile: Regulated Strict

Use for highly controlled environments where strong identity guarantees and immutable evidence are
required.

Required runtime controls:

- `auth_mode=mtls` with trusted CA bundle and client cert validation
- `event_sink.enabled=true` with strict `allow_kinds` allowlist
- `event_sink.max_event_bytes` and queue limits pinned to risk-reviewed values
- `store_forward.max_message_age_ms` and `max_messages` explicitly bounded
- key management backend class `hsm` or approved `os_keystore`

Required operational controls:

- immutable audit log export enabled for auth/config/send/cancel paths
- security checklist PASS at or above canary floor
- release scorecard and canary criteria artifacts archived per release

## Audit Logging and Evidence

Release evidence bundle must include:

- `target/release-scorecard/release-scorecard.json`
- `target/release-readiness/canary-criteria-report.json`
- `target/release-readiness/generated-migration-notes.md`
- `target/supply-chain/sbom/cargo-metadata.sbom.json`
- `target/supply-chain/provenance/artifact-provenance.json`
- `target/supply-chain/reproducible/reproducible-build-report.txt`

Operational evidence must include:

- most recent security checklist (`docs/runbooks/security-review-checklist.md`)
- disaster recovery drill results (`docs/runbooks/disaster-recovery-drills.md`)
- incident response record for last P0/P1 drill

## Release Gate Mapping

Required release/CI gates for regulated profiles:

- `compliance-profile-check`
- `security-review-check`
- `unsafe-audit-check`
- `support-policy-check`
- `release-scorecard-check`
- `canary-criteria-check`
- `key-management-check`
- `supply-chain-check`
- `reproducible-build-check`

## Operational Checklist

- [ ] Compliance profile selected (`regulated-baseline` or `regulated-strict`)
- [ ] Auth mode and bind mode meet profile requirements
- [ ] Redaction policy verified in emitted events/errors
- [ ] Key management backend and fallback path validated
- [ ] Release evidence artifacts attached
- [ ] Security review checklist PASS floor met
- [ ] Backup/restore drill completed in current release window
