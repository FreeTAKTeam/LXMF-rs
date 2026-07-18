#[allow(clippy::large_enum_variant)]
pub enum LinkHandleResult {
    None,
    Activated,
    Proof(Packet),
    KeepAlive,
}

#[derive(Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum LinkWatchdogAction {
    None,
    SendKeepAlive,
    SendTeardown(Packet),
}

#[derive(Clone)]
pub enum LinkEvent {
    Activated,
    Data(Box<LinkPayload>),
    PeerIdentified(Box<Identity>),
    Closed,
}

#[derive(Clone)]
pub struct LinkEventData {
    pub id: LinkId,
    pub address_hash: AddressHash,
    pub event: LinkEvent,
}

pub struct Link {
    id: LinkId,
    destination: DestinationDesc,
    ingress_iface: Option<AddressHash>,
    priv_identity: PrivateIdentity,
    peer_identity: Identity,
    identified_peer_identity: Option<Identity>,
    derived_key: DerivedKey,
    session_cipher: Option<CachedFernet>,
    signalling: Option<[u8; LINK_MTU_SIZE]>,
    status: LinkStatus,
    request_time: Instant,
    rtt: Duration,
    activated_at: Option<Instant>,
    last_inbound: Option<Instant>,
    last_outbound: Option<Instant>,
    last_data: Option<Instant>,
    last_keepalive: Option<Instant>,
    last_proof: Option<Instant>,
    stale_since: Option<Instant>,
    keepalive: Duration,
    stale_time: Duration,
    next_channel_sequence: u16,
    next_channel_rx_sequence: u16,
    channel_open: bool,
    next_channel_handler_id: u64,
    channel_handlers: HashMap<u16, Vec<RegisteredChannelHandler>>,
    channel_pending: HashMap<Hash, PendingChannelPacket>,
    channel_states: HashMap<u16, ChannelMessageState>,
    channel_rx_ring: HashMap<u16, ChannelEnvelope>,
    channel_window: u8,
    channel_window_max: u8,
    channel_window_min: u8,
    channel_window_flexibility: u8,
    channel_fast_rate_rounds: u8,
    channel_medium_rate_rounds: u8,
    event_tx: tokio::sync::broadcast::Sender<LinkEventData>,
}
