use alloc::string::String;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::{lookup_host, TcpListener};

use crate::error::RnsError;

use super::tcp_client::{
    backbone_hdlc_watchdog, prefer_ipv6_socket_addrs, HdlcStreamWatchdog, TcpClient,
    TcpRuntimeStatusHandle, TcpSocketTuning,
};
use super::{Interface, InterfaceContext, InterfaceManager};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FastFlapPolicy {
    pub enabled: bool,
    pub threshold: Duration,
    pub grace: u64,
    pub expiry: Duration,
}

impl Default for FastFlapPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: Duration::from_secs(20),
            grace: 5,
            expiry: Duration::from_secs(12 * 60 * 60),
        }
    }
}

#[derive(Clone)]
struct AcceptedClientOptions {
    client_mtu: usize,
    client_socket_tuning: TcpSocketTuning,
    client_hdlc_watchdog: Option<HdlcStreamWatchdog>,
    client_forced_bitrate_bps: Option<u64>,
    fast_flap_tracker: Option<FastFlapTracker>,
    fast_flap_policy: FastFlapPolicy,
    peer_ip: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FastFlapTracker {
    entries: Arc<std::sync::Mutex<HashMap<String, FastFlapEntry>>>,
}

#[derive(Debug, Clone, Copy)]
struct FastFlapEntry {
    last: Instant,
    flaps: u64,
}

impl Default for FastFlapTracker {
    fn default() -> Self {
        Self { entries: Arc::new(std::sync::Mutex::new(HashMap::new())) }
    }
}

impl FastFlapTracker {
    pub(crate) fn is_blocked(&self, ip: &str, policy: FastFlapPolicy) -> bool {
        if !policy.enabled {
            return false;
        }
        let now = Instant::now();
        let mut entries = self.entries.lock().expect("fast-flap tracker mutex poisoned");
        entries.retain(|_, entry| now.saturating_duration_since(entry.last) <= policy.expiry);
        entries.get(ip).is_some_and(|entry| entry.flaps > policy.grace)
    }

    pub(crate) fn record_short_connection(
        &self,
        ip: &str,
        connected_for: Duration,
        policy: FastFlapPolicy,
    ) {
        if !policy.enabled || connected_for >= policy.threshold {
            return;
        }
        let now = Instant::now();
        let mut entries = self.entries.lock().expect("fast-flap tracker mutex poisoned");
        entries.retain(|_, entry| now.saturating_duration_since(entry.last) <= policy.expiry);
        let entry = entries.entry(ip.to_string()).or_insert(FastFlapEntry { last: now, flaps: 0 });
        entry.last = now;
        entry.flaps = entry.flaps.saturating_add(1);
    }

    pub(crate) fn blocked_ip_count(&self, policy: FastFlapPolicy) -> usize {
        if !policy.enabled {
            return 0;
        }
        let now = Instant::now();
        let mut entries = self.entries.lock().expect("fast-flap tracker mutex poisoned");
        entries.retain(|_, entry| now.saturating_duration_since(entry.last) <= policy.expiry);
        entries.values().filter(|entry| entry.flaps > policy.grace).count()
    }

    pub(crate) fn blocked_ip_list(&self, policy: FastFlapPolicy) -> Vec<String> {
        if !policy.enabled {
            return Vec::new();
        }
        let now = Instant::now();
        let mut entries = self.entries.lock().expect("fast-flap tracker mutex poisoned");
        entries.retain(|_, entry| now.saturating_duration_since(entry.last) <= policy.expiry);
        let mut ips = entries
            .iter()
            .filter(|(_, entry)| entry.flaps > policy.grace)
            .map(|(ip, _)| ip.clone())
            .collect::<Vec<_>>();
        ips.sort();
        ips
    }
}

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

pub struct TcpServer {
    addr: String,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    client_mtu: usize,
    client_socket_tuning: TcpSocketTuning,
    client_hdlc_watchdog: Option<HdlcStreamWatchdog>,
    client_forced_bitrate_bps: Option<u64>,
    prefer_ipv6: bool,
    fast_flap_policy: FastFlapPolicy,
    fast_flap_tracker: FastFlapTracker,
    runtime_status: Arc<std::sync::Mutex<TcpListenerRuntimeStatus>>,
}

impl TcpServer {
    pub const DEFAULT_CLIENT_MTU: usize = TcpClient::DEFAULT_MTU;

