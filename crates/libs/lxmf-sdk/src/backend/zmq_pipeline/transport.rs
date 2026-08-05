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
        apply_role(&mut responses, config.response_role, &config.response_endpoint).await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Ok(Self { command, responses })
    }
}

impl ZmqPipelineBackendClient {
    pub(super) async fn call_rpc_async(
        &self,
        method: &str,
        params: Option<JsonValue>,
    ) -> Result<JsonValue, SdkError> {
        let request_id = self.next_request_id();
        let payload = build_rpc_frame(request_id, method, params)
            .map_err(|err| sdk_error(ErrorCategory::Internal, err.to_string()))?;
        let auth = self.auth_metadata_for_request(request_id).ok().flatten();
        let envelope = ZmqRpcEnvelope::request(
            self.session_id.clone(),
            request_id,
            self.config.response_endpoint.clone(),
            payload,
            auth,
        );
        let mut envelope = envelope;
        if self.config.is_single_endpoint() {
            envelope.response_endpoint = None;
        }
        let encoded = zmq::encode_envelope(&envelope)
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        if encoded.len() > self.config.max_envelope_bytes {
            return Err(sdk_error(
                ErrorCategory::Transport,
                "zmq rpc envelope exceeded configured limit",
            ));
        }
        let response = match self.send_and_recv(encoded, request_id).await {
            Ok(response) => response,
            Err(error) => {
                if !self.config.is_single_endpoint() {
                    *self.transport.lock().await = None;
                }
                return Err(error);
            }
        };
        let rpc_response = parse_rpc_frame(&response.payload)
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        if let Some(error) = rpc_response.error {
            return Err(map_rpc_error(error));
        }
        Ok(rpc_response.result.unwrap_or(JsonValue::Null))
    }

    pub(super) async fn send_and_recv(
        &self,
        encoded: Vec<u8>,
        request_id: u64,
    ) -> Result<ZmqRpcEnvelope, SdkError> {
        if self.config.is_single_endpoint() {
            return self.send_and_recv_single_endpoint(encoded, request_id).await;
        }
        let mut transport = self.transport.lock().await;
        if transport.is_none() {
            *transport = Some(ZmqPipelineTransport::connect(&self.config).await?);
        }
        let transport = transport
            .as_mut()
            .ok_or_else(|| sdk_error(ErrorCategory::Internal, "missing zmq transport"))?;

        // Bound the command send and response wait by one deadline. A PUSH
        // socket can itself await indefinitely when the peer or its receive
        // queue is wedged; starting the timeout only after `send` completed
        // lets the actor retain stale work until the application queue fills.
        let deadline = tokio::time::Instant::now() + self.config.request_timeout;
        let timeout_error = || {
            SdkError::new(
                "SDK_TRANSPORT_ZMQ_TIMEOUT",
                ErrorCategory::Timeout,
                "zmq rpc request timed out waiting for send or correlated response",
            )
        };
        async {
            let send_result = tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => Err(timeout_error()),
                result = transport.command.send(ZmqMessage::from(encoded)) => result
                    .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string())),
            };
            send_result?;

            loop {
                let message = tokio::select! {
                    biased;
                    _ = tokio::time::sleep_until(deadline) => return Err(timeout_error()),
                    result = transport.responses.recv() => result
                        .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?,
                };
                let bytes = Vec::<u8>::try_from(message)
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
        .await
    }

    async fn send_and_recv_single_endpoint(
        &self,
        encoded: Vec<u8>,
        request_id: u64,
    ) -> Result<ZmqRpcEnvelope, SdkError> {
        // A bounded persistent DEALER pool permits correlated requests on one client to proceed
        // concurrently without paying connection setup on every operation.
        let slot = request_id as usize % self.dealer_pool.len();
        let mut slot_guard = self.dealer_pool[slot].lock().await;
        if slot_guard.is_none() {
            *slot_guard = Some(ZmqDealerTransport::connect(&self.config).await?);
        }
        // Apply the same deadline to DEALER sends as to response receives. A
        // blocked send must not hold this pool slot forever or leave a caller
        // retrying while the original request is still in flight.
        let result = tokio::time::timeout(self.config.request_timeout, async {
            let transport = slot_guard.as_mut().ok_or_else(|| {
                sdk_error(ErrorCategory::Internal, "missing zmq dealer transport")
            })?;
            transport
                .socket
                .send(ZmqMessage::from(encoded))
                .await
                .map_err(|error| sdk_error(ErrorCategory::Transport, error.to_string()))?;
            transport
                .socket
                .recv()
                .await
                .map_err(|error| sdk_error(ErrorCategory::Transport, error.to_string()))
        })
        .await;
        let message = match result {
            Err(_) => {
                *slot_guard = None;
                return Err(SdkError::new(
                    "SDK_TRANSPORT_ZMQ_TIMEOUT",
                    ErrorCategory::Timeout,
                    "zmq rpc request timed out waiting for send or correlated response",
                ));
            }
            Ok(Err(error)) => {
                *slot_guard = None;
                return Err(error);
            }
            Ok(Ok(message)) => message,
        };
        let bytes = Vec::<u8>::try_from(message)
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        let envelope = zmq::decode_envelope(&bytes)
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        if envelope.kind != ZmqRpcEnvelopeKind::Response
            || envelope.session_id != self.session_id
            || envelope.request_id != request_id
        {
            return Err(SdkError::new(
                "SDK_TRANSPORT_ZMQ_CORRELATION_MISMATCH",
                ErrorCategory::Transport,
                "zmq rpc response did not match the active session and request",
            ));
        }
        Ok(envelope)
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
