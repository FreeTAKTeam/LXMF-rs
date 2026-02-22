# SDK Contract v2.5 (Errors)

Status: Draft, implementation target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Error Envelope

All SDK errors must expose:

- `machine_code`
- `category`
- `retryable`
- `is_user_actionable`
- `message`
- `details`
- `cause_code` optional

`details` contract:

- JSON object only (`map<string, scalar|array|object>`)
- keys must be snake_case
- values must be redacted according to event/error redaction policy
- recommended keys: `field`, `expected`, `observed`, `limit_name`, `limit_value`, `cursor_scope`, `config_revision`

`machine_code` format:

- `SDK_<CATEGORY>_<NAME>`

## RPC Transport Compatibility

`rns-rpc` responses keep legacy `code`/`message` fields for wire compatibility and may include additive envelope fields (`machine_code`, `category`, `retryable`, `is_user_actionable`, `details`, `cause_code`, `extensions`).

## Categories

- `Validation`
- `Capability`
- `Config`
- `Policy`
- `Transport`
- `Storage`
- `Crypto`
- `Timeout`
- `Runtime`
- `Security`
- `Internal`

## Stability Rules

1. Existing error code semantics are contract-governed.
2. Reusing a code for a different meaning is a breaking change.
3. Removing a widely used code is a major-version change.
4. Unknown future codes must parse as `Unknown(code_string)`.

## Required Runtime Codes (Minimum Set)

- `SDK_CAPABILITY_CONTRACT_INCOMPATIBLE`
- `SDK_RUNTIME_INVALID_STATE`
- `SDK_RUNTIME_ALREADY_RUNNING_WITH_DIFFERENT_CONFIG`
- `SDK_RUNTIME_ALREADY_TERMINAL`
- `SDK_RUNTIME_INVALID_CURSOR`
- `SDK_RUNTIME_CURSOR_EXPIRED`
- `SDK_RUNTIME_STREAM_DEGRADED`
- `SDK_RUNTIME_CONFLICT`
- `SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED`
- `SDK_VALIDATION_IDEMPOTENCY_CONFLICT`
- `SDK_VALIDATION_UNKNOWN_FIELD`
- `SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED`
- `SDK_VALIDATION_EVENT_TOO_LARGE`
- `SDK_VALIDATION_BATCH_TOO_LARGE`
- `SDK_VALIDATION_MAX_EXTENSION_KEYS_EXCEEDED`
- `SDK_CONFIG_CONFLICT`
- `SDK_CONFIG_UNKNOWN_KEY`
- `SDK_CAPABILITY_DISABLED`

## Security-Oriented Codes (Minimum Set)

- `SDK_SECURITY_AUTH_REQUIRED`
- `SDK_SECURITY_AUTHZ_DENIED`
- `SDK_SECURITY_TOKEN_INVALID`
- `SDK_SECURITY_TOKEN_REPLAYED`
- `SDK_SECURITY_RATE_LIMITED`
- `SDK_SECURITY_REMOTE_BIND_DISALLOWED`
- `SDK_SECURITY_REDACTION_REQUIRED`

## Error Redaction Rules

1. Errors must not contain secrets in `message` or `details`.
2. Sensitive identifiers must be transformed per redaction policy.
3. Raw transport payload excerpts are disallowed in production error strings by default.
