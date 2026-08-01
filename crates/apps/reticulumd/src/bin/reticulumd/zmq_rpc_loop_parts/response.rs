use super::*;

pub(super) fn rpc_error_envelope(
    session_id: String,
    request_id: u64,
    error: RpcError,
) -> ZmqRpcEnvelope {
    let response = RpcResponse { id: request_id, result: None, error: Some(error) };
    ZmqRpcEnvelope::response(session_id, request_id, encode_rpc_response_frame(&response))
}

pub(super) fn error_envelope(
    session_id: impl Into<String>,
    request_id: u64,
    code: &'static str,
    message: impl Into<String>,
) -> ZmqRpcEnvelope {
    let response = RpcResponse {
        id: request_id,
        result: None,
        error: Some(RpcError::new(code, message.into())),
    };
    ZmqRpcEnvelope::response(session_id.into(), request_id, encode_rpc_response_frame(&response))
}

pub(super) fn encode_rpc_response_frame(response: &RpcResponse) -> Vec<u8> {
    rns_rpc::rpc::codec::encode_frame(response)
        .expect("RPC response frame serialization for ZMQ error response")
}

pub(super) fn validate_zmq_loop_config(
    config: &ZmqRpcLoopConfig,
    daemon: &RpcDaemon,
) -> io::Result<()> {
    validate_zmq_bind_security(
        config.command_endpoint.as_str(),
        config.require_auth_for_remote,
        daemon,
    )
}
