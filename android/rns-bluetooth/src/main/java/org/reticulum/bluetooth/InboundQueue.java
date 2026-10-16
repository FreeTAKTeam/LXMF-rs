package org.reticulum.bluetooth;

import java.io.IOException;
import java.util.ArrayDeque;
import java.util.Arrays;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

/** Bounded, terminal-aware stream. Null is idle; close/overflow always throws. */
public final class InboundQueue {
    public static final class Frame {
        public final String service;
        public final String characteristic;
        public final byte[] payload;
        Frame(String service, String characteristic, byte[] payload) {
            this.service = service;
            this.characteristic = characteristic;
            this.payload = Arrays.copyOf(payload, payload.length);
        }
    }
    private final int capacity;
    private final int maxPayload;
    private final ArrayDeque<Frame> frames = new ArrayDeque<>();
    private String terminal;

    public InboundQueue(int capacity, int maxPayload) {
        if (capacity <= 0 || maxPayload <= 0) throw new IllegalArgumentException("Invalid queue limits");
        this.capacity = capacity;
        this.maxPayload = maxPayload;
    }

    public synchronized boolean offer(String service, String characteristic, byte[] payload) {
        if (terminal != null) return false;
        if (payload == null || payload.length > maxPayload || frames.size() >= capacity) {
            close("Bluetooth inbound capacity exceeded");
            return false;
        }
        frames.addLast(new Frame(service, characteristic, payload));
        notifyAll();
        return true;
    }

    public synchronized Frame poll(long timeoutMs) throws IOException, InterruptedException {
        NativeDeadline deadline = new NativeDeadline(timeoutMs);
        while (terminal == null && frames.isEmpty()) {
            try { TimeUnit.NANOSECONDS.timedWait(this, deadline.remainingNanos()); }
            catch (TimeoutException idle) { return null; }
        }
        if (!frames.isEmpty()) return frames.removeFirst();
        throw new IOException(terminal);
    }

    public synchronized void finish(String reason) {
        if (terminal == null) terminal = reason;
        notifyAll();
    }

    public synchronized void close(String reason) {
        if (terminal == null) terminal = reason;
        frames.clear();
        notifyAll();
    }
}
