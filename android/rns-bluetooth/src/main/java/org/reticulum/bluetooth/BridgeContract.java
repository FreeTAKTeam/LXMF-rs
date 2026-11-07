package org.reticulum.bluetooth;

/** Rust checks the same version; artifact checksums are verified by the host build. */
public final class BridgeContract {
    public static final int VERSION = 1;
    private BridgeContract() { }
    public static void requireVersion(int rustVersion) {
        if (rustVersion != VERSION) throw new IllegalStateException("Bluetooth bridge version mismatch");
    }
}