    pub fn new<T: Into<String>>(
        addr: T,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        let addr = addr.into();
        let fast_flap_policy = FastFlapPolicy::default();
        let fast_flap_tracker = FastFlapTracker::default();
        Self {
            runtime_status: Arc::new(std::sync::Mutex::new(TcpListenerRuntimeStatus::new(
                addr.clone(),
                Self::DEFAULT_CLIENT_MTU,
                fast_flap_policy,
                fast_flap_tracker.clone(),
            ))),
            addr,
            iface_manager,
            client_mtu: Self::DEFAULT_CLIENT_MTU,
            client_socket_tuning: TcpSocketTuning::default(),
            client_hdlc_watchdog: None,
            client_forced_bitrate_bps: None,
            prefer_ipv6: false,
            fast_flap_policy,
            fast_flap_tracker,
        }
    }

    #[must_use]
    pub fn with_client_mtu(mut self, client_mtu: usize) -> Self {
        self.client_mtu = client_mtu.max(256);
        self.runtime_status
            .lock()
            .expect("tcp listener runtime status mutex poisoned")
            .client_mtu = self.client_mtu;
        self
    }

    #[must_use]
    pub fn with_client_socket_tuning(mut self, client_socket_tuning: TcpSocketTuning) -> Self {
        self.client_socket_tuning = client_socket_tuning;
        self
    }

    #[must_use]
    pub fn with_backbone_client_liveness(mut self) -> Self {
        self.client_hdlc_watchdog = Some(backbone_hdlc_watchdog());
        self.runtime_status
            .lock()
            .expect("tcp listener runtime status mutex poisoned")
            .client_liveness_enabled = true;
        self
    }

    #[must_use]
    pub fn with_client_forced_bitrate(mut self, bitrate_bps: u64) -> Self {
        self.client_forced_bitrate_bps = (bitrate_bps > 0).then_some(bitrate_bps);
        self.runtime_status
            .lock()
            .expect("tcp listener runtime status mutex poisoned")
            .client_forced_bitrate_bps = self.client_forced_bitrate_bps;
        self
    }

    #[must_use]
    pub fn with_prefer_ipv6(mut self, prefer_ipv6: bool) -> Self {
        self.prefer_ipv6 = prefer_ipv6;
        self.runtime_status
            .lock()
            .expect("tcp listener runtime status mutex poisoned")
            .prefer_ipv6 = prefer_ipv6;
        self
    }

    #[must_use]
    pub fn with_fast_flapping(
        mut self,
        enabled: bool,
        threshold: Duration,
        grace: u64,
        expiry: Duration,
    ) -> Self {
        self.fast_flap_policy = FastFlapPolicy { enabled, threshold, grace, expiry };
        self.runtime_status
            .lock()
            .expect("tcp listener runtime status mutex poisoned")
            .fast_flap_policy = self.fast_flap_policy;
        self
    }

    #[must_use]
    pub fn client_socket_tuning(&self) -> TcpSocketTuning {
        self.client_socket_tuning
    }

    #[must_use]
    pub fn client_hdlc_liveness_enabled(&self) -> bool {
        self.client_hdlc_watchdog.is_some()
    }

    #[must_use]
    pub fn client_forced_bitrate_bps(&self) -> Option<u64> {
        self.client_forced_bitrate_bps
    }

