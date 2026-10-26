#[cfg(test)]
use super::ZmqFailureContext;
use super::{
    response_connections::{ResponseConnections, Route},
    zmq_response_connect_endpoint, ZmqOutboundResponse, ZmqPipelineMetrics, ZmqStage,
    ZmqStageGuard, ZmqStageOutcome, ZMQ_RPC_WORKER_CONCURRENCY,
};
use rns_rpc::rpc::zmq;
use std::sync::Arc;
use std::{io, time::Duration};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use zeromq::{PushSocket, Socket, SocketSend, ZmqMessage};

// Includes connect/handshake/encode/send; never extends the existing RPC budget.
const RESPONSE_DELIVERY_TIMEOUT: Duration = Duration::from_secs(1);
type CompletedConnection = Option<(Route, PushSocket)>;

async fn deliver(
    response: ZmqOutboundResponse,
    mut delivery: ZmqStageGuard,
    socket: Option<PushSocket>,
    metrics: Arc<ZmqPipelineMetrics>,
    daemon: Arc<rns_rpc::RpcDaemon>,
    queue_depth: usize,
) -> CompletedConnection {
    let route = Route::from_response(&response);
    let session: String = response
        .envelope
        .session_id
        .chars()
        .take(128)
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let request_id = response.envelope.request_id;
    let started = std::time::Instant::now();
    let mut stage = "encode";
    // Exclusive ownership precedes the first await. Cancellation drops the
    // socket and cannot return a partially used connection to the idle pool.
    let mut socket = socket;
    let mut network_stage = None;
    let result = tokio::time::timeout(RESPONSE_DELIVERY_TIMEOUT, async {
        let encoded = zmq::encode_envelope(&response.envelope)?;
        if socket.is_none() {
            stage = "connect";
            network_stage = Some(metrics.enter_correlated(
                ZmqStage::ResponseConnect,
                0,
                response.failure_context,
            ));
            socket = Some(PushSocket::new());
            socket
                .as_mut()
                .expect("installed socket")
                .connect(zmq_response_connect_endpoint(&response.endpoint).as_ref())
                .await
                .map_err(io::Error::other)?;
            let mut connected = network_stage.take().expect("connect stage");
            if connected.is_slow() {
                connected.attach_failure_context(
                    daemon.zmq_failure_context(response.failure_context.method, queue_depth),
                );
            }
            connected.finish(ZmqStageOutcome::Succeeded);
        }
        // connect returns after ZMTP handshake and peer registration. No fixed
        // sleep is necessary before sending on the exclusively owned socket.
        stage = "send";
        network_stage = Some(metrics.enter_correlated(
            ZmqStage::ResponseSend,
            encoded.capacity(),
            response.failure_context,
        ));
        socket
            .as_mut()
            .expect("connected socket")
            .send(ZmqMessage::from(encoded))
            .await
            .map_err(io::Error::other)?;
        let mut sent = network_stage.take().expect("send stage");
        if sent.is_slow() {
            sent.attach_failure_context(
                daemon.zmq_failure_context(response.failure_context.method, queue_depth),
            );
        }
        sent.finish(ZmqStageOutcome::Succeeded);
        Ok::<_, io::Error>(())
    })
    .await;
    match result {
        Ok(Ok(())) => {
            if delivery.is_slow() {
                delivery.attach_failure_context(
                    daemon.zmq_failure_context(response.failure_context.method, queue_depth),
                );
            }
            delivery.finish(ZmqStageOutcome::Succeeded);
            // Legacy clients cannot identify replaced response sockets; their
            // connections remain scoped to this reply, avoiding stale reuse.
            route.zip(socket)
        }
        Ok(Err(error)) => {
            let context = daemon.zmq_failure_context(response.failure_context.method, queue_depth);
            delivery.attach_failure_context(context);
            metrics.record_delivery_failure(
                &session,
                request_id,
                stage,
                false,
                started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            );
            if let Some(guard) = network_stage {
                let mut guard = guard;
                guard.attach_failure_context(context);
                guard.finish(ZmqStageOutcome::Failed);
            }
            delivery.finish(ZmqStageOutcome::Failed);
            log::warn!("[daemon] zmq rpc response failed session_id={session} request_id={request_id} stage={stage} execution_certainty=unknown: {error}");
            None
        }
        Err(_) => {
            let context = daemon.zmq_failure_context(response.failure_context.method, queue_depth);
            delivery.attach_failure_context(context);
            metrics.record_delivery_failure(
                &session,
                request_id,
                stage,
                true,
                started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            );
            if let Some(guard) = network_stage {
                let mut guard = guard;
                guard.attach_failure_context(context);
                guard.finish(ZmqStageOutcome::TimedOut);
            }
            delivery.finish(ZmqStageOutcome::TimedOut);
            log::warn!("[daemon] zmq rpc response timed out session_id={session} request_id={request_id} stage={stage} execution_certainty=unknown timeout_ms={}", RESPONSE_DELIVERY_TIMEOUT.as_millis());
            None
        }
    }
}

