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
4. `MarkerUpdatePositionRequest`
5. `MarkerRecord`
6. `MarkerListRequest`
7. `MarkerListResult`

## Rules

1. Marker coordinates use WGS84 decimal degrees.
2. Marker updates are last-write-wins unless backend advertises stronger consistency.
3. Marker list operations are cursor-based and deterministic for stable replay.
