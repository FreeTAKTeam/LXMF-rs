# SDK Contract v2.5 Markers Domain

Status: Draft, Release B target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Capability IDs

1. `sdk.capability.markers`

## SDK Trait Surface

1. `marker_create`
2. `marker_list`
3. `marker_update_position`
4. `marker_delete`

## Core Types

1. `MarkerId`
2. `GeoPoint`
3. `MarkerCreateRequest`
4. `MarkerUpdatePositionRequest` (`expected_revision` required)
5. `MarkerDeleteRequest` (`expected_revision` required)
6. `MarkerRecord` (`revision` required)
7. `MarkerListRequest`
8. `MarkerListResult`

## Rules

1. Marker coordinates use WGS84 decimal degrees.
2. Marker writes are revision-CAS only. Update/delete requests must include `expected_revision`.
3. On revision mismatch, backend returns `SDK_RUNTIME_CONFLICT` with `details.expected_revision` and `details.observed_revision`.
4. Marker create starts at `revision=1`; successful write increments revision by exactly 1.
5. Marker list operations are cursor-based and deterministic for stable replay.
