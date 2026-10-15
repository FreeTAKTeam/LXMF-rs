package org.reticulum.bluetooth;

import android.bluetooth.*;
import android.content.Context;
import java.io.IOException;
import java.util.UUID;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicReference;
import org.junit.Test;
import static org.junit.Assert.*;
import static org.mockito.ArgumentMatchers.*;
import static org.mockito.Mockito.*;

public class GattLifecycleTest {
    private static final UUID SERVICE = UUID.fromString("00000000-0000-0000-0000-000000000001");
    private static final UUID CHANNEL = UUID.fromString("00000000-0000-0000-0000-000000000002");

    @Test public void closeDuringNativeCreationDisposesLateHandle() throws Exception {
        BluetoothDevice device = mock(BluetoothDevice.class);
        BluetoothGatt nativeGatt = mock(BluetoothGatt.class);
        Context context = mock(Context.class);
        CountDownLatch creating = new CountDownLatch(1), release = new CountDownLatch(1);
        when(device.connectGatt(eq(context), eq(false), any(), eq(BluetoothDevice.TRANSPORT_LE)))
            .thenAnswer(call -> { creating.countDown(); assertTrue(release.await(2, TimeUnit.SECONDS)); return nativeGatt; });
        SessionRegistry.Lease lease = new SessionRegistry(1).reserve("owner", "device");
        GattClient client = new GattClient(1, lease, 8, 200);
        ExecutorService worker = Executors.newSingleThreadExecutor();
        try {
            Future<?> opening = worker.submit(() -> { client.open(context, device, 247, false, 2000); return null; });
            assertTrue(creating.await(1, TimeUnit.SECONDS));
            lease.close(); release.countDown();
            assertThrows(ExecutionException.class, () -> opening.get(2, TimeUnit.SECONDS));
            verify(nativeGatt, atLeastOnce()).close();
        } finally { release.countDown(); lease.close(); worker.shutdownNow(); }
    }

    @Test public void duplicateOpenCannotCloseAdmittedConnection() throws Exception {
        BluetoothDevice device = mock(BluetoothDevice.class);
        BluetoothGatt nativeGatt = mock(BluetoothGatt.class);
        Context context = mock(Context.class);
        CountDownLatch creating = new CountDownLatch(1), release = new CountDownLatch(1);
        AtomicReference<BluetoothGattCallback> callback = new AtomicReference<>();
        when(device.connectGatt(eq(context), eq(false), any(), eq(BluetoothDevice.TRANSPORT_LE)))
            .thenAnswer(call -> {
                callback.set(call.getArgument(2)); creating.countDown();
                assertTrue(release.await(2, TimeUnit.SECONDS));
                callback.get().onConnectionStateChange(nativeGatt, 0, BluetoothProfile.STATE_CONNECTED);
                return nativeGatt;
            });
        when(nativeGatt.requestMtu(247)).thenAnswer(call -> { callback.get().onMtuChanged(nativeGatt, 247, 0); return true; });
        when(nativeGatt.discoverServices()).thenAnswer(call -> { callback.get().onServicesDiscovered(nativeGatt, 0); return true; });
        SessionRegistry.Lease lease = new SessionRegistry(1).reserve("owner", "device");
        GattClient client = new GattClient(1, lease, 8, 200);
        ExecutorService worker = Executors.newSingleThreadExecutor();
        try {
            Future<?> opening = worker.submit(() -> { client.open(context, device, 247, false, 2000); return null; });
            assertTrue(creating.await(1, TimeUnit.SECONDS));
            assertThrows(IOException.class, () -> client.open(context, device, 247, false, 2000));
            release.countDown(); opening.get(2, TimeUnit.SECONDS);
            verify(device, times(1)).connectGatt(eq(context), eq(false), any(), eq(BluetoothDevice.TRANSPORT_LE));
            verify(nativeGatt, never()).close();
            assertEquals(247, client.attMtu());
        } finally { release.countDown(); lease.close(); worker.shutdownNow(); }
    }

    @Test public void closeNeverAcknowledgesPendingWriteAsSuccess() throws Exception {
        BluetoothDevice device = mock(BluetoothDevice.class);
        BluetoothGatt nativeGatt = mock(BluetoothGatt.class);
        BluetoothGattService service = mock(BluetoothGattService.class);
        BluetoothGattCharacteristic channel = mock(BluetoothGattCharacteristic.class);
        Context context = mock(Context.class);
        AtomicReference<BluetoothGattCallback> callback = new AtomicReference<>();
        when(device.connectGatt(eq(context), eq(false), any(), eq(BluetoothDevice.TRANSPORT_LE)))
            .thenAnswer(call -> { callback.set(call.getArgument(2)); callback.get().onConnectionStateChange(nativeGatt, 0, BluetoothProfile.STATE_CONNECTED); return nativeGatt; });
        when(nativeGatt.requestMtu(247)).thenAnswer(call -> { callback.get().onMtuChanged(nativeGatt, 247, 0); return true; });
        when(nativeGatt.discoverServices()).thenAnswer(call -> { callback.get().onServicesDiscovered(nativeGatt, 0); return true; });
        when(nativeGatt.getService(SERVICE)).thenReturn(service);
        when(service.getCharacteristic(CHANNEL)).thenReturn(channel);
        CountDownLatch submitted = new CountDownLatch(1);
        when(nativeGatt.writeCharacteristic(channel)).thenAnswer(call -> { submitted.countDown(); return true; });
        SessionRegistry.Lease lease = new SessionRegistry(1).reserve("owner", "device");
        GattClient client = new GattClient(1, lease, 8, 200);
        ExecutorService worker = Executors.newSingleThreadExecutor();
        try {
            client.open(context, device, 247, false, 2000);
            Future<?> writing = worker.submit(() -> { client.write(SERVICE, CHANNEL, new byte[]{1}, true, 2000); return null; });
            assertTrue(submitted.await(1, TimeUnit.SECONDS));
            lease.close();
            assertThrows(ExecutionException.class, () -> writing.get(1, TimeUnit.SECONDS));
        } finally { lease.close(); worker.shutdownNow(); }
    }
}
