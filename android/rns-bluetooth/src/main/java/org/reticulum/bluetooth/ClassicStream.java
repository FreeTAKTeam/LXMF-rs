package org.reticulum.bluetooth;

import android.annotation.SuppressLint;
import android.bluetooth.BluetoothDevice;
import android.bluetooth.BluetoothSocket;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.Arrays;
import java.util.UUID;
import java.util.concurrent.*;
import java.util.concurrent.locks.ReentrantLock;

/** Caller-selected RFCOMM service. Does not cancel another owner's discovery. */
@SuppressLint("MissingPermission")
public final class ClassicStream implements AutoCloseable {
    private final ExecutorService executor;
    private final InboundQueue inbound;
    private final int maxPayload;
    private final ReentrantLock writes = new ReentrantLock();
    private BluetoothSocket socket;
    private OutputStream output;
    private boolean closed;
    private boolean opening;

    public ClassicStream(int rustBridgeVersion, SessionRegistry.Lease lease, ExecutorService executor,
                         int queueCapacity, int maxPayload) throws Exception {
        BridgeContract.requireVersion(rustBridgeVersion);
        this.executor = executor; this.maxPayload = maxPayload;
        inbound = new InboundQueue(queueCapacity, maxPayload);
        lease.install(this);
    }

    public void open(BluetoothDevice device, UUID service, long timeoutMs) throws Exception {
        NativeDeadline deadline = new NativeDeadline(timeoutMs);
        synchronized (this) {
            ensureOpen();
            if (opening) throw new IOException("Classic open already started");
            opening = true;
        }
        BluetoothSocket created = device.createRfcommSocketToServiceRecord(service);
        synchronized (this) {
            if (closed || socket != null) { created.close(); throw new IOException("Classic session unavailable"); }
            socket = created;
        }
        Future<?> connect = null;
        try {
            connect = executor.submit(() -> { created.connect(); return null; });
            connect.get(deadline.remainingNanos(), TimeUnit.NANOSECONDS);
            InputStream input = created.getInputStream();
            OutputStream writer = created.getOutputStream();
            synchronized (this) {
                ensureOpen(); output = writer;
            }
            Thread reader = new Thread(() -> readLoop(input), "bluetooth-classic-reader");
            reader.setDaemon(true);
            reader.start();
        } catch (Exception error) {
            close(); if (connect != null) connect.cancel(true); throw error;
        }
    }
    public InboundQueue.Frame poll(long timeoutMs) throws Exception { return inbound.poll(timeoutMs); }

    public void write(byte[] payload, long timeoutMs) throws Exception {
        if (payload == null || payload.length == 0 || payload.length > maxPayload)
            throw new IllegalArgumentException("Invalid Classic write size");
        byte[] value = payload.clone();
        NativeDeadline deadline = new NativeDeadline(timeoutMs);
        if (!writes.tryLock(deadline.remainingNanos(), TimeUnit.NANOSECONDS))
            throw new TimeoutException("Classic write queue timed out");
        Future<?> pending = null;
        try {
            OutputStream writer;
            synchronized (this) { ensureOpen(); writer = output; }
            if (writer == null) throw new IOException("Classic stream not connected");
            pending = executor.submit(() -> { writer.write(value); writer.flush(); return null; });
            pending.get(deadline.remainingNanos(), TimeUnit.NANOSECONDS);
            synchronized (this) { ensureOpen(); }
        } catch (Exception error) {
            close(); if (pending != null) pending.cancel(true); throw error;
        } finally { writes.unlock(); }
    }
    private void readLoop(InputStream input) {
        byte[] buffer = new byte[maxPayload];
        try {
            while (true) {
                synchronized (this) { if (closed) return; }
                int count = input.read(buffer);
                if (count < 0) { finish(); return; }
                if (count > 0 && !inbound.offer("", "", Arrays.copyOf(buffer, count))) { close(); return; }
            }
        } catch (IOException error) { close(); }
    }
    private void ensureOpen() throws IOException { if (closed) throw new IOException("Classic stream closed"); }
    private void finish() { terminate(false); }
    @Override public void close() { terminate(true); }
    private void terminate(boolean discard) {
        BluetoothSocket current;
        synchronized (this) {
            if (closed) return;
            closed = true; current = socket; socket = null; output = null;
        }
        if (discard) inbound.close("Classic stream closed");
        else inbound.finish("Classic stream ended");
        if (current != null) try { current.close(); } catch (IOException ignored) { /* best effort */ }
    }
}
