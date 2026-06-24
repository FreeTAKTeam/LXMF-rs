# Two-Phone reticulumd HIL

This runbook executes the phone-only field test for `reticulumd` with:

- S8 running Sideband
- Pixel running Columba
- ADB reverse as the only phone-to-host transport

The harness records evidence under `target/phone-hil/<timestamp>/` and writes a
machine-readable `report.json`.

## Preconditions

- Both phones are visible in `adb devices -l`.
- Sideband on the S8 and Columba on the Pixel are installed and ready.
- Both phone apps are configured with a Reticulum TCP client interface pointing
  at their device-local ADB reverse port. By default that is
  `127.0.0.1:37429` on both phones, but set `SIDE_BAND_REVERSE_PORT` or
  `PIXEL_REVERSE_PORT` when a phone needs a different device-local port.
  Every reverse still forwards into the host daemon transport port.
- Read the phone LXMF destination hashes from the apps before the full run:
  - `SIDE_BAND_HASH`: Sideband destination hash on the S8
  - `COLUMBA_HASH`: Columba destination hash on the Pixel

Current harness automation cannot extract those hashes from the phone UIs.
When both phones are already connected and announcing, set
`AUTO_DISCOVER_PHONE_HASHES=1` to populate missing phone hashes from
`list_peers` before the delivery matrix starts.

## Run

Preflight only:

```bash
bash tools/scripts/phone-reticulumd-hil.sh --preflight-only
```

Full non-interactive run:

```bash
SIDE_BAND_HASH=<32-hex-sideband-hash> \
COLUMBA_HASH=<32-hex-columba-hash> \
bash tools/scripts/phone-reticulumd-hil.sh
```

Full run with manual phone-visible confirmations and screenshots:

```bash
SIDE_BAND_HASH=<32-hex-sideband-hash> \
COLUMBA_HASH=<32-hex-columba-hash> \
bash tools/scripts/phone-reticulumd-hil.sh --interactive
```

Useful overrides:

```bash
SIDE_BAND_SERIAL=988b9b344135304639 \
PIXEL_SERIAL=<pixel-adb-serial> \
SIDE_BAND_REVERSE_PORT=37429 \
PIXEL_REVERSE_PORT=37430 \
AUTO_DISCOVER_PHONE_HASHES=1 \
BURST_COUNT=25 \
PER_PEER_IN_FLIGHT=1 \
LARGE_BYTES=4096 \
PHONE_PEER_WAIT_SECS=180 \
MANUAL_WAIT_SECS=180 \
bash tools/scripts/phone-reticulumd-hil.sh --interactive
```

## What The Harness Does

1. Fails fast unless the S8/Sideband and Pixel/Columba devices are visible to
   ADB.
2. Builds `reticulumd` and `lxmf-cli`.
3. Starts phone logcat capture, configures ADB reverse, and starts `reticulumd`
   with `RUST_LOG=reticulumd=trace,reticulum_rs_transport=trace`, TCP transport, and propagation-node config.
4. Captures the daemon delivery and propagation destination hashes from
   `reticulumd.log`. Use the propagation hash, not the delivery hash, when
   setting Sideband's propagation node.
5. Waits until `list_peers` shows both phone LXMF destination hashes. If one
   phone has not announced, the run is blocked before delivery tests begin and
   peer-readiness evidence is written under `phone-peer-readiness/`.
6. Sends daemon-originated packet and resource-sized messages to both phone
   destinations.
7. Captures `status`, `poll`, `snapshot`, `sdk_status_v2`,
   `sdk_snapshot_v2`, `list_messages`, `list_peers`, and
   `message_delivery_trace` evidence for each daemon-originated message.
8. Exercises failed delivery, queue burst, reverse-removal recovery,
   propagation-node status/maintenance, outbound propagation cost lookup, and
   phone-only capability checks.
   Burst runs default `LXMD_DELIVERY_PER_PEER_IN_FLIGHT=1` so queue and retry
   evidence reflects one active delivery per peer.
9. Records every item as `pass`, `fail`, or `unsupported-by-phone-app` in
   `report.json`.

## Manual Evidence

The phone-only constraint means the harness cannot drive Sideband or Columba UI
actions directly. In `--interactive` mode it prompts the operator to:

- confirm daemon-to-S8 and daemon-to-Pixel app-visible messages
- send S8-to-daemon and Pixel-to-daemon messages
- send S8-to-Pixel and Pixel-to-S8 messages

When confirmed, the harness captures screenshots with `adb exec-out screencap`.
Without `--interactive`, those manual phone-visible checks are recorded as
`fail` rather than silently skipped.

## Capability Gaps

`Link.request()` and `Channel` coverage depend on phone-app behavior that the
normal Sideband and Columba UI paths may not expose.

- If a phone-announced propagation/control destination is visible, the harness
  records Link.request coverage as `pass` with `list_propagation_nodes`
  evidence.
- If no phone-announced propagation/control destination is visible, it records
  `link_request_phone_capability` as `unsupported-by-phone-app`.
- `channel_reliable_delivery` is recorded as `unsupported-by-phone-app` unless
  the operator provides a phone-app channel-capable path and sets
  `PHONE_CHANNEL_CONFIRMED=1`.

The overall result is `pass` only when every recorded check is `pass`. Any
`fail` or `unsupported-by-phone-app` item makes the run fail, matching the
field-test acceptance criteria.
