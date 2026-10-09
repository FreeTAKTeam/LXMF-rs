use super::admission::Admission;
use super::{
    handle_zmq_request_envelope, is_local_zmq_endpoint, is_recoverable_zmq_transport_error,
    validate_zmq_bind_security, zmq_io_error,
};
use rns_rpc::rpc::zmq;
use rns_rpc::RpcDaemon;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinSet;
use zeromq::{RouterSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

pub(crate) async fn run_zmq_router_loop_until(
    endpoint: String,
    require_auth_for_remote: bool,
    daemon: Arc<RpcDaemon>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    validate_zmq_bind_security(endpoint.as_str(), require_auth_for_remote, daemon.as_ref())?;
    let endpoint_requires_auth = require_auth_for_remote && !is_local_zmq_endpoint(&endpoint);
    let mut router = RouterSocket::new();
    router.bind(endpoint.as_str()).await.map_err(zmq_io_error)?;
    let (response_socket, mut request_socket) = router.split();
    let admission = Admission::new();
    let rejection_slots = Arc::new(tokio::sync::Semaphore::new(2));
    let mut workers = JoinSet::new();
    log::info!("reticulumd listening on canonical zmq {endpoint}");

    let result = loop {
        if *shutdown.borrow() {
            break Ok(());
        }
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break Ok(()); }
            }
            completed = workers.join_next(), if !workers.is_empty() => {
                if let Some(Err(error)) = completed {
                    log::error!("[daemon] zmq router worker failed: {error}");
                }
            }
            message = request_socket.recv() => {
                let message = match message {
                    Ok(message) => message,
                    Err(error) if is_recoverable_zmq_transport_error(&error) => {
                        log::warn!("[daemon] zmq router dropped client: {error}");
                        continue;
                    }
                    Err(error) => break Err(zmq_io_error(error)),
                };
                let frames = message.into_vec();
                if frames.len() != 2 || frames[1].len() > zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES {
                    log::warn!("[daemon] zmq router rejected request frame count or size");
                    continue;
                }
                let lease = match admission.admit(&frames[1]) {
                    Ok(Some(lease)) => lease,
                    Ok(None) => {
                        // No task or handler has been admitted. Only this generic
                        // overload result is returned before authentication.
                        match (zmq::request_header(&frames[1]),Arc::clone(&rejection_slots).try_acquire_owned()) {
                            (Ok((session,id)),Ok(permit)) => {
                                let reply=super::error_envelope(session.to_owned(),id,"SDK_TRANSPORT_ZMQ_BUSY","zmq request was not admitted; retry later");
                                match zmq::encode_envelope(&reply) {
                                    Ok(encoded) => {
                                        let mut message=ZmqMessage::from(encoded); message.push_front(frames[0].clone());
                                        let mut socket=response_socket.clone();
                                        workers.spawn(async move {
                                            let _permit=permit;
                                            if !matches!(tokio::time::timeout(Duration::from_millis(50),socket.send(message)).await,Ok(Ok(()))) { log::warn!("[daemon] bounded overload reply unavailable"); }
                                        });
                                    }
                                    Err(error)=>log::warn!("[daemon] overload reply encoding failed: {error}"),
                                }
                            }
                            (Err(error),_)=>log::warn!("[daemon] overload reply header invalid: {error}"),
                            (_,Err(_))=>log::debug!("[daemon] overload reply lane full"),
                        }
                        continue;
                    }
                    Err(error) => {
                        log::warn!("[daemon] zmq router invalid request: {error}");
                        continue;
                    }
                };
                let identity = frames[0].clone();
                let encoded = frames[1].to_vec();
                let daemon = Arc::clone(&daemon);
                let mut response_socket = response_socket.clone();
                workers.spawn(async move {
                    // Ownership and byte/count reservation precede task creation.
                    let _lease = lease;
                    let response = tokio::task::spawn_blocking(move || {
                        let envelope = zmq::decode_envelope(&encoded).map_err(io::Error::other)?;
                        handle_zmq_request_envelope(daemon.as_ref(), envelope, endpoint_requires_auth, true)
                            .map_err(io::Error::other)
                    }).await;
                    let response = match response {
                        Ok(Ok(response)) => response,
                        Ok(Err(error)) => { log::warn!("[daemon] zmq router request failed: {error}"); return; }
                        Err(error) => { log::error!("[daemon] zmq router dispatch failed: {error}"); return; }
                    };
                    let encoded = match zmq::encode_envelope(&response) {
                        Ok(encoded) => encoded,
                        Err(error) => { log::warn!("[daemon] zmq router response encode failed: {error}"); return; }
                    };
                    let mut message = ZmqMessage::from(encoded);
                    message.push_front(identity);
                    match tokio::time::timeout(Duration::from_secs(1), response_socket.send(message)).await {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => log::warn!("[daemon] zmq router reply failed: {error}"),
                        Err(_) => log::warn!("[daemon] zmq router reply timed out"),
                    }
                });
            }
        }
    };
    // Async cancellation cannot abort an admitted blocking mutation. Observe
    // every completion before dropping the socket owner and reporting shutdown.
    while let Some(completed) = workers.join_next().await {
        if let Err(error) = completed {
            log::error!("[daemon] zmq router shutdown worker failed: {error}");
        }
    }
    result
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
