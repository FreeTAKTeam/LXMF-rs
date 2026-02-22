# SDK Contract v2.5 Telemetry Domain

Status: Draft, Release B target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Capability IDs

1. `sdk.capability.telemetry_query`
2. `sdk.capability.telemetry_stream`

## SDK Trait Surface

1. `telemetry_query`
2. `telemetry_subscribe`

## Core Types

1. `TelemetryQuery`
2. `TelemetryPoint`

## Rules

1. Query windows are expressed by optional `from_ts_ms` and `to_ts_ms`.
2. Query and stream payloads must preserve unknown extension fields.
3. Stream subscriptions are capability-gated and may be emulated by poll-backed adapters.
