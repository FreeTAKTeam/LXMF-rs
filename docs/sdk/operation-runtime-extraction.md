# SDK Operation Runtime Extraction

This guide describes the shared operation-runtime boundary used by product
clients such as `R3AKTClient`.

The checked R3AKT catalog fixture is
`docs/fixtures/sdk-operation-runtime/r3akt.catalog.json`. Product clients should
load or generate catalog entries in that shape and pass them as
`custom_operations` during SDK startup.

## Ownership

LXMF-rs owns:

- operation registry construction and alias canonicalization
- envelope validation and query/command kind checks
- daemon dispatch through `sdk_envelope_execute_v2`
- typed event and error envelopes
- startup propagation of `custom_operations`

Product repos own:

- product catalog entries
- payload schemas and product command semantics
- domain-specific event typing
- UI policy and workflow state

`R3AKTClient` should therefore shrink to product-specific behavior: it should
provide the R3AKT catalog and payload models, then call the shared SDK runtime
instead of reimplementing registry lookup, alias normalization, envelope
dispatch, event cursor handling, or generic error mapping.

## Runtime Flow

1. Build an SDK app `Config`.
2. Add product entries from
   `docs/fixtures/sdk-operation-runtime/r3akt.catalog.json` as
   `custom_operations`.
3. Start the SDK runtime. The startup request serializes the custom catalog into
   `SdkConfig.extensions.custom_operations`.
4. `reticulumd` installs the catalog into its operation registry during
   `sdk_negotiate_v2`.
5. Product callers execute canonical operation IDs or legacy aliases through
   `sdk_envelope_execute_v2`.

The shared runtime must resolve aliases such as `R3AKT;EMergencyMessages.send`
to canonical IDs such as `r3akt.message.send` before dispatching.
