# Disaster Recovery Drills

Status: active  
Scope: durable store backup, restore validation, and migration rollback readiness

## Objectives

1. Prove that a backup can restore SDK domain snapshot + message state.
2. Detect post-backup drift and confirm it is removed after restore.
3. Verify release rollback instructions are actionable.

## Drill Cadence

- Minimum: once per release candidate.
- Recommended: weekly in staging.
- Trigger immediately after any storage schema or migration change.

## Preconditions

1. Workspace is clean enough to run tests.
2. Rust toolchain is installed.
3. Database file paths are writable in local temp storage.

## Automated Drill

Run the backup/restore drill script:

```bash
./tools/scripts/backup-restore-drill.sh
```

The drill runs:

- `sdk_backup_restore_drill_recovers_snapshot_and_messages`

The scenario intentionally:

1. seeds baseline state,
2. takes a backup,
3. introduces drift data,
4. restores from backup,
5. verifies baseline state remains and drift state is absent.

## Migration Rollback Readiness

For each release candidate:

1. Identify migration impact in `docs/migrations/`.
2. Record rollback plan in RC notes.
3. Confirm backup restore drill passes before release cut.
4. Confirm replay fixture still executes:

```bash
cargo run -p rns-tools --bin rnx -- replay --trace docs/fixtures/sdk-v2/rpc/replay_known_send_cancel.v1.json
```

## Evidence to Attach

- output from `backup-restore-drill.sh`
- commit hash and branch
- runtime profile used
- operator performing drill
- timestamp (UTC)
