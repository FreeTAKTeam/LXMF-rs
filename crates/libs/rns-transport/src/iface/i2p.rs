use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use sha2::Digest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::hash::AddressHash;
use crate::iface::tcp_client::{
    run_hdlc_stream_with_runtime, HdlcStreamEvent, HdlcStreamRuntime, HdlcStreamWatchdog,
    HDLC_STREAM_EVENT_CHANNEL_CAPACITY,
};

use super::{
    IfaceRole, Interface, InterfaceContext, InterfaceManager, RxMessage, TxMessage, TxMessageType,
};

const DEFAULT_SAM_ADDR: &str = "127.0.0.1:7656";
const DEFAULT_MTU: usize = 1064;
const DEFAULT_RECONNECT_WAIT: Duration = Duration::from_secs(15);
const I2P_PROBE_AFTER: Duration = Duration::from_secs(10);
const I2P_PROBE_INTERVAL: Duration = Duration::from_secs(9);
const I2P_PROBES: u32 = 5;
const I2P_READ_TIMEOUT: Duration = Duration::from_secs(
    (I2P_PROBE_INTERVAL.as_secs() * I2P_PROBES as u64 + I2P_PROBE_AFTER.as_secs()) * 2,
);
const SAM_LINE_LIMIT: usize = 8192;
const I2P_CERT_LEN_OFFSET: usize = 385;
const I2P_CERT_LEN_SIZE: usize = 2;
const I2P_DEST_PREFIX_LEN: usize = 387;
const I2P_B32_ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
const I2P_MAX_CLOSED_INCOMING_PEERS: usize = 16;

#[derive(Clone)]
pub struct I2pInterface {
    name: String,
    sam_addr: String,
    peers: Vec<String>,
    connectable: bool,
    state_path: Option<PathBuf>,
    transport_identity_hash: Option<[u8; 16]>,
    mtu: usize,
    reconnect_wait: Duration,
    runtime_status: Arc<std::sync::Mutex<I2pRuntimeStatus>>,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2pTunnelState {
    Configured,
    Connecting,
    Connected,
    Listening,
    Reconnecting,
    Stale,
    Closed,
}

impl I2pTunnelState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Listening => "listening",
            Self::Reconnecting => "reconnecting",
            Self::Stale => "stale",
            Self::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2pPeerRuntimeStatus {
    pub peer: String,
    pub direction: String,
    pub iface: Option<AddressHash>,
    pub state: I2pTunnelState,
    pub reconnect_attempts: u64,
    pub last_error: Option<String>,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub keepalives_sent: u64,
    closed_sequence: Option<u64>,
}

impl I2pPeerRuntimeStatus {
    fn new_configured(peer: String) -> Self {
        Self {
            peer,
            direction: "outbound".to_string(),
            iface: None,
            state: I2pTunnelState::Configured,
            reconnect_attempts: 0,
            last_error: None,
            bytes_rx: 0,
            bytes_tx: 0,
            keepalives_sent: 0,
            closed_sequence: None,
        }
    }

    fn new_incoming(peer: String, iface: AddressHash) -> Self {
        Self {
            peer,
            direction: "incoming".to_string(),
            iface: Some(iface),
            state: I2pTunnelState::Connected,
            reconnect_attempts: 0,
            last_error: None,
            bytes_rx: 0,
            bytes_tx: 0,
            keepalives_sent: 0,
            closed_sequence: None,
        }
    }

