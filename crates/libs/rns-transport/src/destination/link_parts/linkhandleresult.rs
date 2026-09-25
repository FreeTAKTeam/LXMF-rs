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

/// Why an RNS link entered its closed state. Numeric values match
/// `RNS.Link.TIMEOUT`, `INITIATOR_CLOSED`, and `DESTINATION_CLOSED`.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[repr(u8)]
pub enum LinkCloseReason {
    Timeout = 0x01,
    InitiatorClosed = 0x02,
    DestinationClosed = 0x03,
}

impl LinkCloseReason {
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Clone)]
pub struct LinkEventData {
    pub id: LinkId,
    pub address_hash: AddressHash,
    pub event: LinkEvent,
    /// Present only for `LinkEvent::Closed` and matches the pinned Python
    /// `Link.teardown_reason` value.
    pub close_reason: Option<LinkCloseReason>,
}

pub struct Link {
    id: LinkId,
    is_initiator: bool,
    close_reason: Option<LinkCloseReason>,
    destination: DestinationDesc,
    ingress_iface: Option<AddressHash>,
    priv_identity: PrivateIdentity,
    peer_identity: Identity,
    identified_peer_identity: Option<Identity>,
    derived_key: DerivedKey,
    session_cipher: Option<CachedFernet>,
    signalling: Option<[u8; LINK_MTU_SIZE]>,
    status: LinkStatus,
    establishment_started_at: Instant,
    establishment_timeout: Duration,
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
    pending_response_limits: HashMap<Vec<u8>, usize>,
    channel_window: u8,
    channel_window_max: u8,
    channel_window_min: u8,
    channel_window_flexibility: u8,
    channel_fast_rate_rounds: u8,
    channel_medium_rate_rounds: u8,
    event_tx: tokio::sync::broadcast::Sender<LinkEventData>,
}
