use super::*;

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
