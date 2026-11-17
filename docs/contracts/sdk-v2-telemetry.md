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

## R3AKT UAS / REM / RCH sensor application profile

See [R3S/1 sensor geometry and proposed SPI events](r3s-sensor-telemetry-spi-v1.md).
The application profile reuses LXMF numeric fields 2 (self Telemeter), 3
(independent object Telemeter stream), and 13 (typed application events).
Existing SID 128 sensor geometry is an R3AKT application extension, not a
new SDK transport method. Proposed SPI upsert/remove events live in field 13
and must be ignored safely by consumers that do not recognize them.

The SPI addition is a draft and is not evidence that the current SDK or daemon
provides an SPI domain. Any future typed SDK operation requires independent
capability negotiation, schema changes, fixtures and conformance tests.
