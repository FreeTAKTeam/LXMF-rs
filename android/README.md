# First-party Bluetooth companion

Build with Java 17, Android SDK 35, Gradle 8.11.1:

```
gradle -p android :rns-bluetooth:testDebugUnitTest :rns-bluetooth:lintDebug :rns-bluetooth:assembleRelease
```

This AAR contains Java classes and source identity metadata, never native `.so`
libraries. The embedding application owns Android permissions, pairing UI,
foreground services and scheduling. It must provide a process-wide SessionRegistry
and pass the Rust bridge version into every transport constructor. Keep leases
until the native resource has closed. Close the lease on setup failure, stop or
permission loss. Do not share a lease between transports.

GattClient is a blocking worker API for connection, MTU negotiation, enumeration,
characteristic read/write and subscriptions. Notifications carry both UUIDs.
ClassicStream is a blocking worker API for caller-selected RFCOMM services. It
has one daemon reader thread per admitted stream, separate from operation workers.
It does not stop discovery or reset the adapter. Neither implementation retries
writes. Native completion is not application or recipient acknowledgement.

The current companion does not implement discovery, advertising or a GATT server.
No REM facade delegates to it yet. These are prerequisites for the later stages,
not optional support claims. JVM mocks and compilation do not establish radio,
Android lifecycle, permission revocation, Binder or cross-APK coexistence support.

Before consumer adoption, create the pair manifest from a clean committed source
revision using tools/scripts/bluetooth-artifact-pair.py and verify it at the host
build boundary. The script checks both artifacts' embedded source revision,
dirty state, hashes and bridge version. The release pipeline is responsible for
signed provenance; checksums alone do not establish publisher trust. Consumers
must pin the expected manifest, source revision and hashes independently of the
downloaded artifacts. Dirty development builds cannot produce a trusted pair.
