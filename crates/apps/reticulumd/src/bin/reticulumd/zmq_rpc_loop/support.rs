use rns_rpc::RpcDaemon;
use std::borrow::Cow;
use std::io;

pub(super) fn validate_zmq_bind_security(
    endpoint: &str,
    require_auth_for_remote: bool,
    daemon: &RpcDaemon,
) -> io::Result<()> {
    if require_auth_for_remote
        && !is_local_zmq_endpoint(endpoint)
        && !daemon.remote_rpc_token_auth_configured()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "remote zmq endpoints require explicit token authentication",
        ));
    }
    Ok(())
}

pub(super) fn validate_zmq_response_endpoint(endpoint: &str) -> io::Result<()> {
    if is_local_zmq_endpoint(endpoint) {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "remote zmq response endpoints require explicit authentication",
    ))
}

pub(super) fn is_local_zmq_endpoint(endpoint: &str) -> bool {
    endpoint.starts_with("inproc://")
        || endpoint.starts_with("tcp://127.")
        || endpoint.starts_with("tcp://localhost:")
        || endpoint.starts_with("tcp://[::1]:")
}

pub(super) fn zmq_response_connect_endpoint(endpoint: &str) -> Cow<'_, str> {
    if let Some(port) = endpoint.strip_prefix("tcp://localhost:") {
        return Cow::Owned(format!("tcp://127.0.0.1:{port}"));
    }
    Cow::Borrowed(endpoint)
}

pub(super) fn zmq_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

pub(super) fn is_recoverable_zmq_transport_error(err: &zeromq::ZmqError) -> bool {
    if matches!(err, zeromq::ZmqError::Codec(_)) {
        return true;
    }
    let text = err.to_string();
    text.contains("connection was aborted")
        || text.contains("connection was forcibly closed")
        || text.contains("connection reset")
        || text.contains("(os error 10053)")
        || text.contains("(os error 10054)")
}

/// Classify the typed timeout without decoding/allocating a broker result payload.
pub(super) fn broker_handler_outcome(
    method: &str,
    envelope: &rns_rpc::rpc::zmq::ZmqRpcEnvelope,
) -> rns_rpc::rpc::zmq_metrics::ZmqStageOutcome {
    use rns_rpc::rpc::zmq_metrics::ZmqStageOutcome;
    if !method.starts_with("sdk_broker_") {
        return ZmqStageOutcome::Succeeded;
    }
    #[derive(serde::Deserialize)]
    struct Response {
        #[serde(rename = "id")]
        _id: serde::de::IgnoredAny,
        #[serde(rename = "result")]
        _result: serde::de::IgnoredAny,
        error: Option<rns_rpc::RpcError>,
    }
    match rns_rpc::rpc::codec::decode_frame::<Response>(&envelope.payload) {
        Ok(response) => match response.error {
            Some(error) if error.code == "SDK_STORAGE_BROKER_TIMEOUT" => ZmqStageOutcome::TimedOut,
            _ => ZmqStageOutcome::Succeeded,
        },
        Err(error) => {
            log::warn!("[daemon] broker reply classification failed method={method}: {error}");
            ZmqStageOutcome::Failed
        }
    }
}
