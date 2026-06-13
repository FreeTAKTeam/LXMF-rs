use super::{
    sdk_error, ErrorCategory, SdkError, ZmqEndpointRole, ZmqPipelineBackendClient,
    ZmqPipelineBackendConfig,
};
use rns_rpc::rpc::zmq::{self, ZmqRpcEnvelope, ZmqRpcEnvelopeKind};
use zeromq::{PullSocket, PushSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

pub(super) struct ZmqPipelineTransport {
    pub(super) command: PushSocket,
    pub(super) responses: PullSocket,
}

impl ZmqPipelineTransport {
    pub(super) async fn connect(config: &ZmqPipelineBackendConfig) -> Result<Self, SdkError> {
        let mut command = PushSocket::new();
        apply_role(&mut command, config.command_role, &config.command_endpoint).await?;
        let mut responses = PullSocket::new();
        apply_role(&mut responses, config.response_role, &config.response_endpoint).await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Ok(Self { command, responses })
    }
}

impl ZmqPipelineBackendClient {
    pub(super) async fn send_and_recv(
        &self,
        encoded: Vec<u8>,
        request_id: u64,
    ) -> Result<ZmqRpcEnvelope, SdkError> {
        let mut transport = self.transport.lock().await;
        if transport.is_none() {
            *transport = Some(ZmqPipelineTransport::connect(&self.config).await?);
        }
        let transport = transport
            .as_mut()
            .ok_or_else(|| sdk_error(ErrorCategory::Internal, "missing zmq transport"))?;

        transport
            .command
            .send(ZmqMessage::from(encoded))
            .await
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;

        let deadline = tokio::time::sleep(self.config.request_timeout);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => {
                    return Err(SdkError::new(
                        "SDK_TRANSPORT_ZMQ_TIMEOUT",
                        ErrorCategory::Timeout,
                        "zmq rpc request timed out waiting for correlated response",
                    ));
                }
                message = transport.responses.recv() => {
                    let bytes = Vec::<u8>::try_from(message.map_err(|err| {
                        sdk_error(ErrorCategory::Transport, err.to_string())
                    })?)
                    .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
                    let envelope = zmq::decode_envelope(&bytes)
                        .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
                    if envelope.kind == ZmqRpcEnvelopeKind::Response
                        && envelope.session_id == self.session_id
                        && envelope.request_id == request_id
                    {
                        return Ok(envelope);
                    }
                }
            }
        }
    }
}

async fn apply_role<S>(
    socket: &mut S,
    role: ZmqEndpointRole,
    endpoint: &str,
) -> Result<(), SdkError>
where
    S: Socket,
{
    match role {
        ZmqEndpointRole::Bind => socket.bind(endpoint).await.map(|_| ()).map_err(|err| {
            sdk_error(ErrorCategory::Transport, format!("zmq bind {endpoint} failed: {err}"))
        }),
        ZmqEndpointRole::Connect => socket.connect(endpoint).await.map_err(|err| {
            sdk_error(ErrorCategory::Transport, format!("zmq connect {endpoint} failed: {err}"))
        }),
    }
}
