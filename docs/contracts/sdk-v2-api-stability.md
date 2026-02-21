# SDK API Stability Classes

Status: Active, CI-enforced by `sdk-api-break`

This registry classifies the `lxmf-sdk` public API into lifecycle classes.
`cargo xtask sdk-api-break` enforces that every public API line matches one rule.

Rule matching is ordered top-to-bottom (first match wins).

## Stability Classes

| Class | Meaning |
| --- | --- |
| `stable` | Covered by `v2.x` compatibility expectations and migration workflow. |
| `experimental` | Usable in production with caution; additive changes may occur before promotion to stable. |
| `internal` | Public for composition/testing/runtime wiring but not a compatibility guarantee for embedders. |

## Classification Rules

| Class | Match Prefix | Lifecycle Rule |
| --- | --- | --- |
| `internal` | `lxmf_sdk::SdkBackend` | May evolve on any release with migration notes. |
| `internal` | `lxmf_sdk::RpcBackendClient` | Backend wiring surface, not host-stable API. |
| `internal` | `lxmf_sdk::Client` | Concrete generic wrapper; behavior-oriented use only. |
| `experimental` | `lxmf_sdk::LxmfSdkTopics` | Extension trait, additive/shape changes allowed with release notes. |
| `experimental` | `lxmf_sdk::LxmfSdkTelemetry` | Extension trait, additive/shape changes allowed with release notes. |
| `experimental` | `lxmf_sdk::LxmfSdkAttachments` | Extension trait, additive/shape changes allowed with release notes. |
| `experimental` | `lxmf_sdk::LxmfSdkMarkers` | Extension trait, additive/shape changes allowed with release notes. |
| `experimental` | `lxmf_sdk::LxmfSdkIdentity` | Extension trait, additive/shape changes allowed with release notes. |
| `experimental` | `lxmf_sdk::LxmfSdkPaper` | Extension trait, additive/shape changes allowed with release notes. |
| `experimental` | `lxmf_sdk::LxmfSdkRemoteCommands` | Extension trait, additive/shape changes allowed with release notes. |
| `experimental` | `lxmf_sdk::LxmfSdkVoiceSignaling` | Extension trait, additive/shape changes allowed with release notes. |
| `stable` | `lxmf_sdk::` | Default class for all remaining SDK public symbols. |

## Deprecation Workflow

1. `stable -> deprecated` requires migration note updates and replacement guidance.
2. `experimental -> stable` requires contract + conformance evidence.
3. `internal` items may change without deprecation, but significant behavior shifts must be documented.
4. Class changes must update this file in the same change set as API surface changes.
