# R3S/1 sensor geometry and SPI over LXMF

Status: **Draft interoperable application profile** (15 November 2026). The FOV/pose part of R3S/1 is implemented in REM and RCH; the SPI event below is a **proposed additive extension and is not yet implemented**. No Reticulum routing, LXMF framing, encryption or base Telemeter change is required.

Consumers: R3AKT UAS/FreeTAKUAS, camera and binocular adapters, REM, RCH and other LXMF/Reticulum clients.

Sources of truth:
- [Original REM R3S/1 contract](https://github.com/FreeTAKTeam/reticulum_mobile_emergency_management/blob/main/docs/plans/R3AKT_Sensor_Telemetry_Extension_v1.md)
- [RCH implemented sensor contract](https://github.com/FreeTAKTeam/Reticulum-Community-Hub/blob/main/docs/r3s-telemetry.md)
- [RCH checked model and wire encoder](https://github.com/FreeTAKTeam/Reticulum-Community-Hub/blob/main/crates/r3akt-map-core/src/sensor_wire.rs)
- [RCH camera ground-point estimator](https://github.com/FreeTAKTeam/Reticulum-Community-Hub/blob/main/crates/r3akt-map-core/README.md#camera-ground-point-estimates)
- [MITRE CoT Sensor Schema v0.6](https://github.com/TAK-Product-Center/atak-civ/blob/main/takcot/mitre/CoT%20Sensor%20Schema%20%20(PUBLIC%20RELEASE).xsd)
- [ATAK-CIV sensor detail handler](https://github.com/TAK-Product-Center/atak-civ/blob/main/atak/ATAK/app/src/main/java/com/atakmap/android/cot/detail/SensorDetailHandler.java)

## 1. Intent and responsibility

A platform (UAS, device, vehicle, fixed camera) is an **origin** with a stable canonical R3AKT object hash or the sender's authenticated destination. Its sensor is a component of that origin, identified by sensorId. The FOV moves with the origin. An SPI (Sensor Point of Interest) is a geographically located **center-of-view or operator-designated point associated with one sensor**, not the origin's position and not proof that a target has been detected.

The original R3S/1 contract intentionally does not define a dedicated SPI packet. This draft supplies one without changing already deployed R3S/1 pose packets. The SPI shown in ATAK UAS screenshots may be a separate map point; that display alone does not establish a specific CoT/SPI wire encoding. A TAK adapter must be tested against actual ATAK UAS clients.

Ownership:
- R3AKT UAS adapter reads the aircraft and gimbal pose, rangefinder and any frame-center geographic coordinate, and timestamps them at source.
- REM signed plugin/native runtime validates grants, binds controller to the authorized origin, stores state and sends coalesced LXMF messages.
- RCH/REM receivers authenticate the writer, preserve source times, merge authorized state and expose native map projections.
- The shared r3akt-map-core library is the owner of checked sensor data, geometry, codec and storage primitives; UI code only renders projections.
- LXMF-rs transports fields and handles receipts/queuing. It MUST NOT decide geolocation accuracy, infer a camera footprint, or treat the relay as the drone's owner.

## 2. Existing R3S/1 sensor pose (unchanged)

The established numeric Telemeter SID 128 (0x80, **provisional**) contains the MessagePack array:

~~~
[1, [
  [sensorId, azimuthCd, elevationCd, rollCd, horizontalFovCd,
   verticalFovCd, slantRangeCm, imageNorthCd, sampleAtMs, rangeKind],
  ...
]]
~~~

Exactly ten values per sensor; 1 to 4 sensors per origin; numeric angles in centidegrees, distance in centimetres, unknown angles/distances as MessagePack nil. Encoded SID 128 size <= 512 bytes. sensorId is 1-32 printable ASCII bytes; sampleAtMs is source UTC milliseconds; rangeKind is 0 unknown, 1 measured-target, 2 display-limit. Unknown range requires nil; a configured map radius is NOT a measured target distance.

- azimuth: clockwise from true north, [0, 360) degrees; independently calibrated from aircraft heading.
- elevation: [-90, +90] degrees relative to horizon, negative toward ground.
- roll: (-180, +180] degrees.
- horizontalFov and verticalFov: full angular widths in [0, 360) degrees.
- slantRange: along the sensor optical axis to the center pixel or a labeled display limit, NOT horizontal range.
- imageNorth: direction of the north arrow **within the video/image**, NOT the sensor azimuth.

Encoding: LXMF field 2 (0x02, FIELD_TELEMETRY) for the authenticated sender's own origin; field 3 (0x03, FIELD_TELEMETRY_STREAM) for an independently identified drone/camera object. Field 3 reuses the existing [binaryOriginHash, epochSeconds, binaryPackedTelemeter, appearance] tuple. An optional exact, nullable position and metadata are sent in typed field-13 events. The sending relay's position MUST NOT replace the remote object's position.

Draw the existing ATAK-like 2D wedge with:

~~~
groundExtentM = slantRangeM * cos(elevationDeg * pi / 180)
leftBearing   = (azimuthDeg - horizontalFovDeg/2) mod 360
rightBearing  = (azimuthDeg + horizontalFovDeg/2) mod 360
~~~

This is a 2D display sector, NOT a terrain-intersected camera footprint. At nadir the 2D projection can collapse despite a real camera ground footprint. Lack of calibrated data MUST NOT be disguised as precise coverage.

## 3. Proposed SPI extension: typed field-13 events

**Do not allocate another Telemeter sensor ID** merely to carry SPI coordinates. Keep SID 128 byte-for-byte unchanged. Carry the SPI as a typed R3AKT application event inside LXMF field 13 (0x0D, FIELD_EVENT), alongside the existing R3S origin/manifest/removal events.

An origin has at most one *current* SPI per sensor. Its logical identity is (originId, sensorId). The SPI is a transient, linked map projection; it MUST NOT implicitly create a separately authoritative R3AKT marker or change the sensor's origin. An operator may explicitly save a target as an ordinary, independently permissioned marker through the existing marker contract.

### Upsert event

~~~
{
  "event_type": "mission.registry.sensor_spi.upsert",
  "payload": {
    "originId": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "sensorId": "eo-main",
    "point": {
      "lat": 38.8900,
      "lon": -77.0138,
      "altM": null,
      "altitudeDatum": "unknown"
    },
    "sampleAtMs": 1793440000000,
    "pointKind": "frame-center",
    "solution": "source-geolocation",
    "horizontalUncertaintyM": null
  }
}
~~~

This JSON is the **logical event representation**. On the network it is MessagePack-encoded in numeric LXMF field 13; do not send the illustrative JSON as an LXMF text body.

Fields:

| Field | Type / allowed values | Contract |
| --- | --- | --- |
| originId | 32 lowercase hex characters, canonical origin destination hash | REQUIRED, authenticated and writer-authorized |
| sensorId | 1-32 printable ASCII bytes | REQUIRED, same origin's sensor |
| point.lat, point.lon | finite WGS-84 degrees, lat [-90,90], lon [-180,180] | REQUIRED, do not fabricate coordinates |
| point.altM | finite metres or null | Optional; no sentinel/zero substitution |
| point.altitudeDatum | hae, msl, agl, unknown | REQUIRED; unknown if altitude is null |
| sampleAtMs | positive Unix UTC milliseconds | REQUIRED, the originating frame/solution sample time |
| pointKind | frame-center, designated | REQUIRED: center of optical frame vs explicitly selected point |
| solution | source-geolocation, operator-designated, flat-ground-estimate, terrain-intersection | REQUIRED: separates reported coordinates from estimates |
| horizontalUncertaintyM | finite nonnegative metres or null | Optional for directly reported points (unknown accuracy); REQUIRED for computed estimates |

For a frame-center SPI, a reported coordinate may come from a UAS metadata source that already determines the frame center. For a designated point, the sender MUST NOT call it the center optical-axis intersection unless it is. A solution is never called 'measured' merely because a non-null slant range exists. No receiver may synthesize an SPI from range alone.

### Removal event

~~~
{
  "event_type": "mission.registry.sensor_spi.remove",
  "payload": {
    "originId": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "sensorId": "eo-main",
    "deletedAtMs": 1793440010000
  }
}
~~~

Remove only the SPI, not its sensor or origin. Origin/sensor removal removes all dependent SPIs. An omitted SPI in a pose snapshot does not delete it; TTL expiration only removes it from the **live overlay**, with retained state governed by the existing bounded storage policy.

### Wire and state rules

1. Validate exact allowed fields and enum values, finite coordinates, time range, size and writer rights before accepting SPI. Null point is not an upsert: send the removal event.
2. Bound one event to <= 1024 MessagePack bytes, one current SPI per sensor, and no more than 4 live SPI bindings per origin. Respect existing field-13 and outer-message limits. Reject duplicate conflicting events for the same binding in a single packet.
3. Updates for each (originId, sensorId) are source-time monotonic. Identical equal-time retransmissions are idempotent; equal-time divergent values conflict; old upserts cannot resurrect a newer removal. Persist monotonic tombstones with the sensor lifecycle.
4. Accept a sensor-only or SPI-only update without moving the origin; do not invent a location if an authoritative origin fix is unavailable. Authenticate sender and resolve the origin/writer association **before** any projection or forwarding.
5. A moving origin's fix, sensor pose and SPI source times must be mutually coherent (initial tolerance <= 2 seconds) before showing a connected line, precise footprint or live camera target. Independent SPI may be stored but shown as unsynchronized/stale, not fused inaccurately.
6. The native sensor TTL defaults to 300 seconds in RCH; do not reset sample age to receipt time. Expired or unsynchronized SPIs MUST NOT appear as current detections. A missing uncertainty measure is visibly 'accuracy unknown', never implicitly 0 metres.
7. The SPI field is an optional extension: receivers unaware of these event types ignore them without rejecting ordinary position or valid SID-128 FOV. Preserve original packed binary telemetry where possible. Old Sideband repacking can drop SID 128 and does not guarantee SPI retention.
8. Do not publish camera/gimbal/SPI data to unauthorized peer/team subscribers. Check current rights at admission and retry; broker-admitted retries cannot currently be forcibly revoked by RCH and remain a release gate.
9. No image, video bytes, credentials, signed video URLs, arbitrary user text or executable actions belong in the SPI event.

## 4. SPI inference and display

Precedence is **source-supplied SPI**, then (only when explicitly enabled and calibrated) **computed estimate**. The existing RCH camera_ground::ground_point method can estimate a ray-to-flat-plane intersection if it receives compatible known altitude datum, a chosen ground plane, calibration, bounded motion/time error and sufficient sensor pose. Terrain-intersection requires a separate elevation model and uncertainty evaluation; the protocol makes no claim that RCH or REM currently computes it.

The map renders:
- An origin icon at the authoritative drone/camera coordinate.
- The existing translucent FOV wedge or direction arrow at the origin.
- A crosshair at the accepted SPI coordinate, visibly joined to its origin/sensor, with pointKind, solution, source age and uncertainty status available on selection.
- A sector and/or crosshair only when the underlying data permit it. They are not the same entity, and a sensor may have a valid FOV without a valid SPI.

The SPI can be outside the displayed wedge for an operator-designated off-center image point. The receiver MUST NOT silently move the SPI into the wedge.

## 5. Producer/consumer contracts

| Component | Required work for this draft |
| --- | --- |
| R3AKT UAS adapter | Convert DJI/vendor heading and gimbal angles to true geodetic angles; publish aircraft origin and SID 128; forward *reported* frame center/selected point with correct pointKind/solution; never synthesize coordinates from absent GPS, pose or terrain |
| REM plugin 1.4 host | Add an independently grantable sensor SPI update/remove operation; validate source and authorized origin; coalesce SPI updates with existing low-rate telemetry publication and use native persistence |
| r3akt-map-core (RCH shared crate) | Add strict typed SensorSpiUpsert / SensorSpiRemove decoding, source-time/tombstone validation, bounded SQLite state and a projection separate from SensorState and SensorManifest |
| REM native and MapLibre UI | Expose SPI state and draw crosshair/link with stale/accuracy status; do not duplicate wire parsing in Vue/TypeScript |
| RCH northbound API / map | Return SPI in sensor projections, add explicit authorization and adapter input, and render on existing map; preserve old GET/POST /Sensors consumers through optional additive fields or versioned routes |
| LXMF-rs | Keep ordinary transport, numeric fields, delivery receipts, collector compatibility and unknown-field preservation; no core wire protocol change |

Transport cadence is policy, not a message-format rule: REM currently coalesces at roughly 1 Hz over TCP, 30 s over RNode/unknown paths, 300 s in power-saver. SPI must share that budget; it is NOT a video-frame update channel.

## 6. ATAK / CoT boundary

The ATAK-CIV sensor detail schema describes a cone on the *parent* track (azimuth, elevation, roll, fov, vfov, slant range, image north). An SPI must be exported as a **separate point representation** when the TAK adapter and recipient support it. Some TAK/KLV bridges use CoT type 'b-m-p-s-p-i' and a parent link relation 'p-p'; these details require actual ATAK UAS interoperability testing before being treated as a normative adapter contract. Do not mistake a generic detection marker for a camera SPI.

Inbound TAK adapter: resolve durable CoT UID -> authorized R3AKT origin, preserve sensor origin/source time and separate the parent position from the SPI point. Reject stale, spoofed or unbound updates. Keep video endpoints unapproved until explicitly authorized.

Outbound TAK adapter: export one selected parent sensor as in existing R3S/1, and optionally a separate linked SPI CoT point with stable UID and measured/unknown CE and LE metadata. If horizontalUncertaintyM is unknown, do NOT claim a precise CE. Never export a flat-ground estimate as physically validated terrain geolocation.

## 7. Conformance tests required before implementation release

| Test | Expected result |
| --- | --- |
| Existing SID-128 fixture and Sideband/Columba/Pyxis ordinary Telemeter | No changed byte encoding, speed/position units or regression |
| UAS A controlled by REM B | FOV and SPI stay attached to A; B does not move to A or to the SPI |
| Cross-0-degree FOV, negative elevation, nadir, antimeridian | Correct sector handling or deliberate non-render; SPI coordinate independent of wedge |
| Reported frame-center, operator point, computed flat estimate | Distinct pointKind/solution; unknown accuracy clearly displayed |
| Mismatched source times, stale origin/pose/SPI | No false connected live target; retain source timestamps |
| Unauthorized relay, object takeover, forbidden team, replay after delete | Reject, no state or metadata leakage; persistent tombstones |
| Malformed coordinates, NaN/infinite values, invalid enum, duplicate sensor IDs, oversized events | SPI rejected without poisoning ordinary parent fix/sensor update |
| Partial updates, restart and collector reply | SPI associated with correct origin/sensor; source revision and tombstone survive |
| Route through old client which discards unknown fields | Ordinary position remains usable; loss of FOV/SPI is explicitly detectable/allowed |
| Synthetic UAS adapter -> REM -> REM/RCH over TCP, RNode/LoRa | Live source identity, TTL, size, cadence and delivery measured; no assertion of qualification without actual tests |
| ATAK UAS CoT import/export | Separate parent track, sensor FOV and linked SPI visually verified on supported ATAK version |

## 8. Status / release gates

Existing: R3S/1 cone/pose format, ownership and projection exist in REM/RCH. Proposed: new SPI upsert/remove events, persistence, plugin APIs, RCH northbound projection and ATAK/UAS SPI adapter. This document is a review target, **not evidence of implementation**.

Before adopting this extension as an active LXMF-rs registry entry: coordinate provisional SID 128 allocation; add shared reference codecs and golden MessagePack fixtures; implement REM/RCH access control and deletion semantics; test physical UAS and radio paths; resolve RCH broker cancellation/re-authorization limitations. Keep the SDK v2 telemetry methods and schema backward compatible; do not claim that this changes native LXMF or Python Reticulum specifications.
