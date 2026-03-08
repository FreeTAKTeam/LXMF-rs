# SDK Easy-Mode Errors v1

Status: Draft, implementation target  
Contract family: `sdk-easy`  
Contract release: `v1`

## Purpose

This document defines the typed error model for easy-mode consumers.

Apps should not need to inspect low-level transport or runtime internals to decide what happened or what to do next.

## Error Envelope

All easy-mode errors must expose:

- `code`
- `category`
- `retryable`
- `terminal`
- `user_action_required`
- `message`
- `details`
- `cause_code` optional

Rules:

1. Unknown future errors must parse safely as `Unknown(code)`.
2. Reusing an existing error code with different semantics is a breaking change.
3. Error messages are descriptive only; behavior must be driven by typed fields.

## Categories

Required categories:

- `Validation`
- `Capability`
- `Config`
- `Policy`
- `Delivery`
- `Connectivity`
- `Persistence`
- `Security`
- `Timeout`
- `Runtime`
- `Internal`

## Required Easy-Mode Codes

Minimum default set:

- `EASY_VALIDATION_INVALID_ARGUMENT`
- `EASY_VALIDATION_UNKNOWN_FIELD`
- `EASY_CAPABILITY_UNSUPPORTED_PROFILE`
- `EASY_CAPABILITY_REQUIRED_FEATURE_MISSING`
- `EASY_CONFIG_INVALID`
- `EASY_RUNTIME_INVALID_STATE`
- `EASY_RUNTIME_ALREADY_RUNNING_DIFFERENT_CONFIG`
- `EASY_RUNTIME_STREAM_DEGRADED`
- `EASY_RUNTIME_NOT_STARTED`
- `EASY_DELIVERY_QUEUE_PRESSURE`
- `EASY_DELIVERY_PARTIAL_ACCEPTANCE`
- `EASY_DELIVERY_RETRY_EXHAUSTED`
- `EASY_DELIVERY_CANCELLED`
- `EASY_CONNECTIVITY_DISCONNECTED`
- `EASY_CONNECTIVITY_RECONNECT_FAILED`
- `EASY_PERSISTENCE_UNAVAILABLE`
- `EASY_PERSISTENCE_RECOVERY_REQUIRED`
- `EASY_TIMEOUT_OPERATION_EXPIRED`
- `EASY_SECURITY_AUTH_REQUIRED`
- `EASY_SECURITY_AUTHZ_DENIED`
- `EASY_SECURITY_REDACTION_REQUIRED`
- `EASY_INTERNAL_UNEXPECTED_FAILURE`

## Retryability Rules

`retryable` is contract-governed.

Rules:

1. The same error code must not be `retryable=true` in one wrapper and `false` in another unless the profile explicitly declares that difference.
2. Retryability reflects default SDK policy, not merely possibility in theory.
3. If SDK policy will retry automatically, the error should usually be surfaced as an event or non-terminal status rather than an immediate terminal send failure.

## Terminality Rules

`terminal` refers to the current operation or delivery item, not necessarily the whole runtime.

Rules:

1. `EASY_DELIVERY_RETRY_EXHAUSTED` is terminal for that delivery item.
2. `EASY_DELIVERY_QUEUE_PRESSURE` is not inherently terminal unless admission policy says fail-fast.
3. `EASY_RUNTIME_STREAM_DEGRADED` is terminal for the affected subscription state until explicit recovery.
4. `EASY_INTERNAL_UNEXPECTED_FAILURE` may be terminal for the runtime session.

## User-Actionable Rules

Apps often need to know whether to surface UI or let the SDK continue.

Rules:

1. `user_action_required=true` only when the caller or user must do something meaningful:
   - re-authenticate
   - change config
   - free capacity
   - explicitly recover stream state
2. Automatic retries and reconnects should not be surfaced as user-actionable errors by default.

## Queue Pressure and Partial Acceptance

Easy mode must make fanout and pressure semantics explicit.

Rules:

1. Queue pressure must map to `EASY_DELIVERY_QUEUE_PRESSURE`.
2. Partial acceptance must map to `EASY_DELIVERY_PARTIAL_ACCEPTANCE`.
3. Partial success must never be hidden in a generic `Ok`.
4. If a profile promises all-or-nothing admission for a helper, partial acceptance must not occur for that helper.

## Recovery-Oriented Errors

The following errors require explicit recovery logic, whether automatic or user-triggered:

- `EASY_RUNTIME_STREAM_DEGRADED`
- `EASY_PERSISTENCE_RECOVERY_REQUIRED`
- `EASY_CONNECTIVITY_RECONNECT_FAILED`

Rules:

1. Recovery semantics must be profile-defined.
2. Wrappers must not improvise different recovery behavior without a declared profile difference.

## Security and Redaction

Rules:

1. Secrets must never appear in `message` or `details`.
2. Sensitive identifiers must be redacted or transformed according to SDK policy.
3. Raw payload or credential excerpts are forbidden in default easy-mode errors.