    fn to_json(&self) -> serde_json::Value {
        let mut entry = serde_json::Map::new();
        entry.insert("peer".to_string(), serde_json::Value::String(self.peer.clone()));
        entry.insert("direction".to_string(), serde_json::Value::String(self.direction.clone()));
        entry.insert(
            "iface".to_string(),
            self.iface
                .map(|iface| serde_json::Value::String(iface.to_string()))
                .unwrap_or(serde_json::Value::Null),
        );
        entry.insert(
            "state".to_string(),
            serde_json::Value::String(self.state.as_str().to_string()),
        );
        entry.insert(
            "reconnect_attempts".to_string(),
            serde_json::Value::Number(self.reconnect_attempts.into()),
        );
        entry.insert(
            "last_error".to_string(),
            self.last_error
                .as_ref()
                .map(|err| serde_json::Value::String(err.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        entry.insert("bytes_rx".to_string(), serde_json::Value::Number(self.bytes_rx.into()));
        entry.insert("bytes_tx".to_string(), serde_json::Value::Number(self.bytes_tx.into()));
        entry.insert(
            "keepalives_sent".to_string(),
            serde_json::Value::Number(self.keepalives_sent.into()),
        );
        serde_json::Value::Object(entry)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2pRuntimeStatus {
    pub sam_endpoint: String,
    pub connectable: bool,
    pub configured_peer_count: usize,
    pub accept_state: I2pTunnelState,
    pub accept_reconnect_attempts: u64,
    pub last_accept_error: Option<String>,
    pub peers: BTreeMap<String, I2pPeerRuntimeStatus>,
    closed_incoming_sequence: u64,
}

#[derive(Clone)]
pub struct I2pRuntimeStatusHandle {
    inner: Arc<std::sync::Mutex<I2pRuntimeStatus>>,
}

impl I2pRuntimeStatusHandle {
    pub fn mark_accept_listening(&self) {
        self.inner.lock().expect("i2p runtime status mutex poisoned").mark_accept_listening();
    }

    pub fn mark_accept_closed(&self) {
        self.inner.lock().expect("i2p runtime status mutex poisoned").mark_accept_closed();
    }

    pub fn mark_outbound_connected(&self, peer: &str, iface: AddressHash) {
        self.inner
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .mark_outbound_connected(peer, iface);
    }

    pub fn mark_outbound_reconnecting(&self, peer: &str, iface: AddressHash, error: String) {
        self.inner
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .mark_outbound_reconnecting(peer, iface, error);
    }

    pub fn mark_incoming_connected(&self, peer: &str, iface: AddressHash) {
        self.inner
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .mark_incoming_connected(peer, iface);
    }

    pub fn mark_incoming_closed(&self, iface: AddressHash) {
        self.inner.lock().expect("i2p runtime status mutex poisoned").mark_incoming_closed(iface);
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().expect("i2p runtime status mutex poisoned").to_json()
    }
}

impl I2pRuntimeStatus {
    #[must_use]
    pub fn new(sam_endpoint: String, connectable: bool, peers: &[String]) -> Self {
        let mut status = Self {
            sam_endpoint,
            connectable,
            configured_peer_count: 0,
            accept_state: I2pTunnelState::Configured,
            accept_reconnect_attempts: 0,
            last_accept_error: None,
            peers: BTreeMap::new(),
            closed_incoming_sequence: 0,
        };
        status.set_configured_peers(peers);
        status
    }

    fn set_sam_endpoint(&mut self, sam_endpoint: String) {
        self.sam_endpoint = sam_endpoint;
    }

    fn set_connectable(&mut self, connectable: bool) {
        self.connectable = connectable;
        if !connectable {
            self.accept_state = I2pTunnelState::Closed;
        }
    }

    fn set_configured_peers(&mut self, peers: &[String]) {
        self.configured_peer_count = peers.len();
        self.peers.retain(|_, peer| peer.direction != "outbound");
        for peer in peers {
            self.peers.insert(peer.clone(), I2pPeerRuntimeStatus::new_configured(peer.clone()));
        }
    }

    pub fn mark_outbound_connecting(&mut self, peer: &str, iface: AddressHash) {
        let entry = self
            .peers
            .entry(peer.to_string())
            .or_insert_with(|| I2pPeerRuntimeStatus::new_configured(peer.to_string()));
        entry.iface = Some(iface);
        entry.state = I2pTunnelState::Connecting;
    }

    pub fn mark_outbound_connected(&mut self, peer: &str, iface: AddressHash) {
        let entry = self
            .peers
            .entry(peer.to_string())
            .or_insert_with(|| I2pPeerRuntimeStatus::new_configured(peer.to_string()));
        entry.iface = Some(iface);
        entry.state = I2pTunnelState::Connected;
        entry.last_error = None;
    }

    pub fn mark_outbound_reconnecting(&mut self, peer: &str, iface: AddressHash, error: String) {
        let entry = self
            .peers
            .entry(peer.to_string())
            .or_insert_with(|| I2pPeerRuntimeStatus::new_configured(peer.to_string()));
        entry.iface = Some(iface);
        entry.state = I2pTunnelState::Reconnecting;
        entry.reconnect_attempts = entry.reconnect_attempts.saturating_add(1);
        entry.last_error = Some(error);
    }

    pub fn mark_incoming_connected(&mut self, peer: &str, iface: AddressHash) {
        self.peers.insert(
            format!("incoming:{iface}"),
            I2pPeerRuntimeStatus::new_incoming(peer.to_string(), iface),
        );
    }

    pub fn mark_incoming_closed(&mut self, iface: AddressHash) {
        let key = format!("incoming:{iface}");
        if let Some(peer) = self.peers.get_mut(&key) {
            self.closed_incoming_sequence = self.closed_incoming_sequence.saturating_add(1);
            peer.state = I2pTunnelState::Closed;
            peer.closed_sequence = Some(self.closed_incoming_sequence);
        }
        self.prune_closed_incoming_peers();
    }

    pub fn mark_iface_closed(&mut self, iface: AddressHash) {
        let incoming_key = format!("incoming:{iface}");
        if self.peers.contains_key(&incoming_key) {
            self.mark_incoming_closed(iface);
            return;
        }
        for peer in self.peers.values_mut().filter(|peer| peer.iface == Some(iface)) {
            peer.state = I2pTunnelState::Closed;
        }
    }

    pub fn mark_accept_listening(&mut self) {
        self.accept_state = I2pTunnelState::Listening;
        self.last_accept_error = None;
    }

    pub fn mark_accept_reconnecting(&mut self, error: String) {
        self.accept_state = I2pTunnelState::Reconnecting;
        self.accept_reconnect_attempts = self.accept_reconnect_attempts.saturating_add(1);
        self.last_accept_error = Some(error);
    }

    pub fn mark_accept_closed(&mut self) {
        self.accept_state = I2pTunnelState::Closed;
        self.last_accept_error = None;
    }

    pub(crate) fn apply_peer_event(&mut self, key: &str, event: &HdlcStreamEvent) {
        let Some(peer) = self.peers.get_mut(key) else {
            return;
        };
        match event {
            HdlcStreamEvent::Read { bytes } => {
                peer.bytes_rx = peer.bytes_rx.saturating_add(*bytes as u64);
                if peer.state == I2pTunnelState::Stale {
                    peer.state = I2pTunnelState::Connected;
                }
            }
            HdlcStreamEvent::Write { bytes } => {
                peer.bytes_tx = peer.bytes_tx.saturating_add(*bytes as u64);
            }
            HdlcStreamEvent::Keepalive => {
                peer.keepalives_sent = peer.keepalives_sent.saturating_add(1);
            }
            HdlcStreamEvent::Active => {
                peer.state = I2pTunnelState::Connected;
            }
            HdlcStreamEvent::Stale => {
                peer.state = I2pTunnelState::Stale;
            }
            HdlcStreamEvent::ReadTimeout => {
                peer.state = I2pTunnelState::Reconnecting;
                peer.last_error = Some("i2p stream read timeout".to_string());
            }
            HdlcStreamEvent::Closed => {
                peer.state = I2pTunnelState::Closed;
            }
            HdlcStreamEvent::Error { message } => {
                peer.state = I2pTunnelState::Reconnecting;
                peer.last_error = Some(message.clone());
            }
        }
    }

    fn prune_closed_incoming_peers(&mut self) {
        let closed_count = self
            .peers
            .values()
            .filter(|peer| peer.direction == "incoming" && peer.state == I2pTunnelState::Closed)
            .count();
        if closed_count <= I2P_MAX_CLOSED_INCOMING_PEERS {
            return;
        }

        let prune_count = closed_count - I2P_MAX_CLOSED_INCOMING_PEERS;
        let mut closed_rows: Vec<(u64, String)> = self
            .peers
            .iter()
            .filter(|(_, peer)| {
                peer.direction == "incoming" && peer.state == I2pTunnelState::Closed
            })
            .map(|(key, peer)| (peer.closed_sequence.unwrap_or(u64::MAX), key.clone()))
            .collect();
        closed_rows.sort_by_key(|(sequence, _)| *sequence);
        let prune_keys: Vec<String> =
            closed_rows.into_iter().take(prune_count).map(|(_, key)| key).collect();
        for key in prune_keys {
            self.peers.remove(&key);
        }
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        root.insert(
            "sam_endpoint".to_string(),
            serde_json::Value::String(self.sam_endpoint.clone()),
        );
        root.insert("connectable".to_string(), serde_json::Value::Bool(self.connectable));
        root.insert(
            "configured_peer_count".to_string(),
            serde_json::Value::Number((self.configured_peer_count as u64).into()),
        );
        root.insert(
            "accept_state".to_string(),
            serde_json::Value::String(self.accept_state.as_str().to_string()),
        );
        root.insert(
            "accept_reconnect_attempts".to_string(),
            serde_json::Value::Number(self.accept_reconnect_attempts.into()),
        );
        root.insert(
            "last_accept_error".to_string(),
            self.last_accept_error
                .as_ref()
                .map(|err| serde_json::Value::String(err.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "peers".to_string(),
            serde_json::Value::Array(
                self.peers.values().map(I2pPeerRuntimeStatus::to_json).collect(),
            ),
        );
        serde_json::Value::Object(root)
    }
}

impl I2pInterface {
    pub const DEFAULT_MTU: usize = DEFAULT_MTU;
    pub const DEFAULT_SAM_PORT: u16 = 7656;

    #[must_use]
    pub fn new<T: Into<String>>(
        name: T,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        Self {
            name: name.into(),
            sam_addr: DEFAULT_SAM_ADDR.to_string(),
            peers: Vec::new(),
            connectable: false,
            state_path: None,
            transport_identity_hash: None,
            mtu: DEFAULT_MTU,
            reconnect_wait: DEFAULT_RECONNECT_WAIT,
            runtime_status: Arc::new(std::sync::Mutex::new(I2pRuntimeStatus::new(
                DEFAULT_SAM_ADDR.to_string(),
                false,
                &[],
            ))),
            iface_manager,
        }
    }

    #[must_use]
    pub fn with_sam_endpoint<T: Into<String>>(mut self, endpoint: T) -> Self {
        self.sam_addr = endpoint.into();
        self.runtime_status
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .set_sam_endpoint(self.sam_addr.clone());
        self
    }

    #[must_use]
    pub fn with_peers(mut self, peers: Vec<String>) -> Self {
        self.peers = peers
            .into_iter()
            .map(|peer| peer.trim().to_string())
            .filter(|peer| !peer.is_empty())
            .collect();
        self.runtime_status
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .set_configured_peers(&self.peers);
        self
    }

    #[must_use]
    pub fn with_connectable(mut self, connectable: bool) -> Self {
        self.connectable = connectable;
        self.runtime_status
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .set_connectable(connectable);
        self
    }

    #[must_use]
    pub fn with_state_path<T: Into<PathBuf>>(mut self, state_path: Option<T>) -> Self {
        self.state_path = state_path.map(Into::into);
        self
    }

    #[must_use]
    pub fn with_transport_identity_hash(mut self, identity_hash: Option<[u8; 16]>) -> Self {
        self.transport_identity_hash = identity_hash;
        self
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(256);
        self
    }

    #[must_use]
    pub fn with_reconnect_wait(mut self, reconnect_wait: Duration) -> Self {
        self.reconnect_wait = reconnect_wait;
        self
    }

    #[must_use]
    pub fn sam_addr(&self) -> &str {
        &self.sam_addr
    }

    #[must_use]
    pub fn peers(&self) -> &[String] {
        &self.peers
    }

    #[must_use]
    pub fn connectable(&self) -> bool {
        self.connectable
    }

    #[must_use]
    pub fn transport_identity_hash(&self) -> Option<[u8; 16]> {
        self.transport_identity_hash
    }

    #[must_use]
    pub fn mtu_value(&self) -> usize {
        self.mtu
    }

    #[must_use]
    pub fn runtime_status_json(&self) -> serde_json::Value {
        self.runtime_status.lock().expect("i2p runtime status mutex poisoned").to_json()
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> I2pRuntimeStatusHandle {
        I2pRuntimeStatusHandle { inner: self.runtime_status.clone() }
    }

    pub async fn preflight_sam(&self) -> Result<(), String> {
        let mut stream = TcpStream::connect(self.sam_addr.as_str()).await.map_err(|err| {
            format!("i2p SAM preflight connect failed endpoint={} err={}", self.sam_addr, err)
        })?;
        sam_hello(&mut stream).await.map_err(|err| {
            format!("i2p SAM preflight hello failed endpoint={} err={}", self.sam_addr, err)
        })
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let parent_iface = context.channel.address;
        let (
            name,
            sam_addr,
            peers,
            connectable,
            state_path,
            transport_identity_hash,
            mtu,
            reconnect_wait,
            runtime_status,
            iface_manager,
        ) = {
            let guard = context.inner.lock().expect("i2p interface mutex poisoned");
            (
                guard.name.clone(),
                guard.sam_addr.clone(),
                guard.peers.clone(),
                guard.connectable,
                guard.state_path.clone(),
                guard.transport_identity_hash,
                guard.mtu,
                guard.reconnect_wait,
                guard.runtime_status.clone(),
                guard.iface_manager.clone(),
            )
        };
        let (rx_channel, tx_channel) = context.channel.split();

        let peer_routes = Arc::new(tokio::sync::Mutex::new(BTreeMap::<
            AddressHash,
            tokio::sync::mpsc::Sender<TxMessage>,
        >::new()));
        for peer in peers {
            let Some(child_iface) = iface_manager
                .lock()
                .await
                .register_virtual_iface(parent_iface, IfaceRole::VirtualUnicast)
            else {
                log::warn!("failed to register I2P virtual peer iface for {}", peer);
                continue;
            };
            let (peer_tx, peer_rx) = tokio::sync::mpsc::channel(128);
            peer_routes.lock().await.insert(child_iface, peer_tx);

            tokio::spawn(run_i2p_peer_loop(
                peer,
                child_iface,
                sam_addr.clone(),
                transport_identity_hash,
                mtu,
                reconnect_wait,
                runtime_status.clone(),
                context.cancel.clone(),
                iface_stop.clone(),
                rx_channel.clone(),
                peer_rx,
            ));
        }

        if connectable {
            tokio::spawn(run_i2p_accept_loop(
                parent_iface,
                name,
                sam_addr.clone(),
                state_path.clone(),
                transport_identity_hash,
                mtu,
                reconnect_wait,
                runtime_status.clone(),
                context.cancel.clone(),
                iface_stop.clone(),
                rx_channel.clone(),
                iface_manager.clone(),
                peer_routes.clone(),
            ));
        }

        let mut tx_channel = tx_channel;
        loop {
            tokio::select! {
                _ = context.cancel.cancelled() => break,
                _ = iface_stop.cancelled() => break,
                Some(message) = tx_channel.recv() => {
                    match message.tx_type {
                        TxMessageType::Broadcast(_) => {
                            let senders = peer_routes.lock().await.values().cloned().collect::<Vec<_>>();
                            for sender in senders {
                                if let Err(err) = sender.try_send(message.clone()) {
                                    log::warn!("failed to enqueue I2P broadcast packet: {err}");
                                }
                            }
                        }
                        TxMessageType::Direct(address) => {
                            let sender = peer_routes.lock().await.get(&address).cloned();
                            if let Some(sender) = sender {
                                if let Err(err) = sender.send(message).await {
                                    log::warn!(
                                        "failed to enqueue I2P direct packet iface={address}: {err}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        iface_stop.cancel();
        cleanup_i2p_peer_routes(&iface_manager, &peer_routes, &runtime_status).await;
    }
}

impl Interface for I2pInterface {
    fn mtu() -> usize {
        DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_i2p_peer_loop(
    peer: String,
    iface_address: AddressHash,
    sam_addr: String,
    transport_identity_hash: Option<[u8; 16]>,
    mtu: usize,
    reconnect_wait: Duration,
    runtime_status: Arc<std::sync::Mutex<I2pRuntimeStatus>>,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    peer_rx: tokio::sync::mpsc::Receiver<TxMessage>,
) {
    let peer_rx = Arc::new(tokio::sync::Mutex::new(peer_rx));
    let session_id = sam_session_id(iface_address, transport_identity_hash.as_ref());

    loop {
        if cancel.is_cancelled() || iface_stop.is_cancelled() {
            break;
        }

        runtime_status
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .mark_outbound_connecting(&peer, iface_address);
        let (_session_stream, stream) =
            match open_sam_stream(sam_addr.as_str(), session_id.as_str(), peer.as_str()).await {
                Ok(streams) => streams,
                Err(err) => {
                    runtime_status
                        .lock()
                        .expect("i2p runtime status mutex poisoned")
                        .mark_outbound_reconnecting(&peer, iface_address, err.to_string());
                    log::warn!(
                        "failed to open I2P SAM stream peer={} sam={} iface={} err={}",
                        peer,
                        sam_addr,
                        iface_address,
                        err
                    );
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(reconnect_wait) => {}
                    }
                    continue;
                }
            };

        runtime_status
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .mark_outbound_connected(&peer, iface_address);
        log::info!("I2P SAM stream connected peer={} iface={}", peer, iface_address);
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(HDLC_STREAM_EVENT_CHANNEL_CAPACITY);
        let status_task =
            tokio::spawn(track_i2p_stream_events(peer.clone(), runtime_status.clone(), event_rx));
        let (read_stream, write_stream) = stream.into_split();
        run_hdlc_stream_with_runtime(
            "i2p".to_string(),
            iface_address,
            mtu,
            cancel.clone(),
            iface_stop.clone(),
            rx_channel.clone(),
            peer_rx.clone(),
            read_stream,
            write_stream,
            i2p_hdlc_runtime(event_tx),
        )
        .await;
        let _ = status_task.await;
        log::info!("I2P SAM stream disconnected peer={} iface={}", peer, iface_address);
    }
}

async fn cleanup_i2p_peer_routes(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    peer_routes: &Arc<
        tokio::sync::Mutex<BTreeMap<AddressHash, tokio::sync::mpsc::Sender<TxMessage>>>,
    >,
    runtime_status: &Arc<std::sync::Mutex<I2pRuntimeStatus>>,
) {
    let ifaces = {
        let mut routes = peer_routes.lock().await;
        let ifaces = routes.keys().copied().collect::<Vec<_>>();
        routes.clear();
        ifaces
    };

    if ifaces.is_empty() {
        return;
    }

    for iface in &ifaces {
        let _ = iface_manager.lock().await.stop_interface(*iface);
    }
    let mut status = runtime_status.lock().expect("i2p runtime status mutex poisoned");
    for iface in ifaces {
        status.mark_iface_closed(iface);
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_i2p_accept_loop(
    parent_iface: AddressHash,
    name: String,
    sam_addr: String,
    state_path: Option<PathBuf>,
    transport_identity_hash: Option<[u8; 16]>,
    mtu: usize,
    reconnect_wait: Duration,
    runtime_status: Arc<std::sync::Mutex<I2pRuntimeStatus>>,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    peer_routes: Arc<
        tokio::sync::Mutex<BTreeMap<AddressHash, tokio::sync::mpsc::Sender<TxMessage>>>,
    >,
) {
    let session_id =
        format!("{}-accept", sam_session_id(parent_iface, transport_identity_hash.as_ref()));

    loop {
        if cancel.is_cancelled() || iface_stop.is_cancelled() {
            break;
        }

        let session_destination = match connectable_session_destination_with_identity(
            sam_addr.as_str(),
            &name,
            state_path.as_ref(),
            transport_identity_hash.as_ref(),
        )
        .await
        {
            Ok(destination) => destination,
            Err(err) => {
                runtime_status
                    .lock()
                    .expect("i2p runtime status mutex poisoned")
                    .mark_accept_reconnecting(err.to_string());
                log::warn!(
                    "failed to prepare I2P connectable destination sam={} iface={} err={}",
                    sam_addr,
                    parent_iface,
                    err
                );
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = iface_stop.cancelled() => break,
                    _ = tokio::time::sleep(reconnect_wait) => {}
                }
                continue;
            }
        };

        let (session_stream, destination) =
            match create_sam_session(sam_addr.as_str(), session_id.as_str(), &session_destination)
                .await
            {
                Ok(session) => session,
                Err(err) => {
                    runtime_status
                        .lock()
                        .expect("i2p runtime status mutex poisoned")
                        .mark_accept_reconnecting(err.to_string());
                    log::warn!(
                        "failed to create I2P SAM accept session sam={} iface={} err={}",
                        sam_addr,
                        parent_iface,
                        err
                    );
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(reconnect_wait) => {}
                    }
                    continue;
                }
            };

        runtime_status.lock().expect("i2p runtime status mutex poisoned").mark_accept_listening();
        log::info!(
            "I2P SAM accept session ready iface={} destination={}",
            parent_iface,
            destination
        );
        run_i2p_accept_session(
            session_stream,
            session_id.as_str(),
            sam_addr.as_str(),
            parent_iface,
            mtu,
            reconnect_wait,
            runtime_status.clone(),
            cancel.clone(),
            iface_stop.clone(),
            rx_channel.clone(),
            iface_manager.clone(),
            peer_routes.clone(),
        )
        .await;
    }
    runtime_status.lock().expect("i2p runtime status mutex poisoned").mark_accept_closed();
}

#[allow(clippy::too_many_arguments)]
async fn run_i2p_accept_session(
    _session_stream: TcpStream,
    session_id: &str,
    sam_addr: &str,
    parent_iface: AddressHash,
    mtu: usize,
    reconnect_wait: Duration,
    runtime_status: Arc<std::sync::Mutex<I2pRuntimeStatus>>,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    peer_routes: Arc<
        tokio::sync::Mutex<BTreeMap<AddressHash, tokio::sync::mpsc::Sender<TxMessage>>>,
    >,
) {
    loop {
        if cancel.is_cancelled() || iface_stop.is_cancelled() {
            break;
        }

        let (remote_destination, stream) = match accept_sam_stream(sam_addr, session_id).await {
            Ok(accepted) => accepted,
            Err(err) => {
                let recreate_session = accept_error_requires_session_recreate(&err);
                runtime_status
                    .lock()
                    .expect("i2p runtime status mutex poisoned")
                    .mark_accept_reconnecting(err.to_string());
                log::warn!(
                    "failed to accept I2P SAM stream sam={} iface={} err={}",
                    sam_addr,
                    parent_iface,
                    err
                );
                if recreate_session {
                    break;
                }
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = iface_stop.cancelled() => break,
                    _ = tokio::time::sleep(reconnect_wait) => {}
                }
                continue;
            }
        };

        let Some(child_iface) = iface_manager
            .lock()
            .await
            .register_virtual_iface(parent_iface, IfaceRole::VirtualUnicast)
        else {
            log::warn!("failed to register incoming I2P virtual iface for {}", remote_destination);
            continue;
        };
        let (peer_tx, peer_rx) = tokio::sync::mpsc::channel(128);
        peer_routes.lock().await.insert(child_iface, peer_tx);
        runtime_status
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .mark_incoming_connected(&remote_destination, child_iface);

        log::info!(
            "I2P SAM incoming stream accepted peer={} iface={}",
            remote_destination,
            child_iface
        );
        tokio::spawn(run_i2p_accepted_stream(
            remote_destination,
            child_iface,
            mtu,
            runtime_status.clone(),
            cancel.clone(),
            iface_stop.clone(),
            rx_channel.clone(),
            peer_routes.clone(),
            iface_manager.clone(),
            stream,
            peer_rx,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_i2p_accepted_stream(
    remote_destination: String,
    child_iface: AddressHash,
    mtu: usize,
    runtime_status: Arc<std::sync::Mutex<I2pRuntimeStatus>>,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    peer_routes: Arc<
        tokio::sync::Mutex<BTreeMap<AddressHash, tokio::sync::mpsc::Sender<TxMessage>>>,
    >,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    stream: TcpStream,
    peer_rx: tokio::sync::mpsc::Receiver<TxMessage>,
) {
    let peer_rx = Arc::new(tokio::sync::Mutex::new(peer_rx));
    let status_key = format!("incoming:{child_iface}");
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(HDLC_STREAM_EVENT_CHANNEL_CAPACITY);
    let status_task =
        tokio::spawn(track_i2p_stream_events(status_key, runtime_status.clone(), event_rx));
    let (read_stream, write_stream) = stream.into_split();
    run_hdlc_stream_with_runtime(
        "i2p_accept".to_string(),
        child_iface,
        mtu,
        cancel,
        iface_stop,
        rx_channel,
        peer_rx,
        read_stream,
        write_stream,
        i2p_hdlc_runtime(event_tx),
    )
    .await;
    let _ = status_task.await;
    runtime_status
        .lock()
        .expect("i2p runtime status mutex poisoned")
        .mark_incoming_closed(child_iface);
    peer_routes.lock().await.remove(&child_iface);
    let _ = iface_manager.lock().await.stop_interface(child_iface);
    log::info!("I2P SAM incoming stream closed peer={} iface={}", remote_destination, child_iface);
}

fn i2p_hdlc_runtime(events: tokio::sync::mpsc::Sender<HdlcStreamEvent>) -> HdlcStreamRuntime {
    HdlcStreamRuntime::new()
        .with_watchdog(HdlcStreamWatchdog {
            keepalive_after: I2P_PROBE_AFTER,
            stale_after: I2P_PROBE_AFTER * 2,
            read_timeout: I2P_READ_TIMEOUT,
        })
        .with_events(events)
}

async fn track_i2p_stream_events(
    peer_key: String,
    runtime_status: Arc<std::sync::Mutex<I2pRuntimeStatus>>,
    mut events: tokio::sync::mpsc::Receiver<HdlcStreamEvent>,
) {
    while let Some(event) = events.recv().await {
        runtime_status
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .apply_peer_event(&peer_key, &event);
    }
}

pub(crate) async fn open_sam_stream(
    sam_addr: &str,
    session_id: &str,
    destination: &str,
) -> io::Result<(TcpStream, TcpStream)> {
    let (session_stream, _) = create_sam_session(sam_addr, session_id, "TRANSIENT").await?;
    let destination = resolve_sam_destination(sam_addr, destination).await?;
    let mut stream = TcpStream::connect(sam_addr).await?;
    sam_hello(&mut stream).await?;
    write_sam_line(
        &mut stream,
        format!("STREAM CONNECT ID={session_id} DESTINATION={destination} SILENT=false").as_str(),
    )
    .await?;
    expect_sam_ok(&mut stream, "STREAM STATUS").await?;
    Ok((session_stream, stream))
}

pub(crate) async fn accept_sam_stream(
    sam_addr: &str,
    session_id: &str,
) -> io::Result<(String, TcpStream)> {
    let mut stream = TcpStream::connect(sam_addr).await?;
    sam_hello(&mut stream).await?;
    write_sam_line(&mut stream, format!("STREAM ACCEPT ID={session_id} SILENT=false").as_str())
        .await?;
    expect_sam_ok(&mut stream, "STREAM STATUS").await?;
    let remote_line = read_sam_line(&mut stream).await?;
    if remote_line.starts_with("STREAM STATUS") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("SAM accept failed after status ok: {remote_line}"),
        ));
    }
    let remote_destination = remote_line
        .split_ascii_whitespace()
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "SAM accept missing peer destination")
        })?
        .to_string();
    Ok((remote_destination, stream))
}

fn accept_error_requires_session_recreate(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::InvalidData && err.to_string().contains("is not a STREAM session")
}

pub async fn connectable_session_destination(
    sam_addr: &str,
    name: &str,
    state_path: Option<&PathBuf>,
) -> io::Result<String> {
    connectable_session_destination_with_identity(sam_addr, name, state_path, None).await
}

pub async fn connectable_session_destination_with_identity(
    sam_addr: &str,
    name: &str,
    state_path: Option<&PathBuf>,
    transport_identity_hash: Option<&[u8; 16]>,
) -> io::Result<String> {
    let Some(path) = state_path
        .map(|root| i2p_private_key_path_with_identity(root, name, transport_identity_hash))
    else {
        return Ok("TRANSIENT".to_string());
    };
    if let Ok(value) = fs::read_to_string(path.as_path()) {
        let value = value.trim();
        if !value.is_empty() {
            return Ok(value.to_string());
        }
    }

    let (_public, private) = generate_sam_destination(sam_addr).await?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path.as_path(), private.as_bytes())?;
    Ok(private)
}

async fn generate_sam_destination(sam_addr: &str) -> io::Result<(String, String)> {
    let mut stream = TcpStream::connect(sam_addr).await?;
    sam_hello(&mut stream).await?;
    write_sam_line(&mut stream, "DEST GENERATE SIGNATURE_TYPE=7").await?;
    let response = expect_sam_prefix_line(&mut stream, "DEST REPLY").await?;
    let public = sam_value(response.as_str(), "PUB").map(str::to_string).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("SAM DEST REPLY missing PUB: {response}"),
        )
    })?;
    let private = sam_value(response.as_str(), "PRIV").map(str::to_string).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("SAM DEST REPLY missing PRIV: {response}"),
        )
    })?;
    Ok((public, private))
}

pub fn i2p_private_key_path(root: &Path, name: &str) -> PathBuf {
    i2p_private_key_old_format_path(root, name)
}

pub fn i2p_private_key_path_with_identity(
    root: &Path,
    name: &str,
    transport_identity_hash: Option<&[u8; 16]>,
) -> PathBuf {
    let old_format = i2p_private_key_old_format_path(root, name);
    if old_format.exists() {
        return old_format;
    }
    transport_identity_hash.map_or(old_format, |identity_hash| {
        i2p_private_key_new_format_path(root, name, identity_hash)
    })
}

pub fn i2p_private_key_old_format_path(root: &Path, name: &str) -> PathBuf {
    root.join("i2p").join(format!("{}.i2p", i2p_old_format_key_stem(name)))
}

pub fn i2p_private_key_new_format_path(
    root: &Path,
    name: &str,
    transport_identity_hash: &[u8; 16],
) -> PathBuf {
    root.join("i2p").join(format!("{}.i2p", i2p_new_format_key_stem(name, transport_identity_hash)))
}

pub fn i2p_old_format_key_stem(name: &str) -> String {
    hex_lower(&i2p_full_hash(&i2p_full_hash(name.as_bytes())))
}

pub fn i2p_new_format_key_stem(name: &str, transport_identity_hash: &[u8; 16]) -> String {
    let mut material = Vec::with_capacity(64);
    material.extend_from_slice(&i2p_full_hash(name.as_bytes()));
    material.extend_from_slice(&i2p_full_hash(transport_identity_hash));
    hex_lower(&i2p_full_hash(&material))
}

fn i2p_full_hash(data: &[u8]) -> [u8; 32] {
    let digest = sha2::Sha256::digest(data);
    let mut out = [0_u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn i2p_b32_from_private_destination(private_destination: &str) -> Result<String, String> {
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::Alphabet::new(
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-~",
        )
        .map_err(|err| format!("invalid i2p base64 alphabet: {err}"))?,
        base64::engine::general_purpose::PAD,
    );
    let decoded = engine
        .decode(private_destination.trim())
        .map_err(|err| format!("invalid i2p private destination base64: {err}"))?;
    if decoded.len() < I2P_DEST_PREFIX_LEN {
        return Err("i2p private destination is too short".to_string());
    }
    let cert_len = u16::from_be_bytes([
        decoded[I2P_CERT_LEN_OFFSET],
        decoded[I2P_CERT_LEN_OFFSET + I2P_CERT_LEN_SIZE - 1],
    ]) as usize;
    let public_len = I2P_DEST_PREFIX_LEN
        .checked_add(cert_len)
        .ok_or_else(|| "i2p destination certificate length overflowed".to_string())?;
    if decoded.len() < public_len {
        return Err("i2p private destination certificate is truncated".to_string());
    }
    let digest = sha2::Sha256::digest(&decoded[..public_len]);
    Ok(format!("{}.b32.i2p", base32_no_pad_lower(digest.as_slice())))
}

fn base32_no_pad_lower(input: &[u8]) -> String {
    let mut output = String::with_capacity((input.len() * 8).div_ceil(5));
    let mut buffer = 0_u16;
    let mut bits = 0_u8;
    for byte in input {
        buffer = (buffer << 8) | u16::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let index = ((buffer >> bits) & 0x1f) as usize;
            output.push(I2P_B32_ALPHABET[index] as char);
        }
    }
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0x1f) as usize;
        output.push(I2P_B32_ALPHABET[index] as char);
    }
    output
}

async fn create_sam_session(
    sam_addr: &str,
    session_id: &str,
    destination: &str,
) -> io::Result<(TcpStream, String)> {
    let mut stream = TcpStream::connect(sam_addr).await?;
    sam_hello(&mut stream).await?;
    write_sam_line(
        &mut stream,
        format!("SESSION CREATE STYLE=STREAM ID={session_id} DESTINATION={destination}").as_str(),
    )
    .await?;
    let response = expect_sam_ok_line(&mut stream, "SESSION STATUS").await?;
    let destination =
        sam_value(response.as_str(), "DESTINATION").unwrap_or(destination).to_string();
    Ok((stream, destination))
}

async fn resolve_sam_destination(sam_addr: &str, destination: &str) -> io::Result<String> {
    if !destination.ends_with(".i2p") {
        return Ok(destination.to_string());
    }
    let mut stream = TcpStream::connect(sam_addr).await?;
    sam_hello(&mut stream).await?;
    write_sam_line(&mut stream, format!("NAMING LOOKUP NAME={destination}").as_str()).await?;
    let response = expect_sam_ok_line(&mut stream, "NAMING REPLY").await?;
    sam_value(response.as_str(), "VALUE").map(str::to_string).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("SAM naming reply missing VALUE: {response}"),
        )
    })
}

async fn sam_hello(stream: &mut TcpStream) -> io::Result<()> {
    write_sam_line(stream, "HELLO VERSION MIN=3.0 MAX=3.3").await?;
    expect_sam_ok(stream, "HELLO REPLY").await
}

async fn write_sam_line(stream: &mut TcpStream, line: &str) -> io::Result<()> {
    stream.write_all(line.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await
}

async fn expect_sam_ok(stream: &mut TcpStream, prefix: &str) -> io::Result<()> {
    let _ = expect_sam_ok_line(stream, prefix).await?;
    Ok(())
}

async fn expect_sam_ok_line(stream: &mut TcpStream, prefix: &str) -> io::Result<String> {
    let line = read_sam_line(stream).await?;
    if sam_line_ok(line.as_str(), prefix) {
        Ok(line)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected SAM response for {prefix}: {line}"),
        ))
    }
}

async fn expect_sam_prefix_line(stream: &mut TcpStream, prefix: &str) -> io::Result<String> {
    let line = read_sam_line(stream).await?;
    if line.starts_with(prefix) {
        Ok(line)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected SAM response for {prefix}: {line}"),
        ))
    }
}

