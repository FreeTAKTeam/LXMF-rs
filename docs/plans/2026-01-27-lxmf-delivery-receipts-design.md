# LXMF Delivery Receipts Design (2026-01-27)

## Goal
Implement delivery receipts in LXMF-rs and reticulum-daemon, surfaced in Weft UI. Behavior must interoperate with Python LXMF nodes.

## Requirements
- Receipt is an LXMF message that references the original message id.
- Receipt is sent by receiver upon successful message ingestion.
- Sender updates `receipt_status` to `delivered` when receipt is received.
- No read receipts in this scope.
- Idempotent updates for multiple receipts.

## Data Flow
Sender:
1) Weft -> daemon `SendMessage`.
2) Daemon sends LXMF wire message via Reticulum.
3) Weft shows `sending` until receipt.
4) Receipt arrives -> daemon emits `receipt` event -> Weft updates `receipt_status=delivered`.

Receiver:
1) Reticulum receives LXMF wire message.
2) LXMF-rs decodes and stores message.
3) LXMF-rs constructs delivery receipt and sends to sender identity.

## Receipt Format
- Use LXMF `fields` with a reserved receipt field id.
- Fields payload: `[original_message_id, timestamp, status]`.
- `status` uses a small enum; in this scope only `delivered`.
- Must match Python behavior (confirm reserved field id and encoding).

## Storage + Events
- Update message record with `receipt_status`.
- Emit daemon event: `receipt { message_id, status, timestamp }`.
- Messages list query should include `receipt_status` once updated.

## Error Handling
- Unknown message id: log + ignore.
- Malformed receipt: log + drop.
- Receipt send failure: log + retry opportunistically (no UI loop).

## Testing
- Unit tests: encode/decode receipt fields.
- Integration: Weft -> Python receiver -> receipt -> Weft `delivered`.
- If no harness, provide a scripted manual test run.
