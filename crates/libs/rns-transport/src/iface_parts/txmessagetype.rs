#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum TxMessageType {
    Broadcast(Option<AddressHash>),
    Direct(AddressHash),
}

#[derive(Debug, PartialEq, Eq, Clone)]
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
    Internal,
}

/// Routing attributes that affect path selection for an interface.
///
/// Reticulum 1.4.2 uses gravity only as a tie-breaker when the same announce
/// is heard over multiple interfaces. Keeping it beside the mode makes the
/// policy explicit and prevents callers from accidentally using announce
/// pacing or hardware bitrate as a routing preference.
#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub struct InterfacePolicy {
    pub mode: InterfaceMode,
    pub gravity: i64,
}

impl InterfaceMode {
    pub fn parse(value: &str) -> Result<Option<Self>, &'static str> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" => Ok(None),
            "full" => Ok(Some(Self::Full)),
            "pointtopoint" | "point_to_point" | "point-to-point" | "ptp" => {
                Ok(Some(Self::PointToPoint))
            }
            "access_point" | "accesspoint" | "access-point" | "ap" => Ok(Some(Self::AccessPoint)),
            "roaming" => Ok(Some(Self::Roaming)),
            "boundary" => Ok(Some(Self::Boundary)),
            "gateway" | "gw" => Ok(Some(Self::Gateway)),
            "internal" => Ok(Some(Self::Internal)),
            _ => Err("unknown interface mode"),
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
            Self::Internal => "internal",
        }
    }

    pub fn discovers_unknown_paths(self) -> bool {
        matches!(self, Self::AccessPoint | Self::Gateway | Self::Roaming | Self::Internal)
    }
}

#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub struct AnnounceBroadcastPolicy {
    pub local_destination: bool,
    pub next_hop_iface_mode: Option<InterfaceMode>,
    /// The next-hop interface's `announces_to_internal`, mirroring the
    /// reference's `from_interface.announces_to_internal`. `None` is the
    /// reference default (`Interface.py`: `self.announces_to_internal =
    /// None`); `Some(true)` lets an announce that arrived over a boundary
    /// interface still cross onto an internal one.
    pub next_hop_announces_to_internal: Option<bool>,
}

