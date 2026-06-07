package org.freetakteam.lxmf.easy

import kotlinx.coroutines.flow.Flow

data class Config(
    val profile: String,
    val eventBufferLimit: Int,
    val retryEnabled: Boolean,
) {
    companion object {
        fun mobile_default(): Config = Config(
            profile = "mobile_default",
            eventBufferLimit = 256,
            retryEnabled = true,
        )
    }
}

data class RuntimeHandle(
    val runtimeId: String,
)

data class SendRequest(
    val source: String,
    val destination: String,
    val payload: Map<String, String>,
    val correlationId: String,
    val ttlMs: Long,
)

data class SendReceipt(
    val messageId: String,
    val correlationId: String,
)

enum class SubscriptionStart {
    Tail,
    Replay,
}

enum class ShutdownMode {
    Graceful,
    Immediate,
}

sealed class LxmfEvent {
    abstract val sequence: Long

    data class RuntimeStarted(
        override val sequence: Long,
        val runtimeId: String,
    ) : LxmfEvent()

    data class RuntimeStopped(
        override val sequence: Long,
        val runtimeId: String,
    ) : LxmfEvent()

    data class MessageQueued(
        override val sequence: Long,
        val messageId: String,
    ) : LxmfEvent()

    data class MessageDispatching(
        override val sequence: Long,
        val messageId: String,
    ) : LxmfEvent()

    data class MessageSent(
        override val sequence: Long,
        val messageId: String,
    ) : LxmfEvent()

    data class MessageDelivered(
        override val sequence: Long,
        val messageId: String,
    ) : LxmfEvent()

    data class MessageFailed(
        override val sequence: Long,
        val messageId: String,
        val error: LxmfEasyError,
    ) : LxmfEvent()

    data class QueuePressureRaised(
        override val sequence: Long,
        val pendingMessages: Int,
    ) : LxmfEvent()

    data class RetryScheduled(
        override val sequence: Long,
        val messageId: String,
        val delayMs: Long,
    ) : LxmfEvent()

    data class StreamGapDetected(
        override val sequence: Long,
        val expectedNextSequence: Long,
        val observedSequence: Long,
    ) : LxmfEvent()
}

sealed class LxmfEasyError(
    val code: String,
    val retryable: Boolean,
    val terminal: Boolean,
) : RuntimeException(code) {
    class InvalidArgument : LxmfEasyError(
        code = "SDK_APP_VALIDATION_INVALID_ARGUMENT",
        retryable = false,
        terminal = true,
    )

    class RuntimeNotStarted : LxmfEasyError(
        code = "SDK_APP_RUNTIME_NOT_STARTED",
        retryable = true,
        terminal = false,
    )

    class QueuePressure : LxmfEasyError(
        code = "SDK_APP_DELIVERY_QUEUE_PRESSURE",
        retryable = true,
        terminal = false,
    )

    class ConnectivityDisconnected : LxmfEasyError(
        code = "SDK_APP_CONNECTIVITY_DISCONNECTED",
        retryable = true,
        terminal = false,
    )
}

interface LxmfEasyBackend {
    suspend fun start(config: Config): RuntimeHandle
    fun events(start: SubscriptionStart): Flow<LxmfEvent>
    suspend fun send(request: SendRequest): SendReceipt
    suspend fun stop(mode: ShutdownMode)
}

class LxmfEasyClient(
    private val backend: LxmfEasyBackend,
) : AutoCloseable {
    companion object {
        fun rpc(endpoint: String): LxmfEasyClient =
            LxmfEasyClient(RpcLxmfEasyBackend(endpoint))
    }

    suspend fun start(config: Config = Config.mobile_default()): RuntimeHandle =
        backend.start(config)

    fun subscribeEvents(start: SubscriptionStart = SubscriptionStart.Tail): Flow<LxmfEvent> =
        events(start)

    fun events(start: SubscriptionStart = SubscriptionStart.Tail): Flow<LxmfEvent> =
        backend.events(start)

    suspend fun send(request: SendRequest): SendReceipt =
        backend.send(request)

    suspend fun stop(mode: ShutdownMode = ShutdownMode.Graceful) =
        backend.stop(mode)

    override fun close() {
        // Kotlin callers that need suspend cleanup should call stop() first.
    }
}

private class RpcLxmfEasyBackend(
    private val endpoint: String,
) : LxmfEasyBackend {
    override suspend fun start(config: Config): RuntimeHandle {
        require(config.profile == "mobile_default") { "only mobile_default is wired by this wrapper" }
        return RuntimeHandle(runtimeId = "rpc:$endpoint")
    }

    override fun events(start: SubscriptionStart): Flow<LxmfEvent> {
        throw LxmfEasyError.RuntimeNotStarted()
    }

    override suspend fun send(request: SendRequest): SendReceipt {
        if (request.destination.isBlank()) {
            throw LxmfEasyError.InvalidArgument()
        }
        return SendReceipt(
            messageId = request.correlationId,
            correlationId = request.correlationId,
        )
    }

    override suspend fun stop(mode: ShutdownMode) {
    }
}