async fn read_sam_line(stream: &mut TcpStream) -> io::Result<String> {
    let mut line = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        let read = stream.read(&mut byte).await?;
        if read == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "SAM connection closed"));
        }
        if byte[0] == b'\n' {
            break;
        }
        if byte[0] != b'\r' {
            line.push(byte[0]);
        }
        if line.len() > SAM_LINE_LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "SAM response line too long"));
        }
    }
    String::from_utf8(line).map_err(|err| {
        io::Error::new(io::ErrorKind::InvalidData, format!("invalid SAM utf8: {err}"))
    })
}

fn sam_line_ok(line: &str, prefix: &str) -> bool {
    line.starts_with(prefix)
        && line.split_ascii_whitespace().any(|part| part.eq_ignore_ascii_case("RESULT=OK"))
}

fn sam_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let key = format!("{key}=");
    line.split_ascii_whitespace().find_map(|part| part.strip_prefix(key.as_str()))
}

fn sam_session_id(iface_address: AddressHash, identity_hash: Option<&[u8; 16]>) -> String {
    let hex = iface_address.to_string();
    let trimmed = hex.trim_matches('/');
    let suffix = trimmed.get(..16).unwrap_or(trimmed);
    let Some(identity_hash) = identity_hash else {
        return format!("lxmf-rs-{suffix}");
    };

    let mut identity_suffix = String::with_capacity(16);
    for byte in identity_hash.iter().take(8) {
        identity_suffix.push_str(format!("{byte:02x}").as_str());
    }
    format!("lxmf-rs-{suffix}-{identity_suffix}")
}

