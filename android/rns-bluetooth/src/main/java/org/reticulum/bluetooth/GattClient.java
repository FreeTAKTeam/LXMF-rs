package org.reticulum.bluetooth;

import android.annotation.SuppressLint;
import android.bluetooth.*;
import android.content.Context;
import android.os.Build;
import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.locks.ReentrantLock;

/** Blocking native boundary. Invoke on an I/O worker; close is safe from another thread. */
@SuppressLint("MissingPermission")
@SuppressWarnings("deprecation")
public final class GattClient implements AutoCloseable {
    private static final UUID CCCD = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb");
    public static final class Endpoint {
        public final UUID service, characteristic;
        public final int properties;
        Endpoint(UUID service, UUID characteristic, int properties) {
            this.service = service; this.characteristic = characteristic; this.properties = properties;
        }
    }
    private static final class Pending {
        final String kind;
        final UUID service, characteristic;
        final CountDownLatch done = new CountDownLatch(1);
        IOException error;
        byte[] value;
        Pending(String kind, UUID service, UUID characteristic) {
            this.kind = kind; this.service = service; this.characteristic = characteristic;
        }
    }
    private final InboundQueue inbound;
    private final int maxPayload;
    private final ReentrantLock operations = new ReentrantLock();
    private final CountDownLatch connected = new CountDownLatch(1);
    private BluetoothGatt gatt;
    private Pending pending;
    private boolean closed;
    private boolean opening;
    private IOException terminal;
    private int mtu = 23;

    public GattClient(int rustBridgeVersion, SessionRegistry.Lease lease, int queueCapacity, int maxPayload) throws Exception {
        BridgeContract.requireVersion(rustBridgeVersion);
        this.inbound = new InboundQueue(queueCapacity, maxPayload);
        this.maxPayload = maxPayload;
        lease.install(this);
    }

    public void open(Context context, BluetoothDevice device, int requestedMtu,
                     boolean requireBond, long timeoutMs) throws Exception {
        NativeDeadline deadline = new NativeDeadline(timeoutMs);
        if (requestedMtu < 23 || requestedMtu > 517) throw new IllegalArgumentException("Invalid ATT MTU");
        if (requireBond && device.getBondState() != BluetoothDevice.BOND_BONDED)
            throw new IOException("Bluetooth pairing required");
        synchronized (this) { ensureOpen(); if (opening) throw new IOException("Already opened"); opening = true; }
        try {
            BluetoothGatt created = device.connectGatt(context, false, callback, BluetoothDevice.TRANSPORT_LE);
            if (created == null) throw new IOException("Android did not create a GATT connection");
            if (!accept(created)) throw new IOException("Bluetooth session closed during open");
            if (!connected.await(deadline.remainingNanos(), TimeUnit.NANOSECONDS))
                throw new java.util.concurrent.TimeoutException("GATT connection timed out");
            synchronized (this) { ensureOpen(); }
            execute(new Pending("mtu", null, null), deadline, () -> created.requestMtu(requestedMtu));
            execute(new Pending("services", null, null), deadline, created::discoverServices);
        } catch (Exception error) { close(); throw error; }
    }

    public synchronized int attMtu() throws IOException { ensureOpen(); return mtu; }

    public synchronized List<Endpoint> endpoints() throws IOException {
        ensureOpen();
        if (gatt == null) throw new IOException("GATT is not open");
        List<Endpoint> result = new ArrayList<>();
        for (BluetoothGattService service : gatt.getServices())
            for (BluetoothGattCharacteristic characteristic : service.getCharacteristics())
                result.add(new Endpoint(service.getUuid(), characteristic.getUuid(), characteristic.getProperties()));
        return result;
    }

    public void subscribe(UUID service, UUID characteristic, boolean indicate, long timeoutMs) throws Exception {
        NativeDeadline deadline = new NativeDeadline(timeoutMs);
        execute(new Pending("subscribe", service, characteristic), deadline, () -> {
            BluetoothGattCharacteristic channel = channel(service, characteristic);
            BluetoothGattDescriptor descriptor = channel.getDescriptor(CCCD);
            if (descriptor == null || !gatt.setCharacteristicNotification(channel, true)) return false;
            byte[] value = indicate ? BluetoothGattDescriptor.ENABLE_INDICATION_VALUE
                                    : BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE;
            if (Build.VERSION.SDK_INT >= 33) return gatt.writeDescriptor(descriptor, value) == BluetoothStatusCodes.SUCCESS;
            descriptor.setValue(value); return gatt.writeDescriptor(descriptor);
        });
    }

    public byte[] read(UUID service, UUID characteristic, long timeoutMs) throws Exception {
        Pending operation = new Pending("read", service, characteristic);
        execute(operation, new NativeDeadline(timeoutMs), () -> gatt.readCharacteristic(channel(service, characteristic)));
        if (operation.value == null) throw new IOException("GATT read has no value");
        return operation.value.clone();
    }

    public void write(UUID service, UUID characteristic, byte[] payload,
                      boolean withResponse, long timeoutMs) throws Exception {
        if (payload == null || payload.length == 0 || payload.length > maxPayload)
            throw new IllegalArgumentException("Invalid GATT write size");
        byte[] value = payload.clone();
        execute(new Pending("write", service, characteristic), new NativeDeadline(timeoutMs), () -> {
            BluetoothGattCharacteristic channel = channel(service, characteristic);
            int type = withResponse ? BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                                    : BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE;
            if (Build.VERSION.SDK_INT >= 33) return gatt.writeCharacteristic(channel, value, type) == BluetoothStatusCodes.SUCCESS;
            channel.setWriteType(type); channel.setValue(value); return gatt.writeCharacteristic(channel);
        });
    }

