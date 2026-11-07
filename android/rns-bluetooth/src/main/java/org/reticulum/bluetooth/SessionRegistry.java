package org.reticulum.bluetooth;

import java.io.IOException;
import java.util.HashMap;
import java.util.Map;

/** Process-local owner leases. No adapter-wide side effects or cross-APK promise. */
public final class SessionRegistry {
    public static final class Lease implements AutoCloseable {
        public final String owner;
        public final long generation;
        private final String device;
        private final SessionRegistry registry;
        private AutoCloseable resource;
        private boolean closed;
        private Lease(SessionRegistry registry, String owner, long generation, String device) {
            this.registry = registry; this.owner = owner; this.generation = generation; this.device = device;
        }
        /** A late-created native resource is closed before installation is rejected. */
        public synchronized void install(AutoCloseable value) throws Exception {
            if (closed) { value.close(); throw new IOException("Bluetooth session closed"); }
            if (resource != null) { value.close(); throw new IOException("Resource already installed"); }
            resource = value;
        }
        public synchronized boolean isClosed() { return closed; }
        @Override public void close() throws Exception {
            AutoCloseable current;
            synchronized (this) {
                if (closed) return;
                closed = true; current = resource; resource = null;
            }
            // Keep the device reserved until its native handle has been closed.
            try { if (current != null) current.close(); }
            finally { registry.release(this); }
        }
    }
    private final int capacity;
    private final Map<String, Lease> owners = new HashMap<>();
    private long generation;
    public SessionRegistry(int capacity) {
        if (capacity <= 0) throw new IllegalArgumentException("Invalid session capacity");
        this.capacity = capacity;
    }
    public synchronized Lease reserve(String owner, String device) throws IOException {
        if (owner == null || owner.isEmpty() || device == null || device.isEmpty())
            throw new IllegalArgumentException("Owner/device required");
        if (owners.containsKey(owner) || owners.size() >= capacity || generation == Long.MAX_VALUE)
            throw new IOException("Bluetooth owner/capacity busy");
        for (Lease existing : owners.values()) {
            if (existing.device.equals(device)) throw new IOException("Bluetooth device busy");
        }
        Lease lease = new Lease(this, owner, ++generation, device);
        owners.put(owner, lease);
        return lease;
    }
    private synchronized void release(Lease lease) { owners.remove(lease.owner, lease); }
}
