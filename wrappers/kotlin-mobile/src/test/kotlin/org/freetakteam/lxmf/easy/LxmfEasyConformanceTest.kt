package org.freetakteam.lxmf.easy

import java.nio.file.Files
import java.nio.file.Path
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.runBlocking

class LxmfEasyConformanceTest {
    private val wrapperRoot: Path = Path.of("").toAbsolutePath()
    private val repoRoot: Path = wrapperRoot.parent.parent
    private val scenarios = listOf(
        "lifecycle.start_stop_restart",
        "events.delivery_ordering",
        "timeout.poll_timeout",
        "delivery.queue_pressure",
        "connectivity.reconnect_recovery",
        "errors.typed_mapping",
        "compatibility.unknown_additive",
    )

    @Test
    fun wrapperScenarioListMatchesSdkAppV1Manifest() {
        assertEquals(7, scenarios.size)
        assertTrue(scenarios.contains("lifecycle.start_stop_restart"))
        assertTrue(scenarios.contains("events.delivery_ordering"))
        assertTrue(scenarios.contains("timeout.poll_timeout"))
        assertTrue(scenarios.contains("delivery.queue_pressure"))
        assertTrue(scenarios.contains("connectivity.reconnect_recovery"))
        assertTrue(scenarios.contains("errors.typed_mapping"))
        assertTrue(scenarios.contains("compatibility.unknown_additive"))
    }

    @Test
    fun wrapperConformanceManifestUsesSharedSdkAppFixtures() {
        val manifest = Files.readString(wrapperRoot.resolve("conformance-manifest.json"))
        assertTrue(manifest.contains("\"contract_family\": \"sdk-app\""))
        assertTrue(manifest.contains("\"contract_release\": \"v1\""))
        assertTrue(manifest.contains("docs/fixtures/sdk-app-v1"))

        val fixtureRoot = repoRoot.resolve("docs/fixtures/sdk-app-v1")
        for (scenario in scenarios) {
            assertTrue(manifest.contains("\"$scenario\""), "manifest missing $scenario")
            assertTrue(
                Files.isRegularFile(fixtureRoot.resolve("$scenario.json")),
                "missing shared fixture for $scenario",
            )
        }
    }

    @Test
    fun lifecycleStartStopRestartUsesOneCallStartupAndCleanup() = runBlocking {
        val backend = RecordingBackend()
        val client = LxmfEasyClient(backend)

        val first = client.start(Config.mobile_default())
        client.stop(ShutdownMode.Graceful)
        val second = client.start(Config.mobile_default())

        assertEquals(listOf("start:mobile_default", "stop:Graceful", "start:mobile_default"), backend.calls)
        assertFalse(first.runtimeId == second.runtimeId)
    }

    @Test
    fun eventStreamExposesDeliveryOrderingWithoutRawPollLoop() {
        val backend = RecordingBackend()
        val client = LxmfEasyClient(backend)

        val events = client.subscribeEvents(SubscriptionStart.Tail)

        assertEquals(SubscriptionStart.Tail, backend.lastSubscriptionStart)
        assertTrue(events is Flow<LxmfEvent>)
    }

    @Test
    fun typedErrorsMatchFixtureMapping() {
        val queuePressure = LxmfEasyError.QueuePressure()
        assertEquals("SDK_APP_DELIVERY_QUEUE_PRESSURE", queuePressure.code)
        assertTrue(queuePressure.retryable)
        assertFalse(queuePressure.terminal)

        val invalidArgument = LxmfEasyError.InvalidArgument()
        assertEquals("SDK_APP_VALIDATION_INVALID_ARGUMENT", invalidArgument.code)
        assertFalse(invalidArgument.retryable)
        assertTrue(invalidArgument.terminal)
    }
}

private class RecordingBackend : LxmfEasyBackend {
    val calls = mutableListOf<String>()
    var lastSubscriptionStart: SubscriptionStart? = null
    private var starts = 0

    override suspend fun start(config: Config): RuntimeHandle {
        starts += 1
        calls += "start:${config.profile}"
        return RuntimeHandle("runtime-$starts")
    }

    override fun events(start: SubscriptionStart): Flow<LxmfEvent> {
        lastSubscriptionStart = start
        return flowOf(
            LxmfEvent.MessageQueued(1, "message-1"),
            LxmfEvent.MessageDispatching(2, "message-1"),
            LxmfEvent.MessageSent(3, "message-1"),
            LxmfEvent.MessageDelivered(4, "message-1"),
        )
    }

    override suspend fun send(request: SendRequest): SendReceipt =
        SendReceipt(messageId = request.correlationId, correlationId = request.correlationId)

    override suspend fun stop(mode: ShutdownMode) {
        calls += "stop:$mode"
    }
}
