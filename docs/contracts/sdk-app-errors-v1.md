# SDK App Errors v1

Status: Draft, implementation target  
Contract family: `sdk-app`  
Contract release: `v1`

## Purpose

This document defines the typed error model for app-api consumers.

Apps should not need to inspect low-level transport or runtime internals to decide what happened or what to do next.

## Error Envelope

All app-api errors must expose:

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

## Required App Error Codes

Minimum default set:

- `SDK_APP_VALIDATION_INVALID_ARGUMENT`
- `SDK_APP_VALIDATION_UNKNOWN_FIELD`
- `SDK_APP_CAPABILITY_UNSUPPORTED_PROFILE`
- `SDK_APP_CAPABILITY_REQUIRED_FEATURE_MISSING`
- `SDK_APP_CONFIG_INVALID`
- `SDK_APP_RUNTIME_INVALID_STATE`
- `SDK_APP_RUNTIME_ALREADY_RUNNING_DIFFERENT_CONFIG`
- `SDK_APP_RUNTIME_STREAM_DEGRADED`
- `SDK_APP_RUNTIME_NOT_STARTED`
- `SDK_APP_DELIVERY_QUEUE_PRESSURE`
- `SDK_APP_DELIVERY_PARTIAL_ACCEPTANCE`
- `SDK_APP_DELIVERY_RETRY_EXHAUSTED`
- `SDK_APP_DELIVERY_CANCELLED`
- `SDK_APP_CONNECTIVITY_DISCONNECTED`
- `SDK_APP_CONNECTIVITY_RECONNECT_FAILED`
- `SDK_APP_PERSISTENCE_UNAVAILABLE`
- `SDK_APP_PERSISTENCE_RECOVERY_REQUIRED`
- `SDK_APP_TIMEOUT_OPERATION_EXPIRED`
- `SDK_APP_SECURITY_AUTH_REQUIRED`
- `SDK_APP_SECURITY_AUTHZ_DENIED`
- `SDK_APP_SECURITY_REDACTION_REQUIRED`
- `SDK_APP_INTERNAL_UNEXPECTED_FAILURE`

## Retryability Rules

`retryable` is contract-governed.

Rules:

1. The same error code must not be `retryable=true` in one wrapper and `false` in another unless the profile explicitly declares that difference.
2. Retryability reflects default SDK policy, not merely possibility in theory.
3. If SDK policy will retry automatically, the error should usually be surfaced as an event or non-terminal status rather than an immediate terminal send failure.

## Terminality Rules

`terminal` refers to the current operation or delivery item, not necessarily the whole runtime.

Rules:

1. `SDK_APP_DELIVERY_RETRY_EXHAUSTED` is terminal for that delivery item.
2. `SDK_APP_DELIVERY_QUEUE_PRESSURE` is not inherently terminal unless admission policy says fail-fast.
3. `SDK_APP_RUNTIME_STREAM_DEGRADED` is terminal for the affected subscription state until explicit recovery.
4. `SDK_APP_INTERNAL_UNEXPECTED_FAILURE` may be terminal for the runtime session.

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

App API must make fanout and pressure semantics explicit.

Rules:

1. Queue pressure must map to `SDK_APP_DELIVERY_QUEUE_PRESSURE`.
2. Partial acceptance must map to `SDK_APP_DELIVERY_PARTIAL_ACCEPTANCE`.
3. Partial success must never be hidden in a generic `Ok`.
4. If a profile promises all-or-nothing admission for a helper, partial acceptance must not occur for that helper.

## Recovery-Oriented Errors

The following errors require explicit recovery logic, whether automatic or user-triggered:

- `SDK_APP_RUNTIME_STREAM_DEGRADED`
- `SDK_APP_PERSISTENCE_RECOVERY_REQUIRED`
- `SDK_APP_CONNECTIVITY_RECONNECT_FAILED`

Rules:

1. Recovery semantics must be profile-defined.
2. Wrappers must not improvise different recovery behavior without a declared profile difference.

## Security and Redaction

Rules:

1. Secrets must never appear in `message` or `details`.
2. Sensitive identifiers must be redacted or transformed according to SDK policy.
3. Raw payload or credential excerpts are forbidden in default app-api errors.