impl AnnounceBroadcastPolicy {
    /// Reads the next hop's routing attributes off `manager`, mirroring the
    /// reference's `from_interface` lookup in `Transport.py`'s announce
    /// ladder. `next_hop` is `None` when the destination has no path on file,
    /// which is itself one of the ladder's rungs.
    pub fn for_next_hop(
        manager: &InterfaceManager,
        next_hop: Option<AddressHash>,
        local_destination: bool,
    ) -> Self {
        Self {
            local_destination,
            next_hop_iface_mode: next_hop.and_then(|iface| manager.mode(&iface)),
            next_hop_announces_to_internal: next_hop
                .and_then(|iface| manager.shared_config(&iface))
                .and_then(|config| config.announces_to_internal),
        }
    }
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct InterfaceSharedConfig {
    pub announce_rate_target: Option<u64>,
    pub announce_rate_grace: Option<u64>,
    pub announce_rate_penalty: Option<u64>,
    pub bootstrap_only: Option<bool>,
    /// Whether this interface will carry a non-local announce whose next hop
    /// is an internal-mode interface. The reference's per-interface
    /// `announces_from_internal` (`Interface.py`, default `True`), read in
    /// `Transport.py`'s announce ladder. `None` means the default, `true`.
    pub announces_from_internal: Option<bool>,
    /// Whether an announce that arrived over *this* interface may cross onto
    /// an internal-mode interface even when this one is a boundary. The
    /// reference's per-interface `announces_to_internal` (`Interface.py`,
    /// default `None`), read via `from_interface` in the same ladder.
    pub announces_to_internal: Option<bool>,
    pub ifac_size: Option<u64>,
    pub network_name: Option<String>,
    pub passphrase: Option<String>,
    pub ingress_control: Option<bool>,
    pub egress_control: Option<bool>,
    pub ic_max_held_announces: Option<u64>,
    pub ic_burst_hold: Option<f64>,
    pub ic_burst_freq_new: Option<f64>,
    pub ic_burst_freq: Option<f64>,
    pub ic_pr_burst_freq_new: Option<f64>,
    pub ic_pr_burst_freq: Option<f64>,
    pub ec_pr_freq: Option<f64>,
    pub ic_new_time: Option<f64>,
    pub ic_burst_penalty: Option<f64>,
    pub ic_held_release_interval: Option<f64>,
    pub discoverable: Option<bool>,
    pub announce_interval: Option<u64>,
    pub discovery_stamp_value: Option<u64>,
    pub discovery_name: Option<String>,
    pub discovery_lxmf_address: Option<String>,
    pub discovery_encrypt: Option<bool>,
    pub reachable_on: Option<String>,
    pub publish_ifac: Option<bool>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub height: Option<f64>,
    pub discovery_frequency: Option<u64>,
    pub discovery_bandwidth: Option<u64>,
    pub discovery_modulation: Option<u64>,
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RxMessage {
    pub address: AddressHash,
    pub packet: Packet,
    pub source: IfaceSource,
}

/// Live IFAC configuration shared by a carrier and its manager entry.
pub type IfacState = Arc<std::sync::RwLock<Option<crate::transport::IfacContext>>>;

pub struct InterfaceChannel {
    pub address: AddressHash,
    pub rx_channel: InterfaceRxSender,
    pub tx_channel: InterfaceTxReceiver,
    pub stop: CancellationToken,
    /// Shared wire-authentication state for this physical interface.
    ///
    /// The state is updated by `InterfaceManager::set_shared_config` so a
    /// carrier that is already running observes a configuration change
    /// without having to restart its read/write tasks. Virtual interfaces
    /// inherit the host state and therefore cannot accidentally bypass IFAC.
    pub ifac_state: IfacState,
    pub ifac_violations: Arc<AtomicU64>,
    pub ifac_default_size_bytes: usize,
    online: Arc<AtomicBool>,
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
        Self::with_wire_state(
            rx_channel,
            tx_channel,
            address,
            stop,
            IfacRuntime::disabled(crate::iface::DEFAULT_IFAC_SIZE_BYTES),
            Arc::new(AtomicBool::new(true)),
        )
    }

    fn with_wire_state(
        rx_channel: InterfaceRxSender,
        tx_channel: InterfaceTxReceiver,
        address: AddressHash,
        stop: CancellationToken,
        ifac: IfacRuntime,
        online: Arc<AtomicBool>,
    ) -> Self {
        Self {
            address,
            rx_channel,
            tx_channel,
            stop,
            ifac_state: ifac.state,
            ifac_violations: ifac.violations,
            ifac_default_size_bytes: ifac.default_size_bytes,
            online,
        }
    }

    pub fn address(&self) -> &AddressHash {
        &self.address
    }

    /// Updates whether this channel currently has an operational carrier.
    pub fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::Release);
    }

    pub fn split(self) -> (InterfaceRxSender, InterfaceTxReceiver) {
        (self.rx_channel, self.tx_channel)
    }
}

pub trait Interface {
    fn mtu() -> usize;

    fn ifac_default_size_bytes() -> usize {
        crate::iface::DEFAULT_IFAC_SIZE_BYTES
    }

    fn configured_mtu(&self) -> usize {
        Self::mtu()
    }
}

struct LocalInterface {
    address: AddressHash,
    parent: Option<AddressHash>,
    full_hash: Hash,
    display_name: Option<String>,
    tcp_client_path_table_metadata: Option<TcpClientPathTableMetadata>,
    tx_send: InterfaceTxSender,
    stop: CancellationToken,
    online: Arc<AtomicBool>,
    mtu: usize,
    role: IfaceRole,
    mode: InterfaceMode,
    gravity: i64,
    outgoing: bool,
    announce_queue: VecDeque<QueuedAnnounce>,
    announce_allowed_at: Instant,
    announce_bitrate_bps: u64,
    announce_cap_percent: u64,
    shared_config: InterfaceSharedConfig,
    ifac_state: IfacState,
    ifac_violations: Arc<AtomicU64>,
    ifac_default_size_bytes: usize,
    inherit_ifac: bool,
    is_shared_instance: bool,
    outgoing_pr_history: VecDeque<Instant>,
    traffic: InterfaceTraffic,
}

#[derive(Debug, Clone)]
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
    ifaces: Vec<LocalInterface>,
}
