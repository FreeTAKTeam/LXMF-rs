package org.reticulum.bluetooth;

import android.bluetooth.BluetoothDevice;
import android.bluetooth.BluetoothSocket;
import java.io.*;
import java.util.UUID;
import java.util.concurrent.*;
import org.junit.Test;
import static org.junit.Assert.*;
import static org.mockito.Mockito.*;

public class ClassicLifecycleTest {
    @Test public void singleWorkerCanWriteWhileReaderWaitsAndEofPreservesBytes() throws Exception {
        UUID service = UUID.fromString("00000000-0000-0000-0000-000000000003");
        BluetoothDevice device = mock(BluetoothDevice.class);
        BluetoothSocket socket = mock(BluetoothSocket.class);
        CountDownLatch reading = new CountDownLatch(1), finish = new CountDownLatch(1), eof = new CountDownLatch(1);
        InputStream input = new InputStream() {
            private boolean first = true;
            @Override public int read() { throw new AssertionError("use bulk read"); }
            @Override public int read(byte[] bytes) throws IOException {
                if (first) { first = false; bytes[0] = 42; return 1; }
                reading.countDown();
                try { if (!finish.await(2, TimeUnit.SECONDS)) throw new IOException("test read timeout"); }
                catch (InterruptedException interrupted) { throw new IOException(interrupted); }
                eof.countDown(); return -1;
            }
        };
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        when(device.createRfcommSocketToServiceRecord(service)).thenReturn(socket);
        when(socket.getInputStream()).thenReturn(input); when(socket.getOutputStream()).thenReturn(output);
        SessionRegistry.Lease lease = new SessionRegistry(1).reserve("owner", "synthetic");
        ExecutorService worker = Executors.newSingleThreadExecutor();
        ClassicStream stream = new ClassicStream(1, lease, worker, 8, 200);
        try {
            stream.open(device, service, 1000);
            assertTrue(reading.await(1, TimeUnit.SECONDS));
            stream.write(new byte[]{7}, 1000);
            assertArrayEquals(new byte[]{7}, output.toByteArray());
            finish.countDown(); assertTrue(eof.await(1, TimeUnit.SECONDS));
            assertArrayEquals(new byte[]{42}, stream.poll(1000).payload);
            assertThrows(IOException.class, () -> stream.poll(1000));
        } finally { finish.countDown(); lease.close(); worker.shutdownNow(); }
    }
}
