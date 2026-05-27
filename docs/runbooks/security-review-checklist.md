# Security Review Checklist (SDK v2.5)

Use this checklist for release-candidate security sign-off.

## Checklist

| Control | Status | Evidence |
| --- | --- | --- |
| Threat inventory is current and STRIDE-mapped | PASS | `docs/adr/0004-sdk-v25-threat-model.md` |
| Auth mode constraints enforce secure remote bind defaults | PASS | `sdk_security_authorize_http_request_*`, `config_rejects_remote_bind_without_token_or_mtls` |
| Token replay rejection (`jti`) active | PASS | `sdk_security_authorize_http_request_rejects_replayed_token_jti` |
| mTLS transport-context validation enforced where configured | PASS | `sdk_security_authorize_http_request_enforces_mtls_transport_context_and_policy` |
| Secret-bearing buffers are scrubbed after request construction and auth verification | PASS | `lxmf-sdk::backend::rpc::transport` zeroize path, `reticulum-rs-rpc` `sdk_auth_http` zeroizing secret verification |
| Sensitive fields redacted in events/errors/logs | PASS | `sdk_security_events_redact_sensitive_fields_by_default` |
| Rate limiting enforced for RPC auth attempts | PASS | `sdk_security_authorize_http_request_enforces_rate_limits_and_emits_event` |
| Event stream and RPC frame/body/header limits prevent unbounded growth | PASS | `sdk_event_queues_remain_bounded_under_sustained_load`, `sdk_poll_events_v2_rejects_oversized_*`, `decode_frame_rejects_oversized_payload_length_before_waiting_for_body`, `rpc_endpoint_rejects_oversized_content_length_without_overflow`, `request_parser_rejects_oversized_header_*`, `request_parser_rejects_too_many_headers`, `parse_content_length_rejects_invalid_or_conflicting_values`, `parse_http_response_body_rejects_oversized_*`, `parse_http_response_body_rejects_conflicting_content_lengths`, `rpc_response_reader_rejects_oversized_*_response`, `parse_request_log_meta_ignores_oversized_rpc_body_length` |
| Runtime command queues are bounded and surface pressure | PASS | `hot_apply_queue_is_bounded_and_reports_pressure`, `receipt_bridge_drops_when_bounded_queue_is_full`, `cargo run -p xtask -- security-review-check` `run_no_unbounded_runtime_channel_check` |
| Production async/runtime paths do not use blocking sleeps | PASS | `cargo run -p xtask -- security-review-check` `run_no_blocking_sleep_check` |
| Unsafe governance gate enforces inventory and invariant discipline | PASS | `cargo run -p xtask -- unsafe-audit-check`, `tools/scripts/check-unsafe.sh`, `docs/architecture/unsafe-inventory.md` |

## Gate

Run before release:

```bash
cargo run -p xtask -- security-review-check
cargo run -p xtask -- unsafe-audit-check
```
