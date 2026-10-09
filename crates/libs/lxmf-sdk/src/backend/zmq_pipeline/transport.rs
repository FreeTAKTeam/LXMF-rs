use super::failure_context::ExchangeContext;
use super::{
    map_rpc_error, sdk_error, ErrorCategory, SdkError, ZmqEndpointRole, ZmqPipelineBackendClient,
    ZmqPipelineBackendConfig,
};
use rns_rpc::e2e_harness::{build_rpc_frame, parse_rpc_frame};
use rns_rpc::rpc::zmq::{self, ZmqRpcEnvelope, ZmqRpcEnvelopeKind};
use rns_rpc::rpc::RpcResponse;
use serde_json::Value as JsonValue;
use zeromq::{DealerSocket, PullSocket, PushSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

pub(super) struct ZmqPipelineTransport {
    pub(super) command: PushSocket,
    pub(super) responses: PullSocket,
    pub(super) response_endpoint: String,
    pub(super) response_connection_id: String,
}

pub(super) struct ZmqDealerTransport {
    socket: DealerSocket,
}

impl Drop for ZmqPipelineTransport {
    fn drop(&mut self) {
        // zeromq 0.6 PullSocket has no Drop shutdown, unlike Push/Dealer.
        // Close its peer queues explicitly when an exchange is cancelled.
        self.responses.backend().shutdown();
    }
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
        // Install the Drop owner before bind/connect can suspend or fail.
        let mut transport = Self {
            command,
            responses: PullSocket::new(),
            response_endpoint: String::new(),
            response_connection_id: new_response_connection_id(),
        };
        transport.response_endpoint =
            apply_role(&mut transport.responses, config.response_role, &config.response_endpoint)
                .await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Ok(transport)
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
        let started = tokio::time::Instant::now();
        let deadline = started + self.config.request_timeout;
        self.call_rpc_attempt(method, params, started, deadline).await
    }

    pub(super) async fn call_rpc_replay_safe(
        &self,
        method: &str,
        params: Option<JsonValue>,
    ) -> Result<JsonValue, SdkError> {
        if !super::recovery::replay_safe(method) {
            return Err(sdk_error(ErrorCategory::Internal, "method has no replay-safe contract"));
        }
        let started = tokio::time::Instant::now();
        let deadline = started + self.config.request_timeout;
        let first_deadline = started + self.config.request_timeout / 2;
        match self.call_rpc_attempt(method, params.clone(), started, first_deadline).await {
            Ok(value) => Ok(value),
            Err(error) => {
                if super::recovery::ZmqRecoveryDecision::for_error(&error, true)
                    != super::recovery::ZmqRecoveryDecision::RetryWithinBudget
                    || tokio::time::Instant::now() >= deadline
                {
                    return Err(error);
                }
                // One retry authority, at most two exchanges under the original
                // operation deadline; each attempt mints a new correlation/JTI.
                let backoff = (deadline - tokio::time::Instant::now())
                    .min(std::time::Duration::from_millis(25));
                tokio::time::sleep(backoff).await;
                self.call_rpc_attempt(method, params, started, deadline).await
            }
        }
    }

    async fn call_rpc_attempt(
        &self,
        method: &str,
        params: Option<JsonValue>,
        started: tokio::time::Instant,
        deadline: tokio::time::Instant,
    ) -> Result<JsonValue, SdkError> {
        let request_id = self.next_request_id();
        let mut context = ExchangeContext::new(&self.session_id, method, request_id, started);
        let result = async {
            if tokio::time::Instant::now() >= deadline {
                return Err(context.timeout());
            }
            let payload = build_rpc_frame(request_id, method, params)
                .map_err(|err| sdk_error(ErrorCategory::Internal, err.to_string()))?;
            context.stage = "authentication";
            let auth = self.auth_metadata_for_request(request_id).map_err(|err| {
                sdk_error(
                    ErrorCategory::Internal,
                    format!("zmq request authentication clock: {err}"),
                )
            })?;
            let envelope = ZmqRpcEnvelope::request(
                self.session_id.clone(),
                request_id,
                String::new(),
                payload,
                auth,
            );
            let response = if self.config.is_single_endpoint() {
                self.send_and_recv_single_endpoint(envelope, deadline, &mut context).await?
            } else {
                self.send_and_recv_pipeline(envelope, deadline, &mut context).await?
            };
            if let Some(error) = response.error {
                context.stage = "rpc response";
                return Err(map_rpc_error(error));
            }
            Ok(response.result.unwrap_or(JsonValue::Null))
        }
        .await;
        result.map_err(|error| context.annotate(error))
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
        context: &mut ExchangeContext<'_>,
    ) -> Result<RpcResponse, SdkError> {
        context.stage = "transport lock";
        let mut guard = tokio::time::timeout_at(deadline, self.transport.lock())
            .await
            .map_err(|_| context.timeout())?;
        // Leave the shared slot empty across every await. An externally dropped
        // future drops its local socket and cannot skip a later reset statement.
        // Retain the slot guard so another caller cannot race endpoint ownership.
        let mut owned = guard.take();
        context.stage = "connection";
        let result = tokio::time::timeout_at(deadline, async {
            if tokio::time::Instant::now() >= deadline {
                return Err(context.timeout());
            }
            if owned.is_none() {
                owned = Some(ZmqPipelineTransport::connect(&self.config).await?);
            }
            let transport = owned
                .as_mut()
                .ok_or_else(|| sdk_error(ErrorCategory::Internal, "missing zmq transport"))?;
            envelope.response_endpoint = Some(transport.response_endpoint.clone());
            envelope.response_connection_id = Some(transport.response_connection_id.clone());
            context.stage = "envelope encode";
            let encoded = self.encode_request(&envelope)?;
            context.stage = "send";
            if tokio::time::Instant::now() >= deadline {
                return Err(context.timeout());
            }
            context.send_started = true;
            transport
                .command
                .send(ZmqMessage::from(encoded))
                .await
                .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
            context.send_completed = true;
            loop {
                context.stage = "correlated response";
                let message = transport
                    .responses
                    .recv()
                    .await
                    .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
                context.stage = "response decode";
                let response = decode_response(message, self.config.max_envelope_bytes)?;
                if response.kind == ZmqRpcEnvelopeKind::Response
                    && response.session_id == self.session_id
                    && response.request_id == envelope.request_id
                {
                    return decode_rpc_response(response, envelope.request_id, context);
                }
                context.ignored_replies = context.ignored_replies.saturating_add(1);
            }
        })
        .await
        .unwrap_or_else(|_| Err(context.timeout()));
        if result.is_ok() {
            *guard = owned;
        }
        result
    }

    async fn send_and_recv_single_endpoint(
        &self,
        mut envelope: ZmqRpcEnvelope,
        deadline: tokio::time::Instant,
        context: &mut ExchangeContext<'_>,
    ) -> Result<RpcResponse, SdkError> {
        envelope.response_endpoint = None;
        context.stage = "envelope encode";
        let encoded = self.encode_request(&envelope)?;
        let slot = envelope.request_id as usize % self.dealer_pool.len();
        context.stage = "transport lock";
        let mut guard = tokio::time::timeout_at(deadline, self.dealer_pool[slot].lock())
            .await
            .map_err(|_| context.timeout())?;
        let mut owned = guard.take();
        context.stage = "connection";
        let result = tokio::time::timeout_at(deadline, async {
            if tokio::time::Instant::now() >= deadline {
                return Err(context.timeout());
            }
            if owned.is_none() {
                owned = Some(ZmqDealerTransport::connect(&self.config).await?);
            }
            let transport = owned.as_mut().ok_or_else(|| {
                sdk_error(ErrorCategory::Internal, "missing zmq dealer transport")
            })?;
            context.stage = "send";
            if tokio::time::Instant::now() >= deadline {
                return Err(context.timeout());
            }
            context.send_started = true;
            transport
                .socket
                .send(ZmqMessage::from(encoded))
                .await
                .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
            context.send_completed = true;
            context.stage = "correlated response";
            let message = transport
                .socket
                .recv()
                .await
                .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
            context.stage = "response decode";
            let response = decode_response(message, self.config.max_envelope_bytes)?;
            if response.kind != ZmqRpcEnvelopeKind::Response
                || response.session_id != self.session_id
                || response.request_id != envelope.request_id
            {
                context.stage = "response correlation";
                return Err(SdkError::new(
                    "SDK_TRANSPORT_ZMQ_CORRELATION_MISMATCH",
                    ErrorCategory::Transport,
                    "zmq rpc response did not match the active session and request",
                ));
            }
            decode_rpc_response(response, envelope.request_id, context)
        })
        .await
        .unwrap_or_else(|_| Err(context.timeout()));
        if result.is_ok() {
            *guard = owned;
        }
        result
    }
}

