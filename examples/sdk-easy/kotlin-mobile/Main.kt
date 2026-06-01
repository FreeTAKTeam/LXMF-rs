package org.example.lxmfeasy

import org.freetakteam.lxmf.easy.Config
import org.freetakteam.lxmf.easy.LxmfEasyClient
import org.freetakteam.lxmf.easy.LxmfEvent
import org.freetakteam.lxmf.easy.SendRequest
import org.freetakteam.lxmf.easy.ShutdownMode
import org.freetakteam.lxmf.easy.SubscriptionStart
import kotlinx.coroutines.runBlocking

fun main() = runBlocking {
    val client = LxmfEasyClient.rpc("unix:/tmp/lxmf-rpc.sock")

    // lifecycle.start_stop_restart
    val handle = client.start(Config.mobile_default())
    println("runtime_id=${handle.runtimeId}")

    // events.delivery_ordering
    val events = client.subscribeEvents(SubscriptionStart.Tail)
    val receipt = client.send(
        SendRequest(
            source = "example.mobile",
            destination = "example.peer",
            payload = mapOf(
                "title" to "hello",
                "content" to "sent from Kotlin easy mode",
            ),
            correlationId = "easy-kotlin-mobile-send",
            ttlMs = 30_000,
        )
    )

    for (event in events) {
        when (event) {
            is LxmfEvent.MessageDelivered ->
                if (event.messageId == receipt.messageId) break
            is LxmfEvent.StreamGapDetected ->
                throw IllegalStateException("recover with poll_events before continuing")
            is LxmfEvent.QueuePressureRaised ->
                throw IllegalStateException("delivery.queue_pressure policy surfaced")
            else -> Unit
        }
    }

    client.stop(ShutdownMode.Graceful)
}
