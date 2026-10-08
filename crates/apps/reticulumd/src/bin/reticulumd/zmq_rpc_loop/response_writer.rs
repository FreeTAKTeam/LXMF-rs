use super::{
    zmq_response_connect_endpoint, ZmqOutboundResponse, ZmqPipelineMetrics, ZmqStage,
    ZmqStageGuard, ZmqStageOutcome, ZMQ_RPC_WORKER_CONCURRENCY,
};
use rns_rpc::rpc::zmq;
use std::sync::Arc;
use std::{io, time::Duration};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use zeromq::{PushSocket, Socket, SocketSend, ZmqMessage};

// Stay below RCH's three-second RPC budget, including connect retries and handshake.
const RESPONSE_DELIVERY_TIMEOUT: Duration = Duration::from_secs(1);

async fn deliver(response: ZmqOutboundResponse, delivery: ZmqStageGuard) {
    let endpoint = response.endpoint.clone();
    let session_id = response.envelope.session_id.clone();
    let request_id = response.envelope.request_id;
    let result = tokio::time::timeout(RESPONSE_DELIVERY_TIMEOUT, async {
        let connect_endpoint = zmq_response_connect_endpoint(&endpoint);
        let mut socket = PushSocket::new();
        socket.connect(connect_endpoint.as_ref()).await.map_err(io::Error::other)?;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let encoded = zmq::encode_envelope(&response.envelope)?;
        socket.send(ZmqMessage::from(encoded)).await.map_err(io::Error::other)
    })
    .await;
    match result {
        Ok(Ok(())) => delivery.finish(ZmqStageOutcome::Succeeded),
        Ok(Err(error)) => {
            delivery.finish(ZmqStageOutcome::Failed);
            log::warn!(
            "[daemon] zmq rpc response failed endpoint={endpoint} session_id={session_id} request_id={request_id}: {error}"
        );
        }
        Err(_) => {
            delivery.finish(ZmqStageOutcome::TimedOut);
            log::warn!(
            "[daemon] zmq rpc response timed out endpoint={endpoint} session_id={session_id} request_id={request_id} timeout_ms={}",
            RESPONSE_DELIVERY_TIMEOUT.as_millis()
        );
        }
    }
}

pub(super) async fn run_zmq_response_writer(
    mut responses: mpsc::Receiver<ZmqOutboundResponse>,
    mut shutdown: watch::Receiver<bool>,
    metrics: Arc<ZmqPipelineMetrics>,
) {
    let mut deliveries = JoinSet::new();
    let mut closed = false;
    loop {
        if *shutdown.borrow() || (closed && deliveries.is_empty()) {
            break;
        }
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            completed = deliveries.join_next(), if !deliveries.is_empty() => {
                if let Some(Err(error)) = completed {
                    log::error!("[daemon] zmq rpc response delivery task failed: {error}");
                }
            }
            response = responses.recv(), if !closed && deliveries.len() < ZMQ_RPC_WORKER_CONCURRENCY => {
                match response {
                    Some(mut response) => {
                        // The guard exists even if this future is aborted before first poll.
                        let delivery = metrics.enter(ZmqStage::Delivery, response.owned_wire_bytes());
                        if let Some(queued) = response.queue_stage.take() { queued.finish(ZmqStageOutcome::Succeeded); }
                        deliveries.spawn(deliver(response, delivery));
                    }
                    None => closed = true,
                }
            }
        }
    }
    // One owner cancels and joins all active sockets; no detached response work survives shutdown.
    responses.close();
    deliveries.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rns_rpc::rpc::zmq::ZmqRpcEnvelope;

    #[test]
    fn unpolled_delivery_is_owned_and_cancelled_on_drop() {
        let metrics = ZmqPipelineMetrics::default();
        let response = ZmqOutboundResponse {
            endpoint: "tcp://127.0.0.1:1".into(),
            envelope: ZmqRpcEnvelope::response("unpolled".into(), 1, vec![0; 4096]),
            queue_stage: None,
        };
        let bytes = response.owned_wire_bytes();
        let guard = metrics.enter(ZmqStage::Delivery, bytes);
        let future = deliver(response, guard);
        assert_eq!(metrics.snapshot()["delivery"]["owned_wire_bytes"], bytes);
        drop(future);
        assert_eq!(metrics.snapshot()["delivery"]["active"], 0);
        assert_eq!(metrics.snapshot()["delivery"]["owned_wire_bytes"], 0);
        assert_eq!(metrics.snapshot()["delivery"]["cancelled"], 1);
    }
}
