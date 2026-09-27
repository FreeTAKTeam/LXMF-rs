# MeshChat interface specifications

Source: `liamcottle/reticulum-meshchat`, application version **2.4.0**, commit **df5aea94eab7f4be1cdfef494446e9d6979aed77** (15 August 2026). Audited 27 September 2026. This specifies the original application, not a fork and not an already implemented LXMF-rs API.

## Files

| File | Purpose |
|---|---|
| `meshchat-openapi.yaml` | OpenAPI 3.1.1: all 49 REST operations, two WebSocket upgrade operations and the explicit browser-entry route. |
| `meshchat-openapi.json` | Identical OpenAPI document in JSON. |
| `meshchat-websocket.protocol.json` | Connection rules, directions, audiences, correlation, errors, timing and binary audio encoding. A documented JSON protocol descriptor, not an AsyncAPI document. |
| `meshchat-websocket.schema.json` | Standalone JSON Schema 2020-12 for all four control commands and 15 server payload variants across 11 event type names. |
| `route-inventory.json` | Every explicit method/path and its pinned source location; also describes static asset serving. |
| `audit-notes.md` | Compatibility details, code defects, security limits and a bounded Rust adoption plan. |
| `contract-fixtures.json` | 32 synthetic positive/negative schema fixtures; not captured network traffic. |
| `validate_specs.py` | Offline schema, reference, example, fixture and optional source-coverage validator. |
| `validation-report.json` | Results and explicitly unperformed checks. Source audit includes response branches and frontend call sites when run with the pinned checkout. |
| `SHA256SUMS` | File integrity values, excluding this checksum file itself. |

## Using the specifications

Open the YAML or JSON document in an OpenAPI 3.1-capable editor. The upstream default HTTP origin is `http://127.0.0.1:8000`. The browser uses the same origin for REST, `/ws`, audio WebSockets and assets. Source references are attached through `x-source`, with immutable commit and line ranges. They refer to upstream implementation evidence, not this specification's line numbers.

For control-message validation, select `#/$defs/ClientMessage` or `#/$defs/ServerMessage` in `meshchat-websocket.schema.json`. The root accepts either direction. `meshchat-websocket.protocol.json` explains when each message appears. The call-audio socket carries **binary Protocol Buffers**, not JSON; its field numbers and enum values are recorded in the protocol descriptor.

Run `python3 validate_specs.py` from this directory with PyYAML and jsonschema available. For source coverage, add `--source-root /path/to/reticulum-meshchat-at-the-pinned-commit`. Optionally add `--oas-schema /path/to/official-openapi-3.1-schema.json`; the audited schema identifier is `https://spec.openapis.org/oas/3.1/schema/2022-10-07`. The validator does not fetch anything or start MeshChat.

This is a source-derived compatibility specification. It does not claim successful Rust integration. Some RNS/LXMF-owned values remain intentionally open because upstream dependencies are not exactly pinned. Input schemas describe normal client payloads; loose Python coercion and malformed-input behavior are documented separately. Do not treat the unauthenticated upstream interface as safe for public exposure.