    #[must_use]
    pub fn prefer_ipv6(&self) -> bool {
        self.prefer_ipv6
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> TcpListenerRuntimeStatusHandle {
        TcpListenerRuntimeStatusHandle { inner: self.runtime_status.clone() }
    }

    fn accepted_client(
        addr: String,
        stream: tokio::net::TcpStream,
        options: AcceptedClientOptions,
    ) -> TcpClient {
        let mut client = TcpClient::new_from_stream(addr, stream)
            .with_mtu(options.client_mtu)
            .with_socket_tuning(options.client_socket_tuning);
        if let Some(bitrate_bps) = options.client_forced_bitrate_bps {
            client = client.with_forced_bitrate(bitrate_bps);
        }
        if let Some(watchdog) = options.client_hdlc_watchdog {
            client = client.with_hdlc_watchdog(watchdog);
        }
        if let Some(tracker) = options.fast_flap_tracker {
            client =
                client.with_fast_flap_tracking(tracker, options.fast_flap_policy, options.peer_ip);
        }
        client
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let parent_iface = context.channel.address;
        let (
            addr,
            client_mtu,
            client_socket_tuning,
            client_hdlc_watchdog,
            client_forced_bitrate_bps,
            prefer_ipv6,
            fast_flap_policy,
            fast_flap_tracker,
            runtime_status,
        ) = {
            let guard = context.inner.lock().unwrap();
            (
                guard.addr.clone(),
                guard.client_mtu,
                guard.client_socket_tuning,
                guard.client_hdlc_watchdog.clone(),
                guard.client_forced_bitrate_bps,
                guard.prefer_ipv6,
                guard.fast_flap_policy,
                guard.fast_flap_tracker.clone(),
                guard.runtime_status.clone(),
            )
        };

        let iface_manager = { context.inner.lock().unwrap().iface_manager.clone() };

        let (_, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            runtime_status.lock().expect("tcp server runtime status mutex poisoned").mark_binding();
            let listener = bind_tcp_listener(addr.clone(), prefer_ipv6).await.map_err(|err| {
                runtime_status
                    .lock()
                    .expect("tcp server runtime status mutex poisoned")
                    .mark_bind_error(err.to_string());
                RnsError::ConnectionError
            });

            if listener.is_err() {
                log::warn!("couldn't bind to <{}>", addr);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            log::info!("listen on <{}>", addr);

            let listener = listener.unwrap();
            runtime_status
                .lock()
                .expect("tcp server runtime status mutex poisoned")
                .mark_listening();

            let tx_task = {
                let cancel = context.cancel.clone();
                let tx_channel = tx_channel.clone();

                tokio::spawn(async move {
                    loop {
                        if cancel.is_cancelled() {
                            break;
                        }

                        let mut tx_channel = tx_channel.lock().await;

                        tokio::select! {
                            _ = cancel.cancelled() => {
                                break;
                            }
                            // Skip all tx messages
                            _ = tx_channel.recv() => {}
                        }
                    }
                })
            };

            let cancel = context.cancel.clone();

            loop {
                if cancel.is_cancelled() {
                    break;
                }

                tokio::select! {
                    _ = cancel.cancelled() => {
                        break;
                    }

                    client = listener.accept() => {
                        match client {
                            Ok(client) => {
                                log::info!(
                                    "new client <{}> connected to <{}>",
                                    client.1,
                                    addr
                                );

                                let endpoint = client.1.to_string();
                                let peer_ip = client.1.ip().to_string();
                                if fast_flap_tracker.is_blocked(&peer_ip, fast_flap_policy) {
                                    log::warn!(
                                        "ignoring incoming connection from fast-flapping IP {}",
                                        peer_ip
                                    );
                                    continue;
                                }

                                let mut iface_manager = iface_manager.lock().await;
                                let accepted_client = TcpServer::accepted_client(
                                    endpoint.clone(),
                                    client.0,
                                    AcceptedClientOptions {
                                        client_mtu,
                                        client_socket_tuning,
                                        client_hdlc_watchdog: client_hdlc_watchdog.clone(),
                                        client_forced_bitrate_bps,
                                        fast_flap_tracker: Some(fast_flap_tracker.clone()),
                                        fast_flap_policy,
                                        peer_ip: Some(peer_ip),
                                    },
                                );
                                let child_status = accepted_client.runtime_status_handle();
                                let child_iface =
                                    iface_manager.spawn(accepted_client, TcpClient::spawn);
                                iface_manager.inherit_runtime_config(parent_iface, child_iface);
                                runtime_status
                                    .lock()
                                    .expect("tcp server runtime status mutex poisoned")
                                    .mark_accepted(endpoint, child_iface, child_status);
                            }
                            Err(err) => {
                                runtime_status
                                    .lock()
                                    .expect("tcp server runtime status mutex poisoned")
                                    .mark_accept_error(err.to_string());
                            }
                        };
                    }
                }
            }

            if let Err(error) = tx_task.await {
                log::error!("tcp server transmit task failed listener={addr}: {error}");
            }
        }
        runtime_status.lock().expect("tcp server runtime status mutex poisoned").mark_closed();
    }
}

impl Interface for TcpServer {
    fn mtu() -> usize {
        2048
    }
}

async fn bind_tcp_listener(addr: String, prefer_ipv6: bool) -> io::Result<TcpListener> {
    let addrs = prefer_ipv6_socket_addrs(lookup_host(addr.as_str()).await?, prefer_ipv6);
    let mut last_err = None;
    for addr in addrs {
        match bind_tcp_socket(addr) {
            Ok(listener) => return Ok(listener),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "TCP listener resolved to no addresses")
    }))
}

fn bind_tcp_socket(addr: SocketAddr) -> io::Result<TcpListener> {
    let domain = if addr.is_ipv6() { Domain::IPV6 } else { Domain::IPV4 };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    let std_listener: std::net::TcpListener = socket.into();
    TcpListener::from_std(std_listener)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{
        backbone_hdlc_watchdog, bind_tcp_listener, AcceptedClientOptions, FastFlapPolicy,
        FastFlapTracker, TcpClient, TcpServer, TcpSocketTuning,
    };
    use crate::iface::InterfaceManager;
    use tokio::net::{TcpListener, TcpStream};

    #[test]
    fn tcp_server_exposes_client_socket_tuning() {
        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let server = TcpServer::new("127.0.0.1:0", manager.clone());
        assert!(server.client_socket_tuning().is_empty());
        assert!(!server.client_hdlc_liveness_enabled());
        assert!(!server.prefer_ipv6());

        let tuned = TcpServer::new("127.0.0.1:0", manager)
            .with_client_socket_tuning(TcpSocketTuning::backbone())
            .with_prefer_ipv6(true);
        assert_eq!(tuned.client_socket_tuning().nodelay, Some(true));
        assert_eq!(tuned.client_socket_tuning().keepalive, Some(true));
        assert_eq!(tuned.client_socket_tuning().tcp_keepalive_idle, Some(Duration::from_secs(5)));
        assert_eq!(
            tuned.client_socket_tuning().tcp_keepalive_interval,
            Some(Duration::from_secs(2))
        );
        assert_eq!(tuned.client_socket_tuning().tcp_keepalive_retries, Some(12));
        assert_eq!(tuned.client_socket_tuning().tcp_user_timeout, Some(Duration::from_secs(24)));
        assert!(!tuned.client_hdlc_liveness_enabled());
        assert!(tuned.prefer_ipv6());
    }

    #[test]
    fn tcp_server_backbone_client_liveness_is_exposed() {
        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let server = TcpServer::new("127.0.0.1:0", manager).with_backbone_client_liveness();

        assert!(server.client_hdlc_liveness_enabled());
    }

    #[test]
    fn fast_flapping_blocks_after_grace_and_reports_live_ip_state() {
        let tracker = FastFlapTracker::default();
        let policy = FastFlapPolicy {
            enabled: true,
            threshold: Duration::from_secs(2),
            grace: 2,
            expiry: Duration::from_secs(60),
        };

        tracker.record_short_connection("192.0.2.7", Duration::from_millis(1), policy);
        tracker.record_short_connection("192.0.2.7", Duration::from_millis(1), policy);
        assert_eq!(tracker.blocked_ip_count(policy), 0);
        assert!(tracker.blocked_ip_list(policy).is_empty());

        tracker.record_short_connection("192.0.2.7", Duration::from_millis(1), policy);
        assert_eq!(tracker.blocked_ip_count(policy), 1);
        assert_eq!(tracker.blocked_ip_list(policy), vec!["192.0.2.7"]);
        assert!(tracker.is_blocked("192.0.2.7", policy));

        let disabled = FastFlapPolicy { enabled: false, ..policy };
        assert_eq!(tracker.blocked_ip_count(disabled), 0);
        assert!(tracker.blocked_ip_list(disabled).is_empty());
        assert!(!tracker.is_blocked("192.0.2.7", disabled));
    }

    #[tokio::test]
    async fn tcp_server_forwards_configured_liveness_to_accepted_clients() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let _peer = TcpStream::connect(addr).await.expect("connect peer");
        let (stream, peer_addr) = listener.accept().await.expect("accept stream");

        let ordinary = TcpServer::accepted_client(
            peer_addr.to_string(),
            stream,
            AcceptedClientOptions {
                client_mtu: TcpClient::DEFAULT_MTU,
                client_socket_tuning: TcpSocketTuning::default(),
                client_hdlc_watchdog: None,
                client_forced_bitrate_bps: None,
                fast_flap_tracker: None,
                fast_flap_policy: FastFlapPolicy::default(),
                peer_ip: None,
            },
        );
        assert!(!ordinary.hdlc_liveness_enabled());
        assert_eq!(ordinary.forced_bitrate_bps(), None);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let _peer = TcpStream::connect(addr).await.expect("connect peer");
        let (stream, peer_addr) = listener.accept().await.expect("accept stream");

        let backbone = TcpServer::accepted_client(
            peer_addr.to_string(),
            stream,
            AcceptedClientOptions {
                client_mtu: 1_048_576,
                client_socket_tuning: TcpSocketTuning::backbone(),
                client_hdlc_watchdog: Some(backbone_hdlc_watchdog()),
                client_forced_bitrate_bps: Some(9_600),
                fast_flap_tracker: None,
                fast_flap_policy: FastFlapPolicy::default(),
                peer_ip: None,
            },
        );

        assert_eq!(backbone.mtu_value(), 1_048_576);
        assert_eq!(backbone.socket_tuning().nodelay, Some(true));
        assert!(backbone.hdlc_liveness_enabled());
        assert_eq!(backbone.forced_bitrate_bps(), Some(9_600));
    }