fn decode_rpc_response(
    response: ZmqRpcEnvelope,
    request_id: u64,
    context: &mut ExchangeContext<'_>,
) -> Result<RpcResponse, SdkError> {
    context.stage = "rpc response decode";
    let response = parse_rpc_frame(&response.payload)
        .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
    if response.id != request_id {
        context.stage = "response correlation";
        return Err(SdkError::new(
            "SDK_TRANSPORT_ZMQ_CORRELATION_MISMATCH",
            ErrorCategory::Transport,
            "zmq rpc payload did not match the active request",
        ));
    }
    Ok(response)
}

fn decode_response(message: ZmqMessage, max_bytes: usize) -> Result<ZmqRpcEnvelope, SdkError> {
    if message.iter().map(|frame| frame.len()).sum::<usize>() > max_bytes {
        return Err(sdk_error(ErrorCategory::Transport, "zmq response exceeded configured limit"));
    }
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

// A monotonic process-local suffix ensures replacement even if wall-clock
// resolution is coarse or the clock moves backwards. This is routing, not auth.
fn new_response_connection_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
    format!("{}-{}", super::new_session_id(), NEXT_GENERATION.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    #[test]
    fn response_socket_generations_are_unique_and_bounded() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..1000 {
            let id = new_response_connection_id();
            assert!(id.len() <= 128);
            assert!(ids.insert(id));
        }
    }
}