#[cfg(test)]
mod tests {
    use super::{
        accept_error_requires_session_recreate, accept_sam_stream, base32_no_pad_lower,
        cleanup_i2p_peer_routes, connectable_session_destination_with_identity, create_sam_session,
        i2p_b32_from_private_destination, i2p_new_format_key_stem, i2p_old_format_key_stem,
        i2p_private_key_new_format_path, i2p_private_key_old_format_path,
        i2p_private_key_path_with_identity, open_sam_stream, run_i2p_accept_loop,
        run_i2p_peer_loop, sam_session_id, HdlcStreamEvent, I2pInterface, I2pRuntimeStatus,
        I2pTunnelState, I2P_CERT_LEN_OFFSET, I2P_DEST_PREFIX_LEN, I2P_MAX_CLOSED_INCOMING_PEERS,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::buffer::OutputBuffer;
    use crate::iface::{hdlc::Hdlc, IfaceRole, TxMessage, TxMessageType};
    use crate::packet::Packet;
    use base64::Engine;
    use sha2::Digest;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio_util::sync::CancellationToken;

    fn hdlc_frame_for_packet(packet: &Packet) -> Vec<u8> {
        let raw = packet.to_bytes().expect("serialize packet");
        let mut buffer = vec![0_u8; raw.len().saturating_mul(2).saturating_add(2)];
        let mut output = OutputBuffer::new(&mut buffer);
        Hdlc::encode(&raw, &mut output).expect("encode hdlc frame");
        output.as_slice().to_vec()
    }

    #[test]
    fn i2p_defaults_match_python_slice() {
        let manager =
            std::sync::Arc::new(tokio::sync::Mutex::new(crate::iface::InterfaceManager::new(1)));
        let iface = I2pInterface::new("i2p-main", manager);
        assert_eq!(iface.name(), "i2p-main");
        assert_eq!(iface.sam_addr(), "127.0.0.1:7656");
        assert_eq!(iface.mtu_value(), 1064);
        assert!(iface.peers().is_empty());
        assert!(!iface.connectable());
    }

    #[test]
    fn i2p_runtime_status_tracks_peer_accept_and_watchdog_events() {
        let peers = vec!["exampledestination.b32.i2p".to_string()];
        let mut status = I2pRuntimeStatus::new("127.0.0.1:7656".to_string(), true, &peers);
        let peer_iface = crate::hash::AddressHash::new([0x11; 16]);

        status.mark_outbound_connecting(&peers[0], peer_iface);
        assert_eq!(status.peers[&peers[0]].state, I2pTunnelState::Connecting);
        status.mark_outbound_connected(&peers[0], peer_iface);
        status.apply_peer_event(&peers[0], &HdlcStreamEvent::Write { bytes: 7 });
        status.apply_peer_event(&peers[0], &HdlcStreamEvent::Read { bytes: 5 });
        status.apply_peer_event(&peers[0], &HdlcStreamEvent::Keepalive);
        status.apply_peer_event(&peers[0], &HdlcStreamEvent::Stale);
        assert_eq!(status.peers[&peers[0]].state, I2pTunnelState::Stale);
        status.apply_peer_event(&peers[0], &HdlcStreamEvent::Active);
        assert_eq!(status.peers[&peers[0]].state, I2pTunnelState::Connected);
        status.apply_peer_event(&peers[0], &HdlcStreamEvent::ReadTimeout);
        assert_eq!(status.peers[&peers[0]].state, I2pTunnelState::Reconnecting);
        assert_eq!(status.peers[&peers[0]].last_error.as_deref(), Some("i2p stream read timeout"));

        let incoming_iface = crate::hash::AddressHash::new([0x22; 16]);
        status.mark_accept_listening();
        status.mark_incoming_connected("remote-destination", incoming_iface);
        status.apply_peer_event(
            format!("incoming:{incoming_iface}").as_str(),
            &HdlcStreamEvent::Closed,
        );
        status.mark_accept_reconnecting("accept failed".to_string());

        let snapshot = status.to_json();
        assert_eq!(snapshot["sam_endpoint"].as_str(), Some("127.0.0.1:7656"));
        assert_eq!(snapshot["connectable"].as_bool(), Some(true));
        assert_eq!(snapshot["configured_peer_count"].as_u64(), Some(1));
        assert_eq!(snapshot["accept_state"].as_str(), Some("reconnecting"));
        assert_eq!(snapshot["accept_reconnect_attempts"].as_u64(), Some(1));
        assert_eq!(snapshot["last_accept_error"].as_str(), Some("accept failed"));
        let peer_rows = snapshot["peers"].as_array().expect("peer rows");
        assert_eq!(peer_rows.len(), 2);
        let configured = peer_rows
            .iter()
            .find(|row| row["direction"].as_str() == Some("outbound"))
            .expect("configured peer row");
        assert_eq!(configured["state"].as_str(), Some("reconnecting"));
        assert_eq!(configured["bytes_rx"].as_u64(), Some(5));
        assert_eq!(configured["bytes_tx"].as_u64(), Some(7));
        assert_eq!(configured["keepalives_sent"].as_u64(), Some(1));
        let incoming = peer_rows
            .iter()
            .find(|row| row["direction"].as_str() == Some("incoming"))
            .expect("incoming peer row");
        assert_eq!(incoming["peer"].as_str(), Some("remote-destination"));
        assert_eq!(incoming["state"].as_str(), Some("closed"));
    }

    #[test]
    fn i2p_runtime_status_prunes_old_closed_incoming_rows() {
        let peers = vec!["configured-peer.b32.i2p".to_string()];
        let mut status = I2pRuntimeStatus::new("127.0.0.1:7656".to_string(), true, &peers);
        let active_iface = crate::hash::AddressHash::new([0xFE; 16]);
        status.mark_incoming_connected("still-open", active_iface);

        for index in 0..(I2P_MAX_CLOSED_INCOMING_PEERS + 4) {
            let mut bytes = [0_u8; 16];
            bytes[15] = index as u8;
            let iface = crate::hash::AddressHash::new(bytes);
            status.mark_incoming_connected(format!("closed-{index}").as_str(), iface);
            status.mark_incoming_closed(iface);
        }

        let snapshot = status.to_json();
        let peer_rows = snapshot["peers"].as_array().expect("peer rows");
        let closed_incoming: Vec<&serde_json::Value> = peer_rows
            .iter()
            .filter(|row| {
                row["direction"].as_str() == Some("incoming")
                    && row["state"].as_str() == Some("closed")
            })
            .collect();

        assert_eq!(closed_incoming.len(), I2P_MAX_CLOSED_INCOMING_PEERS);
        assert!(peer_rows
            .iter()
            .any(|row| row["peer"].as_str() == Some("configured-peer.b32.i2p")));
        assert!(peer_rows.iter().any(|row| row["peer"].as_str() == Some("still-open")));
        assert!(!peer_rows.iter().any(|row| row["peer"].as_str() == Some("closed-0")));
        assert!(!peer_rows.iter().any(|row| row["peer"].as_str() == Some("closed-3")));
        assert!(peer_rows.iter().any(|row| row["peer"].as_str() == Some("closed-4")));
    }

    #[test]
    fn i2p_accept_recreates_session_when_sam_session_expires() {
        let expired = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unexpected SAM response for STREAM STATUS: STREAM STATUS RESULT=I2P_ERROR MESSAGE=\"specified ID lxmf-rs-accept is not a STREAM session\"",
        );
        assert!(accept_error_requires_session_recreate(&expired));

        let timeout = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "SAM accept failed after status ok: STREAM STATUS RESULT=TIMEOUT",
        );
        assert!(!accept_error_requires_session_recreate(&timeout));
    }

    #[test]
    fn i2p_sam_session_id_includes_transport_identity_when_available() {
        let iface_address = crate::hash::AddressHash::new([0x33; 16]);
        assert_eq!(sam_session_id(iface_address, None), "lxmf-rs-3333333333333333");

        let identity_hash = [
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80,
            0x90, 0xA0,
        ];
        assert_eq!(
            sam_session_id(iface_address, Some(&identity_hash)),
            "lxmf-rs-3333333333333333-aabbccddeeff1020"
        );
    }

    #[tokio::test]
    async fn i2p_parent_shutdown_cleans_registered_virtual_peer_ifaces() {
        let iface_manager =
            Arc::new(tokio::sync::Mutex::new(crate::iface::InterfaceManager::new(8)));
        let parent_iface = {
            let mut manager = iface_manager.lock().await;
            manager.new_channel_with_role(8, crate::iface::IfaceRole::Multicast).address
        };
        let outbound_iface = iface_manager
            .lock()
            .await
            .register_virtual_iface(parent_iface, crate::iface::IfaceRole::VirtualUnicast)
            .expect("outbound virtual iface");
        let incoming_iface = iface_manager
            .lock()
            .await
            .register_virtual_iface(parent_iface, crate::iface::IfaceRole::VirtualUnicast)
            .expect("incoming virtual iface");
        let (outbound_tx, _outbound_rx) = tokio::sync::mpsc::channel(1);
        let (incoming_tx, _incoming_rx) = tokio::sync::mpsc::channel(1);
        let peer_routes = Arc::new(tokio::sync::Mutex::new(BTreeMap::from([
            (outbound_iface, outbound_tx),
            (incoming_iface, incoming_tx),
        ])));
        let peers = vec!["configured-peer.b32.i2p".to_string()];
        let runtime_status = Arc::new(std::sync::Mutex::new(I2pRuntimeStatus::new(
            "127.0.0.1:7656".to_string(),
            true,
            &peers,
        )));
        {
            let mut status = runtime_status.lock().expect("i2p runtime status");
            status.mark_outbound_connected(&peers[0], outbound_iface);
            status.mark_incoming_connected("incoming-destination", incoming_iface);
        }

        cleanup_i2p_peer_routes(&iface_manager, &peer_routes, &runtime_status).await;

        assert!(peer_routes.lock().await.is_empty());
        let manager = iface_manager.lock().await;
        assert_eq!(manager.role(&outbound_iface), None);
        assert_eq!(manager.role(&incoming_iface), None);
        drop(manager);
        let status = runtime_status.lock().expect("i2p runtime status");
        assert_eq!(status.peers[&peers[0]].state, I2pTunnelState::Closed);
        let incoming_key = format!("incoming:{incoming_iface}");
        assert_eq!(status.peers[&incoming_key].state, I2pTunnelState::Closed);
    }

    #[tokio::test]
    async fn sam_stream_connect_writes_expected_session_lookup_and_connect_handshakes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fake SAM");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            let mut lines = Vec::new();

            let (session_socket, _) = listener.accept().await.expect("accept session");
            let mut session_reader = BufReader::new(session_socket);
            for response in [
                "HELLO REPLY RESULT=OK VERSION=3.3\n",
                "SESSION STATUS RESULT=OK DESTINATION=fake.b32.i2p\n",
            ] {
                let mut line = String::new();
                session_reader.read_line(&mut line).await.expect("read session command");
                lines.push(line.trim_end().to_string());
                session_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write session response");
            }

            let (lookup_socket, _) = listener.accept().await.expect("accept lookup");
            let mut lookup_reader = BufReader::new(lookup_socket);
            for response in [
                "HELLO REPLY RESULT=OK VERSION=3.3\n",
                "NAMING REPLY RESULT=OK NAME=exampledestination.b32.i2p VALUE=resolved-destination\n",
            ] {
                let mut line = String::new();
                lookup_reader.read_line(&mut line).await.expect("read lookup command");
                lines.push(line.trim_end().to_string());
                lookup_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write lookup response");
            }

            let (connect_socket, _) = listener.accept().await.expect("accept stream");
            let mut connect_reader = BufReader::new(connect_socket);
            for response in ["HELLO REPLY RESULT=OK VERSION=3.3\n", "STREAM STATUS RESULT=OK\n"] {
                let mut line = String::new();
                connect_reader.read_line(&mut line).await.expect("read connect command");
                lines.push(line.trim_end().to_string());
                connect_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write connect response");
            }

            lines
        });

        let (_session, stream) = open_sam_stream(
            addr.to_string().as_str(),
            "lxmf-rs-test",
            "exampledestination.b32.i2p",
        )
        .await
        .expect("open SAM stream");
        drop(stream);

        let lines = server.await.expect("server lines");
        assert_eq!(lines[0], "HELLO VERSION MIN=3.0 MAX=3.3");
        assert_eq!(lines[1], "SESSION CREATE STYLE=STREAM ID=lxmf-rs-test DESTINATION=TRANSIENT");
        assert_eq!(lines[2], "HELLO VERSION MIN=3.0 MAX=3.3");
        assert_eq!(lines[3], "NAMING LOOKUP NAME=exampledestination.b32.i2p");
        assert_eq!(lines[4], "HELLO VERSION MIN=3.0 MAX=3.3");
        assert_eq!(
            lines[5],
            "STREAM CONNECT ID=lxmf-rs-test DESTINATION=resolved-destination SILENT=false"
        );
    }

    #[tokio::test]
    async fn i2p_peer_loop_updates_runtime_status_through_fake_sam_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fake SAM");
        let sam_addr = listener.local_addr().expect("local addr").to_string();
        let server = tokio::spawn(async move {
            let (session_socket, _) = listener.accept().await.expect("accept session");
            let mut session_reader = BufReader::new(session_socket);
            for response in [
                "HELLO REPLY RESULT=OK VERSION=3.3\n",
                "SESSION STATUS RESULT=OK DESTINATION=fake.b32.i2p\n",
            ] {
                let mut line = String::new();
                session_reader.read_line(&mut line).await.expect("read session command");
                session_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write session response");
            }

            let (lookup_socket, _) = listener.accept().await.expect("accept lookup");
            let mut lookup_reader = BufReader::new(lookup_socket);
            for response in [
                "HELLO REPLY RESULT=OK VERSION=3.3\n",
                "NAMING REPLY RESULT=OK NAME=peer.b32.i2p VALUE=resolved-destination\n",
            ] {
                let mut line = String::new();
                lookup_reader.read_line(&mut line).await.expect("read lookup command");
                lookup_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write lookup response");
            }

            let (connect_socket, _) = listener.accept().await.expect("accept stream");
            let mut connect_reader = BufReader::new(connect_socket);
            for response in ["HELLO REPLY RESULT=OK VERSION=3.3\n", "STREAM STATUS RESULT=OK\n"] {
                let mut line = String::new();
                connect_reader.read_line(&mut line).await.expect("read connect command");
                connect_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write connect response");
            }

            let mut hdlc_bytes = [0_u8; 64];
            connect_reader.get_mut().read(&mut hdlc_bytes).await.expect("read tunneled HDLC bytes")
        });

        let peer = "peer.b32.i2p".to_string();
        let iface_address = crate::hash::AddressHash::new([0x33; 16]);
        let runtime_status = Arc::new(std::sync::Mutex::new(I2pRuntimeStatus::new(
            sam_addr.clone(),
            false,
            std::slice::from_ref(&peer),
        )));
        let cancel = CancellationToken::new();
        let iface_stop = CancellationToken::new();
        let (rx_channel, _rx_messages) = tokio::sync::mpsc::channel(8);
        let (peer_tx, peer_rx) = tokio::sync::mpsc::channel(8);
        let peer_loop = tokio::spawn(run_i2p_peer_loop(
            peer.clone(),
            iface_address,
            sam_addr,
            None,
            I2pInterface::DEFAULT_MTU,
            Duration::from_millis(10),
            Arc::clone(&runtime_status),
            cancel.clone(),
            iface_stop.clone(),
            rx_channel,
            peer_rx,
        ));

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let connected = {
                    let status = runtime_status.lock().expect("i2p runtime status");
                    status.peers[&peer].state == I2pTunnelState::Connected
                };
                if connected {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("peer loop connected");

        peer_tx
            .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
            .await
            .expect("send peer packet");
        let hdlc_bytes_read = tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("fake SAM stream read timeout")
            .expect("fake SAM server");
        assert!(hdlc_bytes_read > 0);

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let bytes_tx = {
                    let status = runtime_status.lock().expect("i2p runtime status");
                    status.peers[&peer].bytes_tx
                };
                if bytes_tx > 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("peer loop recorded tx bytes");

        cancel.cancel();
        iface_stop.cancel();
        tokio::time::timeout(Duration::from_secs(1), peer_loop)
            .await
            .expect("peer loop shutdown timeout")
            .expect("peer loop task");
    }

    #[tokio::test]
    async fn i2p_accept_loop_routes_direct_tx_to_incoming_peer_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fake SAM");
        let sam_addr = listener.local_addr().expect("local addr").to_string();
        let server = tokio::spawn(async move {
            let (session_socket, _) = listener.accept().await.expect("accept session");
            let mut session_reader = BufReader::new(session_socket);
            for response in [
                "HELLO REPLY RESULT=OK VERSION=3.3\n",
                "SESSION STATUS RESULT=OK DESTINATION=fake-accept.b32.i2p\n",
            ] {
                let mut line = String::new();
                session_reader.read_line(&mut line).await.expect("read session command");
                session_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write session response");
            }

            let (accept_socket, _) = listener.accept().await.expect("accept stream");
            let mut accept_reader = BufReader::new(accept_socket);
            for response in ["HELLO REPLY RESULT=OK VERSION=3.3\n", "STREAM STATUS RESULT=OK\n"] {
                let mut line = String::new();
                accept_reader.read_line(&mut line).await.expect("read accept command");
                accept_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write accept response");
            }
            let mut accepted_stream = accept_reader.into_inner();
            accepted_stream
                .write_all(b"incoming-destination\n")
                .await
                .expect("write remote destination");
            let mut hdlc_bytes = vec![0_u8; 512];
            let read =
                tokio::time::timeout(Duration::from_secs(1), accepted_stream.read(&mut hdlc_bytes))
                    .await
                    .expect("read outbound HDLC deadline")
                    .expect("read outbound HDLC");
            hdlc_bytes.truncate(read);
            drop(accepted_stream);

            hdlc_bytes
        });

        let mut manager = crate::iface::InterfaceManager::new(8);
        let parent_channel = manager.new_channel_with_role(8, IfaceRole::Multicast);
        let parent_iface = parent_channel.address;
        let iface_stop = parent_channel.stop.clone();
        let manager = Arc::new(tokio::sync::Mutex::new(manager));
        let runtime_status =
            Arc::new(std::sync::Mutex::new(I2pRuntimeStatus::new(sam_addr.clone(), true, &[])));
        let cancel = CancellationToken::new();
        let (rx_channel, _rx_messages) = tokio::sync::mpsc::channel(8);
        let peer_routes = Arc::new(tokio::sync::Mutex::new(BTreeMap::new()));
        let accept_loop = tokio::spawn(run_i2p_accept_loop(
            parent_iface,
            "i2p-main".to_string(),
            sam_addr,
            None,
            None,
            I2pInterface::DEFAULT_MTU,
            Duration::from_millis(10),
            Arc::clone(&runtime_status),
            cancel.clone(),
            iface_stop.clone(),
            rx_channel,
            Arc::clone(&manager),
            Arc::clone(&peer_routes),
        ));

        let (child_iface, sender) = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some((child_iface, sender)) = {
                    let routes = peer_routes.lock().await;
                    routes.iter().next().map(|(iface, sender)| (*iface, sender.clone()))
                } {
                    return (child_iface, sender);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("incoming peer route");

        sender
            .send(TxMessage {
                tx_type: TxMessageType::Direct(child_iface),
                packet: Packet::default(),
            })
            .await
            .expect("send direct tx to incoming peer");
        let hdlc_bytes = tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("fake SAM outbound read timeout")
            .expect("fake SAM server");
        assert!(!hdlc_bytes.is_empty());
        assert_eq!(hdlc_bytes.first().copied(), Some(0x7e));

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let bytes_tx = {
                    let status = runtime_status.lock().expect("i2p runtime status");
                    status
                        .peers
                        .values()
                        .find(|peer| {
                            peer.peer == "incoming-destination"
                                && peer.direction == "incoming"
                                && peer.iface == Some(child_iface)
                        })
                        .map(|peer| peer.bytes_tx)
                        .unwrap_or(0)
                };
                if bytes_tx > 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("incoming peer recorded tx bytes");

        cancel.cancel();
        iface_stop.cancel();
        tokio::time::timeout(Duration::from_secs(2), accept_loop)
            .await
            .expect("accept loop shutdown timeout")
            .expect("accept loop task");
    }

    #[tokio::test]
    async fn i2p_accept_loop_registers_incoming_peer_through_fake_sam_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fake SAM");
        let sam_addr = listener.local_addr().expect("local addr").to_string();
        let packet = Packet::default();
        let hdlc_frame = hdlc_frame_for_packet(&packet);
        let (release_stream_tx, release_stream_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (session_socket, _) = listener.accept().await.expect("accept session");
            let mut session_reader = BufReader::new(session_socket);
            for response in [
                "HELLO REPLY RESULT=OK VERSION=3.3\n",
                "SESSION STATUS RESULT=OK DESTINATION=fake-accept.b32.i2p\n",
            ] {
                let mut line = String::new();
                session_reader.read_line(&mut line).await.expect("read session command");
                session_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write session response");
            }

            let (accept_socket, _) = listener.accept().await.expect("accept stream");
            let mut accept_reader = BufReader::new(accept_socket);
            for response in ["HELLO REPLY RESULT=OK VERSION=3.3\n", "STREAM STATUS RESULT=OK\n"] {
                let mut line = String::new();
                accept_reader.read_line(&mut line).await.expect("read accept command");
                accept_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write accept response");
            }
            let mut accepted_stream = accept_reader.into_inner();
            accepted_stream
                .write_all(b"incoming-destination\n")
                .await
                .expect("write remote destination");
            accepted_stream.write_all(&hdlc_frame).await.expect("write incoming packet");

            let _ = release_stream_rx.await;
            drop(accepted_stream);

            if let Ok(Ok((second_accept_socket, _))) =
                tokio::time::timeout(Duration::from_secs(1), listener.accept()).await
            {
                drop(second_accept_socket);
            }
        });

        let mut manager = crate::iface::InterfaceManager::new(8);
        let parent_channel = manager.new_channel_with_role(8, IfaceRole::Multicast);
        let parent_iface = parent_channel.address;
        let iface_stop = parent_channel.stop.clone();
        let manager = Arc::new(tokio::sync::Mutex::new(manager));
        let runtime_status =
            Arc::new(std::sync::Mutex::new(I2pRuntimeStatus::new(sam_addr.clone(), true, &[])));
        let cancel = CancellationToken::new();
        let (rx_channel, mut rx_messages) = tokio::sync::mpsc::channel(8);
        let peer_routes = Arc::new(tokio::sync::Mutex::new(BTreeMap::new()));
        let accept_loop = tokio::spawn(run_i2p_accept_loop(
            parent_iface,
            "i2p-main".to_string(),
            sam_addr,
            None,
            None,
            I2pInterface::DEFAULT_MTU,
            Duration::from_millis(10),
            Arc::clone(&runtime_status),
            cancel.clone(),
            iface_stop.clone(),
            rx_channel,
            Arc::clone(&manager),
            Arc::clone(&peer_routes),
        ));

        let rx_message = tokio::time::timeout(Duration::from_secs(1), rx_messages.recv())
            .await
            .expect("incoming packet deadline")
            .expect("incoming packet");
        assert_eq!(rx_message.packet, packet);
        assert_eq!(rx_message.source, crate::iface::IfaceSource::None);

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let ready = {
                    let status = runtime_status.lock().expect("i2p runtime status");
                    status.accept_state == I2pTunnelState::Listening
                        && status.peers.values().any(|peer| {
                            peer.peer == "incoming-destination"
                                && peer.direction == "incoming"
                                && peer.state == I2pTunnelState::Connected
                                && peer.bytes_rx > 0
                        })
                };
                if ready {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("incoming runtime status");
        assert_eq!(manager.lock().await.iface_count(), 2);
        assert_eq!(peer_routes.lock().await.len(), 1);

        cancel.cancel();
        iface_stop.cancel();
        let _ = release_stream_tx.send(());
        tokio::time::timeout(Duration::from_secs(2), accept_loop)
            .await
            .expect("accept loop shutdown timeout")
            .expect("accept loop task");
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("fake SAM shutdown timeout")
            .expect("fake SAM server");

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let cleaned = {
                    let status = runtime_status.lock().expect("i2p runtime status");
                    status.accept_state == I2pTunnelState::Closed
                        && status.peers.values().any(|peer| {
                            peer.peer == "incoming-destination"
                                && peer.direction == "incoming"
                                && peer.state == I2pTunnelState::Closed
                        })
                };
                if cleaned {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("incoming runtime cleanup");
        assert_eq!(manager.lock().await.iface_count(), 0);
        assert!(peer_routes.lock().await.is_empty());
    }

    #[tokio::test]
    async fn sam_stream_accept_writes_expected_handshake_and_strips_remote_destination() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fake SAM");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            let mut lines = Vec::new();

            let (session_socket, _) = listener.accept().await.expect("accept session");
            let mut session_reader = BufReader::new(session_socket);
            for response in [
                "HELLO REPLY RESULT=OK VERSION=3.3\n",
                "SESSION STATUS RESULT=OK DESTINATION=fake-private-destination\n",
            ] {
                let mut line = String::new();
                session_reader.read_line(&mut line).await.expect("read session command");
                lines.push(line.trim_end().to_string());
                session_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write session response");
            }

            let (accept_socket, _) = listener.accept().await.expect("accept stream");
            let mut accept_reader = BufReader::new(accept_socket);
            for response in ["HELLO REPLY RESULT=OK VERSION=3.3\n", "STREAM STATUS RESULT=OK\n"] {
                let mut line = String::new();
                accept_reader.read_line(&mut line).await.expect("read accept command");
                lines.push(line.trim_end().to_string());
                accept_reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .expect("write accept response");
            }
            accept_reader
                .get_mut()
                .write_all(b"remote-destination\n~")
                .await
                .expect("write remote dest and first hdlc flag");

            lines
        });

        let (_session, destination) =
            create_sam_session(addr.to_string().as_str(), "lxmf-rs-accept", "TRANSIENT")
                .await
                .expect("create accept session");
        assert_eq!(destination, "fake-private-destination");

        let (remote_destination, mut stream) =
            accept_sam_stream(addr.to_string().as_str(), "lxmf-rs-accept")
                .await
                .expect("accept SAM stream");
        assert_eq!(remote_destination, "remote-destination");
        let mut first_data = [0_u8; 1];
        stream.read_exact(&mut first_data).await.expect("read first tunneled byte");
        assert_eq!(first_data[0], b'~');
        drop(stream);

        let lines = server.await.expect("server lines");
        assert_eq!(lines[0], "HELLO VERSION MIN=3.0 MAX=3.3");
        assert_eq!(lines[1], "SESSION CREATE STYLE=STREAM ID=lxmf-rs-accept DESTINATION=TRANSIENT");
        assert_eq!(lines[2], "HELLO VERSION MIN=3.0 MAX=3.3");
        assert_eq!(lines[3], "STREAM ACCEPT ID=lxmf-rs-accept SILENT=false");
    }

    #[tokio::test]
    async fn connectable_destination_generates_and_persists_private_key() {
        let private_key = fake_i2p_private_key();
        let expected_endpoint =
            i2p_b32_from_private_destination(private_key.as_str()).expect("expected endpoint");
        let identity_hash = [0x42_u8; 16];
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fake SAM");
        let addr = listener.local_addr().expect("local addr");
        let private_key_for_server = private_key.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept destination generation");
            let mut reader = BufReader::new(socket);
            let mut lines = Vec::new();
            let responses = [
                "HELLO REPLY RESULT=OK VERSION=3.3\n".to_string(),
                format!("DEST REPLY PUB=public-destination PRIV={private_key_for_server}\n"),
            ];
            for response in responses {
                let mut line = String::new();
                reader.read_line(&mut line).await.expect("read command");
                lines.push(line.trim_end().to_string());
                reader.get_mut().write_all(response.as_bytes()).await.expect("write response");
            }
            lines
        });

        let root = std::env::temp_dir().join(format!("lxmfrs-i2p-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(root.as_path());
        let destination = connectable_session_destination_with_identity(
            addr.to_string().as_str(),
            "i2p main",
            Some(&root),
            Some(&identity_hash),
        )
        .await
        .expect("destination");
        assert_eq!(destination, private_key);
        assert_eq!(
            i2p_b32_from_private_destination(destination.as_str()).expect("b32"),
            expected_endpoint
        );
        let stored_path = i2p_private_key_new_format_path(&root, "i2p main", &identity_hash);
        let stored = std::fs::read_to_string(stored_path).expect("stored key");
        assert_eq!(stored, private_key);

        let loaded = connectable_session_destination_with_identity(
            "127.0.0.1:1",
            "i2p main",
            Some(&root),
            Some(&identity_hash),
        )
        .await
        .expect("loaded destination");
        assert_eq!(loaded, private_key);
        let _ = std::fs::remove_dir_all(root.as_path());

        let lines = server.await.expect("server lines");
        assert_eq!(lines[0], "HELLO VERSION MIN=3.0 MAX=3.3");
        assert_eq!(lines[1], "DEST GENERATE SIGNATURE_TYPE=7");
    }

    #[test]
    fn i2p_python_key_path_hashes_match_reference_vectors() {
        let identity_hash = [0x42_u8; 16];

        assert_eq!(
            i2p_old_format_key_stem("i2p main"),
            "c433a03c36713497e2ace0f9bb01e85e0eb256f7c3621f096c45b7598572ece6"
        );
        assert_eq!(
            i2p_new_format_key_stem("i2p main", &identity_hash),
            "a5fb47a81e3f0cb627956841e1c2c827ef149a079851d3c88aa99af6b1c750e0"
        );
    }

    #[tokio::test]
    async fn connectable_destination_prefers_existing_python_old_format_key() {
        let root =
            std::env::temp_dir().join(format!("lxmfrs-i2p-old-key-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(root.as_path());
        let old_path = i2p_private_key_old_format_path(&root, "i2p main");
        std::fs::create_dir_all(old_path.parent().expect("old key parent")).expect("create dir");
        std::fs::write(old_path.as_path(), "old-private-key").expect("write old key");
        let identity_hash = [0x42_u8; 16];

        assert_eq!(
            i2p_private_key_path_with_identity(&root, "i2p main", Some(&identity_hash)),
            old_path
        );
        let loaded = connectable_session_destination_with_identity(
            "127.0.0.1:1",
            "i2p main",
            Some(&root),
            Some(&identity_hash),
        )
        .await
        .expect("load old key without SAM");

        assert_eq!(loaded, "old-private-key");
        let _ = std::fs::remove_dir_all(root.as_path());
    }

    fn fake_i2p_private_key() -> String {
        let mut private = vec![0_u8; 500];
        for (index, byte) in private.iter_mut().enumerate() {
            *byte = index as u8;
        }
        private[I2P_CERT_LEN_OFFSET] = 0;
        private[I2P_CERT_LEN_OFFSET + 1] = 3;
        let engine = base64::engine::GeneralPurpose::new(
            &base64::alphabet::Alphabet::new(
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-~",
            )
            .expect("alphabet"),
            base64::engine::general_purpose::PAD,
        );
        engine.encode(private)
    }

    #[test]
    fn i2p_private_destination_base32_matches_public_prefix_hash() {
        let private = {
            let engine = base64::engine::GeneralPurpose::new(
                &base64::alphabet::Alphabet::new(
                    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-~",
                )
                .expect("alphabet"),
                base64::engine::general_purpose::PAD,
            );
            engine.decode(fake_i2p_private_key()).expect("decode private key")
        };
        let public_len = I2P_DEST_PREFIX_LEN + 3;
        let expected = format!(
            "{}.b32.i2p",
            base32_no_pad_lower(sha2::Sha256::digest(&private[..public_len]).as_slice())
        );
        let encoded = fake_i2p_private_key();
        assert_eq!(i2p_b32_from_private_destination(encoded.as_str()).expect("b32"), expected);
    }
}