    public InboundQueue.Frame poll(long timeoutMs) throws Exception { return inbound.poll(timeoutMs); }

    private interface Submission { boolean start() throws Exception; }
    private void execute(Pending operation, NativeDeadline deadline, Submission submission) throws Exception {
        if (!operations.tryLock(deadline.remainingNanos(), TimeUnit.NANOSECONDS))
            throw new java.util.concurrent.TimeoutException("GATT operation queue timed out");
        try {
            synchronized (this) {
                ensureOpen(); pending = operation;
                if (!submission.start()) throw new IOException("Android rejected GATT operation");
            }
            if (!operation.done.await(deadline.remainingNanos(), TimeUnit.NANOSECONDS))
                throw new java.util.concurrent.TimeoutException("GATT operation timed out");
            synchronized (this) { ensureOpen(); if (operation.error != null) throw operation.error; }
        } catch (Exception error) {
            // A timed-out native request cannot be matched safely to a later request.
            close(); throw error;
        } finally {
            synchronized (this) { if (pending == operation) pending = null; }
            operations.unlock();
        }
    }

    private BluetoothGattCharacteristic channel(UUID service, UUID characteristic) throws IOException {
        ensureOpen();
        if (gatt == null) throw new IOException("GATT is not open");
        BluetoothGattService selected = gatt.getService(service);
        BluetoothGattCharacteristic result = selected == null ? null : selected.getCharacteristic(characteristic);
        if (result == null) throw new IOException("GATT endpoint not found");
        return result;
    }

    private synchronized boolean accept(BluetoothGatt candidate) {
        if (!closed && (gatt == null || gatt == candidate)) { gatt = candidate; return true; }
        candidate.close(); return false;
    }
    private void ensureOpen() throws IOException {
        if (closed) throw terminal == null ? new IOException("GATT closed") : terminal;
    }
    private synchronized void complete(String kind, BluetoothGattCharacteristic channel, int status, byte[] value) {
        Pending current = pending;
        if (current == null || !current.kind.equals(kind)) return;
        if (channel != null && (!channel.getUuid().equals(current.characteristic)
            || !channel.getService().getUuid().equals(current.service))) return;
        if (status != BluetoothGatt.GATT_SUCCESS) current.error = new IOException("GATT operation failed: " + status);
        if (value != null) {
            if (value.length > maxPayload) current.error = new IOException("GATT value exceeds capacity");
            else current.value = value.clone();
        }
        current.done.countDown();
    }
    private void notification(BluetoothGatt source, BluetoothGattCharacteristic channel, byte[] value) {
        if (!accept(source)) return;
        if (!inbound.offer(channel.getService().getUuid().toString(), channel.getUuid().toString(), value)) close();
    }
    private final BluetoothGattCallback callback = new BluetoothGattCallback() {
        @Override public void onConnectionStateChange(BluetoothGatt source, int status, int state) {
            if (!accept(source)) return;
            if (status == BluetoothGatt.GATT_SUCCESS && state == BluetoothProfile.STATE_CONNECTED) connected.countDown();
            else if (status != BluetoothGatt.GATT_SUCCESS || state == BluetoothProfile.STATE_DISCONNECTED) close();
        }
        @Override public void onMtuChanged(BluetoothGatt source, int value, int status) {
            if (!accept(source)) return;
            synchronized (GattClient.this) { if (status == BluetoothGatt.GATT_SUCCESS) mtu = value; }
            complete("mtu", null, status, null);
        }
        @Override public void onServicesDiscovered(BluetoothGatt source, int status) {
            if (accept(source)) complete("services", null, status, null);
        }
        @Override public void onDescriptorWrite(BluetoothGatt source, BluetoothGattDescriptor descriptor, int status) {
            if (accept(source) && CCCD.equals(descriptor.getUuid())) complete("subscribe", descriptor.getCharacteristic(), status, null);
        }
        @Override public void onCharacteristicWrite(BluetoothGatt source, BluetoothGattCharacteristic channel, int status) {
            if (accept(source)) complete("write", channel, status, null);
        }
        @Override public void onCharacteristicRead(BluetoothGatt source, BluetoothGattCharacteristic channel, int status) {
            if (accept(source)) complete("read", channel, status, channel.getValue());
        }
        @Override public void onCharacteristicRead(BluetoothGatt source, BluetoothGattCharacteristic channel, byte[] value, int status) {
            if (accept(source)) complete("read", channel, status, value);
        }
        @Override public void onCharacteristicChanged(BluetoothGatt source, BluetoothGattCharacteristic channel) {
            notification(source, channel, channel.getValue());
        }
        @Override public void onCharacteristicChanged(BluetoothGatt source, BluetoothGattCharacteristic channel, byte[] value) {
            notification(source, channel, value);
        }
    };

    @Override public void close() {
        BluetoothGatt current;
        synchronized (this) {
            if (closed) return;
            closed = true; terminal = new IOException("GATT session closed");
            current = gatt; gatt = null;
            if (pending != null) { pending.error = terminal; pending.done.countDown(); }
            connected.countDown();
        }
        inbound.close("GATT session closed");
        if (current != null) {
            try { current.disconnect(); } catch (SecurityException ignored) { /* revoked */ }
            finally { current.close(); }
        }
    }
}