pub(super) async fn run_zmq_response_writer(
    mut responses: mpsc::Receiver<ZmqOutboundResponse>,
    mut shutdown: watch::Receiver<bool>,
    metrics: Arc<ZmqPipelineMetrics>,
    daemon: Arc<rns_rpc::RpcDaemon>,
) {
    let mut deliveries = JoinSet::new();
    let mut connections = ResponseConnections::default();
    let mut closed = false;
    loop {
        if *shutdown.borrow() || (closed && deliveries.is_empty()) {
            break;
        }
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
            }
            completed = deliveries.join_next(), if !deliveries.is_empty() => {
                match completed {
                    Some(Ok(Some((route, socket)))) => {
                        connections.insert(route, socket);
                        connections.reserve_active(deliveries.len());
                    },
                    Some(Err(error)) => log::error!("[daemon] zmq rpc response delivery task failed: {error}"),
                    _ => {}
                }
            }
            response = responses.recv(), if !closed && deliveries.len() < ZMQ_RPC_WORKER_CONCURRENCY => {
                match response {
                    Some(mut response) => {
                        let queue_depth = responses.len();
                        response.failure_context.queue_depth = queue_depth;
                        let socket = Route::from_response(&response).and_then(|route| connections.take(&route));
                        connections.reserve_active(deliveries.len() + 1);
                        let delivery = metrics.enter_correlated(ZmqStage::Delivery, response.owned_wire_bytes(), response.failure_context);
                        if let Some(mut queued) = response.queue_stage.take() {
                            if queued.is_slow() {
                                response.failure_context = daemon.zmq_failure_context(
                                    response.failure_context.method,
                                    queue_depth,
                                );
                                queued.attach_failure_context(response.failure_context);
                            }
                            queued.finish(ZmqStageOutcome::Succeeded);
                        }
                        deliveries.spawn(deliver(response, delivery, socket, Arc::clone(&metrics), Arc::clone(&daemon), queue_depth));
                    }
                    None => closed = true,
                }
            }
        }
    }
    responses.close();
    deliveries.shutdown().await;
    // Dropping the pool also closes every idle socket.
}
#[cfg(test)]
mod tests {
    use super::*;
    use rns_rpc::rpc::zmq::ZmqRpcEnvelope;

    #[test]
    fn unpolled_delivery_is_owned_and_cancelled_on_drop() {
        let metrics = Arc::new(ZmqPipelineMetrics::default());
        let response = ZmqOutboundResponse {
            connection_id: None,
            endpoint: "tcp://127.0.0.1:1".into(),
            envelope: ZmqRpcEnvelope::response("unpolled".into(), 1, vec![0; 4096]),
            failure_context: ZmqFailureContext::new("status", 0),
            queue_stage: None,
            admission: None,
        };
        let bytes = response.owned_wire_bytes();
        let guard = metrics.enter(ZmqStage::Delivery, bytes);
        let future = deliver(
            response,
            guard,
            None,
            Arc::clone(&metrics),
            Arc::new(rns_rpc::RpcDaemon::test_instance()),
            0,
        );
        assert_eq!(metrics.snapshot()["delivery"]["owned_wire_bytes"], bytes);
        drop(future);
        assert_eq!(metrics.snapshot()["delivery"]["active"], 0);
        assert_eq!(metrics.snapshot()["delivery"]["owned_wire_bytes"], 0);
        assert_eq!(metrics.snapshot()["delivery"]["cancelled"], 1);
    }
}
