use super::{zmq_response_connect_endpoint, ZmqOutboundResponse, ZMQ_RPC_WORKER_CONCURRENCY};
use rns_rpc::rpc::zmq;
use std::{io, time::Duration};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use zeromq::{PushSocket, Socket, SocketSend, ZmqMessage};

// Stay below RCH's three-second RPC budget, including connect retries and handshake.
const RESPONSE_DELIVERY_TIMEOUT: Duration = Duration::from_secs(1);

async fn deliver(response: ZmqOutboundResponse) {
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
        Ok(Ok(())) => {}
        Ok(Err(error)) => log::warn!(
            "[daemon] zmq rpc response failed endpoint={endpoint} session_id={session_id} request_id={request_id}: {error}"
        ),
        Err(_) => log::warn!(
            "[daemon] zmq rpc response timed out endpoint={endpoint} session_id={session_id} request_id={request_id} timeout_ms={}",
            RESPONSE_DELIVERY_TIMEOUT.as_millis()
        ),
    }
}

pub(super) async fn run_zmq_response_writer(
    mut responses: mpsc::Receiver<ZmqOutboundResponse>,
    mut shutdown: watch::Receiver<bool>,
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
                    Some(response) => { deliveries.spawn(deliver(response)); }
                    None => closed = true,
                }
            }
        }
    }
    // One owner cancels and joins all active sockets; no detached response work survives shutdown.
    responses.close();
    deliveries.shutdown().await;
}
