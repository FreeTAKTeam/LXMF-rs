# Security Review Checklist (SDK v2.5)

Use this checklist for release-candidate security sign-off.

## Checklist

| Control | Status | Evidence |
| --- | --- | --- |
| Threat inventory is current and STRIDE-mapped | PASS | `docs/adr/0004-sdk-v25-threat-model.md` |
| Auth mode constraints enforce secure remote bind defaults | PASS | `sdk_security_authorize_http_request_*`, `config_rejects_remote_bind_without_token_or_mtls` |
| Token replay rejection (`jti`) active | PASS | `sdk_security_authorize_http_request_rejects_replayed_token_jti` |
| mTLS transport-context validation enforced where configured | PASS | `sdk_security_authorize_http_request_enforces_mtls_transport_context_and_policy` |
| Sensitive fields redacted in events/errors/logs | PASS | `sdk_security_events_redact_sensitive_fields_by_default` |
| Rate limiting enforced for RPC auth attempts | PASS | `sdk_security_authorize_http_request_enforces_rate_limits_and_emits_event` |
| Event stream limits and queue bounds prevent unbounded growth | PASS | `sdk_event_queues_remain_bounded_under_sustained_load`, `sdk_poll_events_v2_rejects_oversized_*` |

## Gate

Run before release:

```bash
cargo run -p xtask -- security-review-check
```
