package org.reticulum.bluetooth;

import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

/** One budget includes lock acquisition, submission and native completion. */
public final class NativeDeadline {
    private final long start = System.nanoTime();
    private final long budget;

    public NativeDeadline(long timeoutMs) {
        if (timeoutMs <= 0 || timeoutMs > Long.MAX_VALUE / 1_000_000L) {
            throw new IllegalArgumentException("Invalid native deadline");
        }
        budget = TimeUnit.MILLISECONDS.toNanos(timeoutMs);
    }

    public long remainingNanos() throws TimeoutException {
        long remaining = budget - (System.nanoTime() - start);
        if (remaining <= 0) throw new TimeoutException("Bluetooth operation timed out");
        return remaining;
    }
}
