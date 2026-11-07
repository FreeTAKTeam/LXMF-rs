package org.reticulum.bluetooth;

import org.junit.Test;
import static org.junit.Assert.*;
import java.io.IOException;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicBoolean;

public class LifecycleTest {
    @Test public void closeIsTerminalAndWakesAllReaders() throws Exception {
        InboundQueue queue = new InboundQueue(2, 8);
        ExecutorService pool = Executors.newFixedThreadPool(2);
        try {
            Callable<Boolean> read = () -> { try { queue.poll(5000); return false; } catch (IOException expected) { return true; } };
            Future<Boolean> first = pool.submit(read), second = pool.submit(read);
            queue.close("closed");
            assertTrue(first.get(1, TimeUnit.SECONDS));
            assertTrue(second.get(1, TimeUnit.SECONDS));
            assertThrows(IOException.class, () -> queue.poll(1));
        } finally { pool.shutdownNow(); }
    }
    @Test public void overflowDoesNotLoseBytesSilently() {
        InboundQueue queue = new InboundQueue(1, 8);
        assertTrue(queue.offer("s", "c", new byte[]{1}));
        assertFalse(queue.offer("s", "c", new byte[]{2}));
        assertThrows(IOException.class, () -> queue.poll(1));
    }
    @Test public void preservesEndpointAndCopiesPayload() throws Exception {
        InboundQueue queue = new InboundQueue(1, 8);
        byte[] bytes = {1}; queue.offer("s", "c", bytes); bytes[0] = 2;
        InboundQueue.Frame frame = queue.poll(1);
        assertEquals("s", frame.service); assertEquals("c", frame.characteristic);
        assertArrayEquals(new byte[]{1}, frame.payload);
        assertNull(queue.poll(1));
    }
    @Test public void lateHandleClosesAndOwnerCannotCloseReplacement() throws Exception {
        SessionRegistry registry = new SessionRegistry(2);
        SessionRegistry.Lease radio = registry.reserve("rnode", "radio");
        SessionRegistry.Lease old = registry.reserve("plugin", "plugin-device");
        old.close();
        AtomicBoolean nativeClosed = new AtomicBoolean();
        assertThrows(IOException.class, () -> old.install(() -> nativeClosed.set(true)));
        assertTrue(nativeClosed.get());
        SessionRegistry.Lease replacement = registry.reserve("plugin", "plugin-device");
        old.close();
        assertFalse(replacement.isClosed()); assertFalse(radio.isClosed());
        assertThrows(IOException.class, () -> registry.reserve("third", "radio"));
        replacement.close(); radio.close();
    }
    @Test public void eofDrainsAlreadyReceivedBytesBeforeTerminal() throws Exception {
        InboundQueue queue = new InboundQueue(2, 8);
        queue.offer("", "", new byte[]{1}); queue.finish("eof");
        assertArrayEquals(new byte[]{1}, queue.poll(1).payload);
        assertThrows(IOException.class, () -> queue.poll(1));
        assertFalse(queue.offer("", "", new byte[]{2}));
    }
    @Test public void mismatchedBridgeFailsBeforeIo() {
        BridgeContract.requireVersion(1);
        assertThrows(IllegalStateException.class, () -> BridgeContract.requireVersion(2));
    }
}
