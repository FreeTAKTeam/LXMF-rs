# Real-world interoperability evidence

Last updated: 2026-09-26

LXMF-rs has been exercised with real applications, real phones, and physical
RNode/LoRa links. The open operational issues track breadth, repeatability, and
release-grade evidence. They must not be read as a statement that LXMF-rs has
only been tested with simulations.

Software parity and operational evidence are separate. A field test proves the
specific application, carrier, hardware, and workflow that was exercised. It
does not automatically prove every device, interface, operating system, or
long-running network scenario.

## Published and retained evidence

| Date / source | Stack and carrier | What was demonstrated | Boundary |
| --- | --- | --- | --- |
| REM 1.4 | REM on POCO + Pixel, physical RNodes, 915 MHz LoRa; LXMF-rs revision `56ea9f06c474426b2245739e9cb5e2c325cdb1e2` | Bidirectional announces, Links, small LXMF messages, a 1 KiB Resource, mission events, telemetry, and SOS activation/cancellation | This is a two-phone field test, not a broad device matrix or soak test. EAM and checklist application gaps were recorded separately. |
| REM 1.2.7 RC1 | Pixel 7 + Samsung S8 over TCP | Mutual announces, discovery, explicit connection, bidirectional acknowledged chat, EAM/event/checklist replication, telemetry, SOS, and restart recovery | LoRa was explicitly deferred in this earlier RC. REM 1.4 later supplied the physical LoRa proof. |
| LXMF-rs Sideband phone HIL | Two real Sideband phones connected to `reticulumd` through ADB-reverse TCP | Commit `cc756672e008d24390047cec02a41196f2c8effe` records Pixel -> POCO and POCO -> Pixel direct text reaching `Delivered`. Commit `79051c3d09ee338febc8bca5db0e249af2dd3258` records a real RCH/Sideband HIL Resource incompatibility found on the wire and fixed in Rust. | This is real-client TCP evidence. It is not a current-release proof for every Sideband mode or propagation workflow. |
| LXMF-rs Columba release gate | `reticulumd` <-> clean Columba `v2.0.9-beta` at `3738bd7834128023db491cca0876585b799de942` | Commit `cb3865378ff77fc81a33280ac6423d8905b497ef` records the external-client gate passing in both directions for direct LXMF through Columba's current Python backend | This proves the recorded direct-LXMF/TCP workflow, not every Columba feature or radio mode. |
| RCH live integration | RCH on a live Pi/host path through `reticulumd`, plus REM phones | Commit `133dd162da59ddf191c8dee7bb2d8ae9e344c649` records a live RCH SDK2/ZMQ enqueue reaching `sent: propagated resource`. RCH's live stress report records real Reticulum connectivity and southbound delivery to REM phones. | The RCH report records stronger evidence for Pixel 7 than Pixel 8a; do not generalize it into a claim that every RCH/REM path passed. |

Primary source links:

- REM 1.4 field-test record:
  https://github.com/FreeTAKTeam/reticulum_mobile_emergency_management/blob/main/docs/releases/1.4.md
- REM 1.2.7 release record:
  https://github.com/FreeTAKTeam/reticulum_mobile_emergency_management/blob/main/docs/releases/1.2.7.md
- REM 1.2.7 physical TCP results:
  https://github.com/FreeTAKTeam/reticulum_mobile_emergency_management/blob/main/docs/stabilization/1.2.7-rc.1-results.html
- Sideband bidirectional phone HIL:
  https://github.com/FreeTAKTeam/LXMF-rs/commit/cc756672e008d24390047cec02a41196f2c8effe
- RCH/Sideband Resource field finding:
  https://github.com/FreeTAKTeam/LXMF-rs/commit/79051c3d09ee338febc8bca5db0e249af2dd3258
- Columba bidirectional external-client evidence:
  https://github.com/FreeTAKTeam/LXMF-rs/commit/cb3865378ff77fc81a33280ac6423d8905b497ef
- RCH live integration evidence:
  https://github.com/FreeTAKTeam/LXMF-rs/commit/133dd162da59ddf191c8dee7bb2d8ae9e344c649
- RCH live stress report:
  https://github.com/FreeTAKTeam/Reticulum-Community-Hub/blob/main/docs/release-live-stress-report.md

## What remains open

The existing field evidence does not close the full operational matrix. The
following remain valid work:

- permanent unattended RNode/LoRa HIL with retained machine-readable results;
- a finite physical matrix for RNode serial, TCP/Wi-Fi, and BLE, with exact
  hardware and firmware identification;
- RNodeMulti, Weave, VR-N76, and other specifically declared hardware rows;
- native Windows/macOS/mobile runtime rows that have not been exercised on the
  target platform;
- public I2P/public-network checks where those are part of the declared support
  claim;
- bounded long-running soak, reconnect, contention, and multi-hop tests;
- release-candidate reruns for external clients whose compatibility is claimed
  by that release;
- branch protection and automatic CI enforcement covered by #617, #618, and
  #623.

## Interpretation of #616 and #624

#616 is a coverage-completion and evidence-normalization issue. It starts from
existing field evidence and closes the unverified rows. It is not a statement
that LXMF-rs has never run on hardware or with real clients.

#624 is an automation issue. Its purpose is to turn physical testing that has
already been performed manually or through product HIL into a permanent,
repeatable hardware lane. It is not the first proof that the RNode/LoRa path
works.
