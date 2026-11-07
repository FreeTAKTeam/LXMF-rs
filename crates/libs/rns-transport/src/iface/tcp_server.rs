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

include!("tcp_server_parts/runtime_status.rs");

pub struct TcpServer {
    addr: String,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    client_mtu: usize,
    client_socket_tuning: TcpSocketTuning,
    client_hdlc_watchdog: Option<HdlcStreamWatchdog>,
    client_forced_bitrate_bps: Option<u64>,
    prefer_ipv6: bool,
    plaintext_shared_clients: bool,
    fast_flap_policy: FastFlapPolicy,
    fast_flap_tracker: FastFlapTracker,
    runtime_status: Arc<std::sync::Mutex<TcpListenerRuntimeStatus>>,
    startup_result: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
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
            plaintext_shared_clients: false,
            fast_flap_policy,
            fast_flap_tracker,
            startup_result: None,
        }
    }

    /// Use the native local shared-instance client's plaintext contract. This
    /// is opt-in; ordinary and explicitly authenticated TCP clients retain IFAC.
    #[must_use]
    pub fn with_plaintext_shared_clients(mut self, enabled: bool) -> Self {
        self.plaintext_shared_clients = enabled;
        self
    }

    /// Request the result of the listener's first bind attempt.
    pub fn with_startup_result(
        mut self,
    ) -> (Self, tokio::sync::oneshot::Receiver<Result<(), String>>) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.startup_result = Some(sender);
        (self, receiver)
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
}

include!("tcp_server_parts/worker.rs");

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
#[path = "tcp_server/tests/runtime.rs"]
mod tests;
