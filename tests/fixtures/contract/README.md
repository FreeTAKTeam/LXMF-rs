# Contract Fixture Format (v0)

These fixtures are consumed by `tests/contract_harness.rs` and represent backend-neutral async contract scenarios.

## Fields

- `id`: Scenario identifier (for example `C01`).
- `description`: Human-readable scenario summary.
- `destination_hex`: 16-byte destination encoded as lowercase hex.
- `source_hex`: 16-byte source encoded as lowercase hex.
- `content`: Message content used by the test harness.
- `title`: Optional message title.
- `require_auth`: Optional bool. If true, runner enables auth-required policy.
- `allow_destination`: Optional bool. If true, runner allowlists `destination_hex`.
- `cancel_before_tick`: Optional bool. If true, runner calls `cancel()` before `tick()`.
- `run_tick`: Optional bool. If true, runner calls `tick(max_outbound)`.
- `max_outbound`: Optional integer. Defaults to 1.
- `expected_state_before_tick`: Optional normalized contract state asserted after `send()`.
- `expected_state_after_tick`: Optional normalized contract state asserted after `tick()`.
- `expect_cancel_result`: Optional bool asserted when `cancel_before_tick=true`.

## Notes

- Current harness executes only lane `L4` (Rust sender -> Rust receiver) and is structured for future lane expansion.
- The fixture format is intentionally narrow for `C01-C03`; extend as additional scenarios are implemented.
