#[derive(Clone, Debug)]
struct TestZmqEndpoint {
    resolved: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl TestZmqEndpoint {
    fn bind_request(&self) -> &'static str {
        "tcp://localhost:0"
    }

    fn set_resolved(&self, endpoint: String) {
        *self.resolved.lock().expect("resolved endpoint") = Some(endpoint);
    }
}

impl From<TestZmqEndpoint> for String {
    fn from(endpoint: TestZmqEndpoint) -> Self {
        endpoint
            .resolved
            .lock()
            .expect("resolved endpoint")
            .clone()
            .unwrap_or_else(|| endpoint.bind_request().to_owned())
    }
}

fn unused_loopback_endpoint() -> TestZmqEndpoint {
    TestZmqEndpoint { resolved: std::sync::Arc::new(std::sync::Mutex::new(None)) }
}

async fn recv_request_envelope(commands: &mut PullSocket) -> Option<ZmqRpcEnvelope> {
    let message = tokio::time::timeout(std::time::Duration::from_secs(1), commands.recv())
        .await
        .ok()?
        .ok()?;
    let bytes = Vec::<u8>::try_from(message).ok()?;
    zmq::decode_envelope(&bytes).ok()
}
