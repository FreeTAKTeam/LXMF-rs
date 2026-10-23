#[derive(Clone)]
pub struct TcpListenerRuntimeStatusHandle {
    inner: Arc<std::sync::Mutex<TcpListenerRuntimeStatus>>,
}

impl TcpListenerRuntimeStatusHandle {
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().expect("tcp listener runtime status mutex poisoned").to_json()
    }
}

#[derive(Clone)]
struct TcpListenerRuntimeStatus {
    bind_addr: String,
    listener_state: String,
    client_mtu: usize,
    prefer_ipv6: bool,
    client_liveness_enabled: bool,
    client_forced_bitrate_bps: Option<u64>,
    accepted_connections: u64,
    accept_errors: u64,
    latest_client_endpoint: Option<String>,
    latest_client_iface: Option<String>,
    latest_stream_status: Option<TcpRuntimeStatusHandle>,
    last_error: Option<String>,
    fast_flap_policy: FastFlapPolicy,
    fast_flap_tracker: FastFlapTracker,
}

impl TcpListenerRuntimeStatus {
    fn new(
        bind_addr: String,
        client_mtu: usize,
        fast_flap_policy: FastFlapPolicy,
        fast_flap_tracker: FastFlapTracker,
    ) -> Self {
        Self {
            bind_addr,
            listener_state: "configured".to_string(),
            client_mtu,
            prefer_ipv6: false,
            client_liveness_enabled: false,
            client_forced_bitrate_bps: None,
            accepted_connections: 0,
            accept_errors: 0,
            latest_client_endpoint: None,
            latest_client_iface: None,
            latest_stream_status: None,
            last_error: None,
            fast_flap_policy,
            fast_flap_tracker,
        }
    }

    fn mark_binding(&mut self) {
        self.listener_state = "binding".to_string();
        self.last_error = None;
    }

    fn mark_listening(&mut self) {
        self.listener_state = "listening".to_string();
        self.last_error = None;
    }

    fn mark_bind_error(&mut self, error: String) {
        self.listener_state = "bind_error".to_string();
        self.last_error = Some(error);
    }

    fn mark_accept_error(&mut self, error: String) {
        self.accept_errors = self.accept_errors.saturating_add(1);
        self.last_error = Some(error);
    }

    fn mark_accepted(
        &mut self,
        endpoint: String,
        iface: crate::hash::AddressHash,
        status: TcpRuntimeStatusHandle,
    ) {
        self.listener_state = "listening".to_string();
        self.accepted_connections = self.accepted_connections.saturating_add(1);
        self.latest_client_endpoint = Some(endpoint);
        self.latest_client_iface = Some(iface.to_string());
        self.latest_stream_status = Some(status);
        self.last_error = None;
    }

    fn mark_closed(&mut self) {
        self.listener_state = "closed".to_string();
    }

    fn to_json(&self) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        root.insert("bind_addr".to_string(), serde_json::Value::String(self.bind_addr.clone()));
        root.insert(
            "listener_state".to_string(),
            serde_json::Value::String(self.listener_state.clone()),
        );
        root.insert(
            "client_mtu".to_string(),
            serde_json::Value::Number((self.client_mtu as u64).into()),
        );
        root.insert("prefer_ipv6".to_string(), serde_json::Value::Bool(self.prefer_ipv6));
        root.insert(
            "client_liveness_enabled".to_string(),
            serde_json::Value::Bool(self.client_liveness_enabled),
        );
        root.insert(
            "client_forced_bitrate_bps".to_string(),
            self.client_forced_bitrate_bps
                .map(|value| serde_json::Value::Number(value.into()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "accepted_connections".to_string(),
            serde_json::Value::Number(self.accepted_connections.into()),
        );
        root.insert(
            "accept_errors".to_string(),
            serde_json::Value::Number(self.accept_errors.into()),
        );
        root.insert(
            "latest_client_endpoint".to_string(),
            self.latest_client_endpoint
                .as_ref()
                .map(|value| serde_json::Value::String(value.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "latest_client_iface".to_string(),
            self.latest_client_iface
                .as_ref()
                .map(|value| serde_json::Value::String(value.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "latest_stream_status".to_string(),
            self.latest_stream_status
                .as_ref()
                .map(TcpRuntimeStatusHandle::to_json)
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "last_error".to_string(),
            self.last_error
                .as_ref()
                .map(|err| serde_json::Value::String(err.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "blocked_ip_count".to_string(),
            serde_json::Value::Number(
                (self.fast_flap_tracker.blocked_ip_count(self.fast_flap_policy) as u64).into(),
            ),
        );
        root.insert(
            "blocked_ips".to_string(),
            serde_json::Value::Array(
                self.fast_flap_tracker
                    .blocked_ip_list(self.fast_flap_policy)
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
        serde_json::Value::Object(root)
    }
}
