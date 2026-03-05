# Failure Injection Matrix

Status: draft
Owners: SDK + runtime QA maintainers

## Scope

Defines deterministic failure scenarios, expected machine code outcomes, and CI gating.

## Required Matrix

| Scenario | Expected machine code | Notes |
|---|---|---|
| offset_mismatch | SDK_RUNTIME_INVALID_CURSOR | upload chunk offset != next_offset |
| unknown_upload_id | SDK_RUNTIME_NOT_FOUND | upload session missing |
| commit_incomplete | SDK_VALIDATION_INVALID_ARGUMENT | commit before full upload |
| checksum_mismatch | SDK_VALIDATION_CHECKSUM_MISMATCH | commit hash mismatch |
| duplicate_chunk_same_bytes | NONE | idempotent success |
| duplicate_chunk_conflict | SDK_VALIDATION_IDEMPOTENCY_CONFLICT | same offset, different bytes |
| seq_gap | SDK_RUNTIME_SEQ_GAP | receiver observed seq > expected |
| forced_disconnect_mid_transfer | SDK_RUNTIME_DISCONNECTED | disconnect during active transfer |
| queue_timeout_exhausted | SDK_RUNTIME_BACKPRESSURE_TIMEOUT | retries exhausted due tx timeout |

## Test Artifact Requirement

Each matrix row must be represented in repo by:

- `test_id`
- fixture path
- asserted machine code
- CI job name

No row may be marked complete without all four artifact links.