    #[tokio::test]
    async fn tcp_server_runtime_status_tracks_accepted_client() {
        let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe listener");
        let addr = probe.local_addr().expect("probe listener addr");
        drop(probe);

        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let server = TcpServer::new(addr.to_string(), manager.clone())
            .with_backbone_client_liveness()
            .with_client_forced_bitrate(9_600);
        let status = server.runtime_status_handle();
        let parent_iface = manager.lock().await.spawn(server, TcpServer::spawn);

        wait_for_status(&status, |status| {
            status.get("listener_state").and_then(serde_json::Value::as_str) == Some("listening")
        })
        .await;
        let _peer = TcpStream::connect(addr).await.expect("connect peer");
        wait_for_status(&status, |status| {
            status.get("accepted_connections").and_then(serde_json::Value::as_u64) == Some(1)
        })
        .await;

        let snapshot = status.to_json();
        assert_eq!(snapshot["bind_addr"].as_str(), Some(addr.to_string().as_str()));
        assert_eq!(snapshot["client_liveness_enabled"].as_bool(), Some(true));
        assert_eq!(snapshot["client_forced_bitrate_bps"].as_u64(), Some(9_600));
        assert!(snapshot["latest_client_endpoint"].as_str().is_some());
        assert!(snapshot["latest_client_iface"].as_str().is_some());
        assert_eq!(snapshot["latest_stream_status"]["liveness_enabled"].as_bool(), Some(true));
        assert_eq!(snapshot["latest_stream_status"]["forced_bitrate_bps"].as_u64(), Some(9_600));

        manager.lock().await.stop_interface(parent_iface);
    }

    #[tokio::test]
    async fn tcp_listener_sets_reuse_address_for_ipv4() {
        let listener =
            bind_tcp_listener("127.0.0.1:0".to_string(), false).await.expect("bind listener");
        let std_listener = listener.into_std().expect("std listener");
        let socket: socket2::Socket = std_listener.into();

        assert!(socket.reuse_address().expect("reuse_address"));
    }

    #[tokio::test]
    async fn tcp_listener_sets_reuse_address_for_ipv6() {
        let listener = match bind_tcp_listener("[::1]:0".to_string(), true).await {
            Ok(listener) => listener,
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
                ) =>
            {
                return;
            }
            Err(err) => panic!("bind IPv6 listener: {err}"),
        };
        let std_listener = listener.into_std().expect("std listener");
        let socket: socket2::Socket = std_listener.into();

        assert!(socket.reuse_address().expect("reuse_address"));
    }

    async fn wait_for_status(
        status: &super::TcpListenerRuntimeStatusHandle,
        predicate: impl Fn(&serde_json::Value) -> bool,
    ) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = status.to_json();
            if predicate(&snapshot) {
                return;
            }
            assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {snapshot:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
