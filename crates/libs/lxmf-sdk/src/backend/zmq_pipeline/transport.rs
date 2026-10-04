use super::{
    map_rpc_error, sdk_error, ErrorCategory, SdkError, ZmqEndpointRole, ZmqPipelineBackendClient,
    ZmqPipelineBackendConfig,
};
use rns_rpc::e2e_harness::{build_rpc_frame, parse_rpc_frame};
use rns_rpc::rpc::zmq::{self, ZmqRpcEnvelope, ZmqRpcEnvelopeKind};
use serde_json::Value as JsonValue;
use zeromq::{DealerSocket, PullSocket, PushSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

pub(super) struct ZmqPipelineTransport {
    pub(super) command: PushSocket,
    pub(super) responses: PullSocket,
    pub(super) response_endpoint: String,
}

pub(super) struct ZmqDealerTransport {
    socket: DealerSocket,
}

impl ZmqDealerTransport {
    async fn connect(config: &ZmqPipelineBackendConfig) -> Result<Self, SdkError> {
        let mut socket = DealerSocket::new();
        socket
            .connect(config.command_endpoint.as_str())
            .await
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        Ok(Self { socket })
    }
}

impl ZmqPipelineTransport {
    pub(super) async fn connect(config: &ZmqPipelineBackendConfig) -> Result<Self, SdkError> {
        let mut command = PushSocket::new();
        apply_role(&mut command, config.command_role, &config.command_endpoint).await?;
        let mut responses = PullSocket::new();
        let response_endpoint =
            apply_role(&mut responses, config.response_role, &config.response_endpoint).await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Ok(Self { command, responses, response_endpoint })
    }
}

impl ZmqPipelineBackendClient {
    pub(super) async fn call_rpc_async(
        &self,
        method: &str,
        params: Option<JsonValue>,
    ) -> Result<JsonValue, SdkError> {
        // The deadline includes contention, connection retries and the ZMTP handshake,
        // not only the send/receive phase. Socket defaults can otherwise take 30 seconds.
        let deadline = tokio::time::Instant::now() + self.config.request_timeout;
        let request_id = self.next_request_id();
        let payload = build_rpc_frame(request_id, method, params)
            .map_err(|err| sdk_error(ErrorCategory::Internal, err.to_string()))?;
        let auth = self.auth_metadata_for_request(request_id).map_err(|err| {
            sdk_error(ErrorCategory::Internal, format!("zmq request authentication clock: {err}"))
        })?;
        let envelope = ZmqRpcEnvelope::request(
            self.session_id.clone(),
            request_id,
            String::new(),
            payload,
            auth,
        );
        let response = if self.config.is_single_endpoint() {
            self.send_and_recv_single_endpoint(envelope, deadline).await?
        } else {
            self.send_and_recv_pipeline(envelope, deadline).await?
        };
        let rpc_response = parse_rpc_frame(&response.payload)
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        if let Some(error) = rpc_response.error {
            return Err(map_rpc_error(error));
        }
        Ok(rpc_response.result.unwrap_or(JsonValue::Null))
    }

    fn encode_request(&self, envelope: &ZmqRpcEnvelope) -> Result<Vec<u8>, SdkError> {
        let encoded = zmq::encode_envelope(envelope)
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        if encoded.len() > self.config.max_envelope_bytes {
            return Err(sdk_error(
                ErrorCategory::Transport,
                "zmq rpc envelope exceeded configured limit",
            ));
        }
        Ok(encoded)
    }

    async fn send_and_recv_pipeline(
        &self,
        mut envelope: ZmqRpcEnvelope,
        deadline: tokio::time::Instant,
    ) -> Result<ZmqRpcEnvelope, SdkError> {
        let mut guard = tokio::time::timeout_at(deadline, self.transport.lock())
            .await
            .map_err(|_| request_timeout())?;
        // Keep the advertised endpoint, socket exchange and failed-transport reset
        // under the same owner. Another caller must never use a replaced endpoint.
        let result = tokio::time::timeout_at(deadline, async {
            if guard.is_none() {
                *guard = Some(ZmqPipelineTransport::connect(&self.config).await?);
            }
            let transport = guard
                .as_mut()
                .ok_or_else(|| sdk_error(ErrorCategory::Internal, "missing zmq transport"))?;
            envelope.response_endpoint = Some(transport.response_endpoint.clone());
            let encoded = self.encode_request(&envelope)?;
            transport
                .command
                .send(ZmqMessage::from(encoded))
                .await
                .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
            loop {
                let message = transport
                    .responses
                    .recv()
                    .await
                    .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
                let response = decode_response(message)?;
                if response.kind == ZmqRpcEnvelopeKind::Response
                    && response.session_id == self.session_id
                    && response.request_id == envelope.request_id
                {
                    return Ok(response);
                }
            }
        })
        .await
        .unwrap_or_else(|_| Err(request_timeout()));
        if result.is_err() {
            *guard = None;
        }
        result
    }

    async fn send_and_recv_single_endpoint(
        &self,
        mut envelope: ZmqRpcEnvelope,
        deadline: tokio::time::Instant,
    ) -> Result<ZmqRpcEnvelope, SdkError> {
        envelope.response_endpoint = None;
        let encoded = self.encode_request(&envelope)?;
        let slot = envelope.request_id as usize % self.dealer_pool.len();
        let mut guard = tokio::time::timeout_at(deadline, self.dealer_pool[slot].lock())
            .await
            .map_err(|_| request_timeout())?;
        let result = tokio::time::timeout_at(deadline, async {
            if guard.is_none() {
                *guard = Some(ZmqDealerTransport::connect(&self.config).await?);
            }
            let transport = guard.as_mut().ok_or_else(|| {
                sdk_error(ErrorCategory::Internal, "missing zmq dealer transport")
            })?;
            transport
                .socket
                .send(ZmqMessage::from(encoded))
                .await
                .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
            let message = transport
                .socket
                .recv()
                .await
                .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
            let response = decode_response(message)?;
            if response.kind != ZmqRpcEnvelopeKind::Response
                || response.session_id != self.session_id
                || response.request_id != envelope.request_id
            {
                return Err(SdkError::new(
                    "SDK_TRANSPORT_ZMQ_CORRELATION_MISMATCH",
                    ErrorCategory::Transport,
                    "zmq rpc response did not match the active session and request",
                ));
            }
            Ok(response)
        })
        .await
        .unwrap_or_else(|_| Err(request_timeout()));
        if result.is_err() {
            *guard = None;
        }
        result
    }
}

fn request_timeout() -> SdkError {
    SdkError::new(
        "SDK_TRANSPORT_ZMQ_TIMEOUT",
        ErrorCategory::Timeout,
        "zmq rpc request timed out waiting for connection, send or correlated response",
    )
}

fn decode_response(message: ZmqMessage) -> Result<ZmqRpcEnvelope, SdkError> {
    let bytes = Vec::<u8>::try_from(message)
        .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
    zmq::decode_envelope(&bytes).map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))
}

async fn apply_role<S>(
    socket: &mut S,
    role: ZmqEndpointRole,
    endpoint: &str,
) -> Result<String, SdkError>
where
    S: Socket,
{
    match role {
        ZmqEndpointRole::Bind => {
            socket.bind(endpoint).await.map(|bound| bound.to_string()).map_err(|err| {
                sdk_error(ErrorCategory::Transport, format!("zmq bind {endpoint} failed: {err}"))
            })
        }
        ZmqEndpointRole::Connect => {
            socket.connect(endpoint).await.map(|_| endpoint.to_owned()).map_err(|err| {
                sdk_error(ErrorCategory::Transport, format!("zmq connect {endpoint} failed: {err}"))
            })
        }
    }
}
