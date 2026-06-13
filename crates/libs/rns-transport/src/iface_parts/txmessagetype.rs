#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum TxMessageType {
    Broadcast(Option<AddressHash>),
    Direct(AddressHash),
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct TxMessage {
    pub tx_type: TxMessageType,
    pub packet: Packet,
}

#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub struct TxDispatchTrace {
    pub matched_ifaces: usize,
    pub sent_ifaces: usize,
    pub queued_ifaces: usize,
    pub failed_ifaces: usize,
}

#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub enum InterfaceMode {
    #[default]
    Full,
    PointToPoint,
    AccessPoint,
    Roaming,
    Boundary,
    Gateway,
}

impl InterfaceMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "pointtopoint" | "point_to_point" | "point-to-point" | "ptp" => {
                Some(Self::PointToPoint)
            }
            "access_point" | "accesspoint" | "access-point" | "ap" => Some(Self::AccessPoint),
            "roaming" => Some(Self::Roaming),
            "boundary" => Some(Self::Boundary),
            "gateway" | "gw" => Some(Self::Gateway),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::PointToPoint => "pointtopoint",
            Self::AccessPoint => "access_point",
            Self::Roaming => "roaming",
            Self::Boundary => "boundary",
            Self::Gateway => "gateway",
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub struct AnnounceBroadcastPolicy {
    pub local_destination: bool,
    pub next_hop_iface_mode: Option<InterfaceMode>,
}

/// Where a received packet came from at the wire level.
///
/// Packets arriving over a stream/serial medium have `None`; UDP packets
/// carry the sender's socket address so the transport can route replies
/// back unicast instead of re-broadcasting them onto a multicast group.
#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub enum IfaceSource {
    #[default]
    None,
    Udp(SocketAddr),
}

/// Tags an interface's transmit semantics.
///
/// - `Unicast` (default): TCP / serial / point-to-point UDP. Carries
///   both `Broadcast` and `Direct` tx.
/// - `Multicast`: shared-group UDP. Carries `Broadcast` tx; `Direct` tx
///   addressed at the iface itself is dropped by the tx-guard in
///   `iface::udp` (nonsensical — multicast sockets broadcast every tx).
///   Per-peer unicast traffic on this medium goes via `VirtualUnicast`
///   siblings that share the host multicast socket.
/// - `VirtualUnicast`: a *virtual* iface pinned to one peer over a host
///   multicast iface. Registered via
///   `InterfaceManager::register_virtual_iface`; shares its tx channel
///   with the host iface so the host iface's tx task routes by
///   destination. Skipped on `Broadcast` tx — the host iface already
///   delivers broadcasts for the whole group.
#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub enum IfaceRole {
    #[default]
    Unicast,
    Multicast,
    VirtualUnicast,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct RxMessage {
    pub address: AddressHash,
    pub packet: Packet,
    pub source: IfaceSource,
}

pub struct InterfaceChannel {
    pub address: AddressHash,
    pub rx_channel: InterfaceRxSender,
    pub tx_channel: InterfaceTxReceiver,
    pub stop: CancellationToken,
}

impl InterfaceChannel {
    pub fn make_rx_channel(cap: usize) -> (InterfaceRxSender, InterfaceRxReceiver) {
        mpsc::channel(cap)
    }

    pub fn make_tx_channel(cap: usize) -> (InterfaceTxSender, InterfaceTxReceiver) {
        mpsc::channel(cap)
    }

    pub fn new(
        rx_channel: InterfaceRxSender,
        tx_channel: InterfaceTxReceiver,
        address: AddressHash,
        stop: CancellationToken,
    ) -> Self {
        Self { address, rx_channel, tx_channel, stop }
    }

    pub fn address(&self) -> &AddressHash {
        &self.address
    }

    pub fn split(self) -> (InterfaceRxSender, InterfaceTxReceiver) {
        (self.rx_channel, self.tx_channel)
    }
}

pub trait Interface {
    fn mtu() -> usize;

    fn configured_mtu(&self) -> usize {
        Self::mtu()
    }
}

struct LocalInterface {
    address: AddressHash,
    full_hash: Hash,
    tx_send: InterfaceTxSender,
    stop: CancellationToken,
    mtu: usize,
    role: IfaceRole,
    mode: InterfaceMode,
    outgoing: bool,
    announce_queue: VecDeque<QueuedAnnounce>,
    announce_allowed_at: Instant,
    announce_bitrate_bps: u64,
    announce_cap_percent: u64,
}

#[derive(Debug, Copy, Clone)]
struct QueuedAnnounce {
    message: TxMessage,
    queued_at: Instant,
    emitted: u64,
}

pub struct InterfaceContext<T: Interface> {
    pub inner: Arc<Mutex<T>>,
    pub channel: InterfaceChannel,
    pub cancel: CancellationToken,
}

pub struct InterfaceManager {
    counter: usize,
    rx_recv: Arc<tokio::sync::Mutex<InterfaceRxReceiver>>,
    rx_send: InterfaceRxSender,
    cancel: CancellationToken,
    ifaces: Vec<LocalInterface>,
}
