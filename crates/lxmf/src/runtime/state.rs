use super::*;

#[derive(Debug)]
pub(super) struct PreparedSendMessage {
    pub(super) id: String,
    pub(super) source: String,
    pub(super) destination: String,
    pub(super) params: Value,
}

pub(super) struct RuntimeRequest {
    pub(super) command: RuntimeCommand,
    pub(super) respond_to: std_mpsc::Sender<Result<RuntimeResponse, String>>,
}

pub(super) enum RuntimeCommand {
    Status,
    Call(RpcRequest),
    PollEvent,
    Stop,
}

pub(super) enum RuntimeResponse {
    Status(DaemonStatus),
    Value(Value),
    Event(Option<RpcEvent>),
    Ack,
}

pub(super) struct WorkerInit {
    pub(super) profile: String,
    pub(super) settings: ProfileSettings,
    pub(super) paths: ProfilePaths,
    pub(super) transport: Option<String>,
    pub(super) transport_inferred: bool,
    pub(super) interfaces: Vec<InterfaceEntry>,
}

pub(super) struct WorkerState {
    pub(super) profile: String,
    pub(super) status_template: DaemonStatus,
    pub(super) daemon: Rc<RpcDaemon>,
    pub(super) transport: Option<Arc<Transport>>,
    pub(super) local_identity: PrivateIdentity,
    pub(super) peer_announce_meta: Arc<Mutex<HashMap<String, PeerAnnounceMeta>>>,
    pub(super) peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    pub(super) peer_identity_cache_path: PathBuf,
    pub(super) selected_propagation_node: Arc<Mutex<Option<String>>>,
    pub(super) propagation_sync_state: Arc<Mutex<RuntimePropagationSyncState>>,
    pub(super) shutdown_tx: watch::Sender<bool>,
    pub(super) scheduler_handle: Option<tokio::task::JoinHandle<()>>,
    pub(super) shutdown: bool,
}

#[derive(Debug, Clone)]
pub(super) struct RuntimePropagationSyncState {
    pub(super) sync_state: u32,
    pub(super) state_name: String,
    pub(super) sync_progress: f64,
    pub(super) messages_received: u32,
    pub(super) max_messages: u32,
    pub(super) selected_node: Option<String>,
    pub(super) last_sync_started: Option<i64>,
    pub(super) last_sync_completed: Option<i64>,
    pub(super) last_sync_error: Option<String>,
}

impl Default for RuntimePropagationSyncState {
    fn default() -> Self {
        Self {
            sync_state: PR_IDLE,
            state_name: "idle".to_string(),
            sync_progress: 0.0,
            messages_received: 0,
            max_messages: 0,
            selected_node: None,
            last_sync_started: None,
            last_sync_completed: None,
            last_sync_error: None,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct OutboundDeliveryOptionsCompat {
    pub(super) method: Option<String>,
    pub(super) stamp_cost: Option<u32>,
    pub(super) include_ticket: bool,
    pub(super) try_propagation_on_fail: bool,
    pub(super) source_private_key: Option<String>,
    pub(super) ticket: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) struct PeerCrypto {
    pub(super) identity: Identity,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PeerAnnounceMeta {
    pub(super) app_data_hex: Option<String>,
}

#[derive(Clone)]
pub(super) struct AnnounceTarget {
    pub(super) destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    pub(super) app_data: Option<Vec<u8>>,
}

pub(super) struct EmbeddedTransportBridge {
    pub(super) transport: Arc<Transport>,
    pub(super) signer: PrivateIdentity,
    pub(super) delivery_source_hash: [u8; 16],
    pub(super) announce_targets: Vec<AnnounceTarget>,
    pub(super) last_announce_epoch_secs: Arc<AtomicU64>,
    pub(super) peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    pub(super) peer_identity_cache_path: PathBuf,
    pub(super) selected_propagation_node: Arc<Mutex<Option<String>>>,
    pub(super) known_propagation_nodes: Arc<Mutex<HashSet<String>>>,
    pub(super) receipt_map: Arc<Mutex<HashMap<String, String>>>,
    pub(super) outbound_resource_map: Arc<Mutex<HashMap<String, String>>>,
    pub(super) delivered_messages: Arc<Mutex<HashSet<String>>>,
    pub(super) receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RuntimePropagationSyncParams {
    #[serde(default)]
    pub(super) identity_private_key: Option<String>,
    #[serde(default)]
    pub(super) max_messages: Option<u32>,
}

impl WorkerState {
    pub(super) fn shutdown(&mut self) {
        if self.shutdown {
            return;
        }
        self.shutdown = true;
        if let Some(handle) = self.scheduler_handle.take() {
            handle.abort();
        }
        let _ = self.shutdown_tx.send(true);
        self.daemon.push_event(RpcEvent {
            event_type: "runtime_stopped".to_string(),
            payload: json!({ "profile": self.profile }),
        });
    }
}

impl EmbeddedTransportBridge {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        transport: Arc<Transport>,
        signer: PrivateIdentity,
        delivery_source_hash: [u8; 16],
        announce_targets: Vec<AnnounceTarget>,
        last_announce_epoch_secs: Arc<AtomicU64>,
        peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
        peer_identity_cache_path: PathBuf,
        selected_propagation_node: Arc<Mutex<Option<String>>>,
        known_propagation_nodes: Arc<Mutex<HashSet<String>>>,
        receipt_map: Arc<Mutex<HashMap<String, String>>>,
        outbound_resource_map: Arc<Mutex<HashMap<String, String>>>,
        delivered_messages: Arc<Mutex<HashSet<String>>>,
        receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    ) -> Self {
        Self {
            transport,
            signer,
            delivery_source_hash,
            announce_targets,
            last_announce_epoch_secs,
            peer_crypto,
            peer_identity_cache_path,
            selected_propagation_node,
            known_propagation_nodes,
            receipt_map,
            outbound_resource_map,
            delivered_messages,
            receipt_tx,
        }
    }
}

impl OutboundBridge for EmbeddedTransportBridge {
    #[cfg(reticulum_api_v2)]
    fn deliver(
        &self,
        record: &MessageRecord,
        options: &reticulum::rpc::OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        self.deliver_with_options(record, merge_outbound_delivery_options(options, record))
    }

    #[cfg(not(reticulum_api_v2))]
    fn deliver(&self, record: &MessageRecord) -> Result<(), std::io::Error> {
        self.deliver_with_options(record, merge_outbound_delivery_options(record))
    }
}

impl AnnounceBridge for EmbeddedTransportBridge {
    fn announce_now(&self) -> Result<(), std::io::Error> {
        self.last_announce_epoch_secs.store(now_epoch_secs(), Ordering::Relaxed);
        let transport = self.transport.clone();
        let announce_targets = self.announce_targets.clone();
        tokio::spawn(async move {
            for target in announce_targets {
                transport.send_announce(&target.destination, target.app_data.as_deref()).await;
            }
        });
        Ok(())
    }
}
