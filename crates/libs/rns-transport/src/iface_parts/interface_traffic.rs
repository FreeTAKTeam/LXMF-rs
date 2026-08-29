const TRAFFIC_FREQUENCY_SAMPLES: usize = 48;
const TRAFFIC_FREQUENCY_DECAY: Duration = Duration::from_secs(10);
const DEFAULT_PR_BURST_FREQ_NEW: f64 = 3.0;
const DEFAULT_PR_BURST_FREQ: f64 = 8.0;
const DEFAULT_INGRESS_NEW_TIME: Duration = Duration::from_secs(2 * 60 * 60);
const DEFAULT_INGRESS_BURST_HOLD: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceTrafficSnapshot {
    pub address: AddressHash,
    pub parent: Option<AddressHash>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_speed: f64,
    pub tx_speed: f64,
    pub announce_rx_bytes: u64,
    pub announce_tx_bytes: u64,
    pub announce_rx_count: u64,
    pub announce_tx_count: u64,
    pub announce_rx_speed: f64,
    pub announce_tx_speed: f64,
    pub announce_rx_frequency: f64,
    pub announce_tx_frequency: f64,
    pub path_request_rx_bytes: u64,
    pub path_request_tx_bytes: u64,
    pub path_request_rx_count: u64,
    pub path_request_tx_count: u64,
    pub path_request_rx_speed: f64,
    pub path_request_tx_speed: f64,
    pub path_request_rx_frequency: f64,
    pub path_request_tx_frequency: f64,
    pub protocol_violations: u64,
    pub ifac_violations: u64,
    pub packet_filter_hits: u64,
    pub announce_burst_active: bool,
    pub path_request_burst_active: bool,
    pub ic_burst_count: Option<u64>,
    pub ic_pr_burst_count: Option<u64>,
}

impl InterfaceTrafficSnapshot {
    pub(crate) fn aggregate_child(&mut self, child: &Self) {
        self.rx_bytes = self.rx_bytes.saturating_add(child.rx_bytes);
        self.tx_bytes = self.tx_bytes.saturating_add(child.tx_bytes);
        self.rx_speed += child.rx_speed;
        self.tx_speed += child.tx_speed;
        self.announce_rx_bytes = self.announce_rx_bytes.saturating_add(child.announce_rx_bytes);
        self.announce_tx_bytes = self.announce_tx_bytes.saturating_add(child.announce_tx_bytes);
        self.announce_rx_count = self.announce_rx_count.saturating_add(child.announce_rx_count);
        self.announce_tx_count = self.announce_tx_count.saturating_add(child.announce_tx_count);
        self.announce_rx_speed += child.announce_rx_speed;
        self.announce_tx_speed += child.announce_tx_speed;
        self.announce_rx_frequency += child.announce_rx_frequency;
        self.announce_tx_frequency += child.announce_tx_frequency;
        self.path_request_rx_bytes =
            self.path_request_rx_bytes.saturating_add(child.path_request_rx_bytes);
        self.path_request_tx_bytes =
            self.path_request_tx_bytes.saturating_add(child.path_request_tx_bytes);
        self.path_request_rx_count =
            self.path_request_rx_count.saturating_add(child.path_request_rx_count);
        self.path_request_tx_count =
            self.path_request_tx_count.saturating_add(child.path_request_tx_count);
        self.path_request_rx_speed += child.path_request_rx_speed;
        self.path_request_tx_speed += child.path_request_tx_speed;
        self.path_request_rx_frequency += child.path_request_rx_frequency;
        self.path_request_tx_frequency += child.path_request_tx_frequency;
        self.protocol_violations = self.protocol_violations.saturating_add(child.protocol_violations);
        self.ifac_violations = self.ifac_violations.saturating_add(child.ifac_violations);
        self.packet_filter_hits = self.packet_filter_hits.saturating_add(child.packet_filter_hits);
        self.announce_burst_active |= child.announce_burst_active;
        self.path_request_burst_active |= child.path_request_burst_active;
    }
}

#[derive(Debug, Clone)]
struct InterfaceTraffic {
    created_at: Instant,
    rx_bytes: u64,
    tx_bytes: u64,
    announce_rx_bytes: u64,
    announce_tx_bytes: u64,
    announce_rx_count: u64,
    announce_tx_count: u64,
    path_request_rx_bytes: u64,
    path_request_tx_bytes: u64,
    path_request_rx_count: u64,
    path_request_tx_count: u64,
    announce_rx_times: VecDeque<Instant>,
    announce_tx_times: VecDeque<Instant>,
    path_request_rx_times: VecDeque<Instant>,
    path_request_tx_times: VecDeque<Instant>,
    protocol_violations: u64,
    ifac_violations: u64,
    packet_filter_hits: u64,
    path_request_burst_active: bool,
    path_request_burst_activated: Option<Instant>,
    path_request_burst_sustained: Option<Instant>,
    path_request_burst_cooldown: u8,
    sample_at: Instant,
    sample_rx_bytes: u64,
    sample_tx_bytes: u64,
    sample_announce_rx_bytes: u64,
    sample_announce_tx_bytes: u64,
    sample_path_request_rx_bytes: u64,
    sample_path_request_tx_bytes: u64,
    rx_speed: f64,
    tx_speed: f64,
    announce_rx_speed: f64,
    announce_tx_speed: f64,
    path_request_rx_speed: f64,
    path_request_tx_speed: f64,
}

impl Default for InterfaceTraffic {
    fn default() -> Self {
        Self {
            created_at: Instant::now(),
            rx_bytes: 0,
            tx_bytes: 0,
            announce_rx_bytes: 0,
            announce_tx_bytes: 0,
            announce_rx_count: 0,
            announce_tx_count: 0,
            path_request_rx_bytes: 0,
            path_request_tx_bytes: 0,
            path_request_rx_count: 0,
            path_request_tx_count: 0,
            announce_rx_times: VecDeque::new(),
            announce_tx_times: VecDeque::new(),
            path_request_rx_times: VecDeque::new(),
            path_request_tx_times: VecDeque::new(),
            protocol_violations: 0,
            ifac_violations: 0,
            packet_filter_hits: 0,
            path_request_burst_active: false,
            path_request_burst_activated: None,
            path_request_burst_sustained: None,
            path_request_burst_cooldown: 0,
            sample_at: Instant::now(),
            sample_rx_bytes: 0,
            sample_tx_bytes: 0,
            sample_announce_rx_bytes: 0,
            sample_announce_tx_bytes: 0,
            sample_path_request_rx_bytes: 0,
            sample_path_request_tx_bytes: 0,
            rx_speed: 0.0,
            tx_speed: 0.0,
            announce_rx_speed: 0.0,
            announce_tx_speed: 0.0,
            path_request_rx_speed: 0.0,
            path_request_tx_speed: 0.0,
        }
    }
}

impl InterfaceTraffic {
    fn push_time(times: &mut VecDeque<Instant>, now: Instant) {
        times.push_back(now);
        while times.len() > TRAFFIC_FREQUENCY_SAMPLES {
            times.pop_front();
        }
    }

    fn frequency(times: &mut VecDeque<Instant>, now: Instant) -> f64 {
        while times.front().is_some_and(|time| now.duration_since(*time) > TRAFFIC_FREQUENCY_DECAY)
        {
            times.pop_front();
        }
        let Some(oldest) = times.front().copied() else {
            return 0.0;
        };
        let span = now.duration_since(oldest).as_secs_f64();
        if times.len() <= 1 || span <= f64::EPSILON {
            0.0
        } else {
            times.len() as f64 / span
        }
    }

    fn update_speeds(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.sample_at).as_secs_f64();
        if elapsed <= f64::EPSILON {
            return;
        }
        self.rx_speed = self.rx_bytes.saturating_sub(self.sample_rx_bytes) as f64 * 8.0 / elapsed;
        self.tx_speed = self.tx_bytes.saturating_sub(self.sample_tx_bytes) as f64 * 8.0 / elapsed;
        self.announce_rx_speed = self
            .announce_rx_bytes
            .saturating_sub(self.sample_announce_rx_bytes) as f64
            * 8.0
            / elapsed;
        self.announce_tx_speed = self
            .announce_tx_bytes
            .saturating_sub(self.sample_announce_tx_bytes) as f64
            * 8.0
            / elapsed;
        self.path_request_rx_speed = self
            .path_request_rx_bytes
            .saturating_sub(self.sample_path_request_rx_bytes) as f64
            * 8.0
            / elapsed;
        self.path_request_tx_speed = self
            .path_request_tx_bytes
            .saturating_sub(self.sample_path_request_tx_bytes) as f64
            * 8.0
            / elapsed;
        self.sample_at = now;
        self.sample_rx_bytes = self.rx_bytes;
        self.sample_tx_bytes = self.tx_bytes;
        self.sample_announce_rx_bytes = self.announce_rx_bytes;
        self.sample_announce_tx_bytes = self.announce_tx_bytes;
        self.sample_path_request_rx_bytes = self.path_request_rx_bytes;
        self.sample_path_request_tx_bytes = self.path_request_tx_bytes;
    }

    fn snapshot(
        &mut self,
        address: AddressHash,
        parent: Option<AddressHash>,
        now: Instant,
    ) -> InterfaceTrafficSnapshot {
        InterfaceTrafficSnapshot {
            address,
            parent,
            rx_bytes: self.rx_bytes,
            tx_bytes: self.tx_bytes,
            rx_speed: self.rx_speed,
            tx_speed: self.tx_speed,
            announce_rx_bytes: self.announce_rx_bytes,
            announce_tx_bytes: self.announce_tx_bytes,
            announce_rx_count: self.announce_rx_count,
            announce_tx_count: self.announce_tx_count,
            announce_rx_speed: self.announce_rx_speed,
            announce_tx_speed: self.announce_tx_speed,
            announce_rx_frequency: Self::frequency(&mut self.announce_rx_times, now),
            announce_tx_frequency: Self::frequency(&mut self.announce_tx_times, now),
            path_request_rx_bytes: self.path_request_rx_bytes,
            path_request_tx_bytes: self.path_request_tx_bytes,
            path_request_rx_count: self.path_request_rx_count,
            path_request_tx_count: self.path_request_tx_count,
            path_request_rx_speed: self.path_request_rx_speed,
            path_request_tx_speed: self.path_request_tx_speed,
            path_request_rx_frequency: Self::frequency(&mut self.path_request_rx_times, now),
            path_request_tx_frequency: Self::frequency(&mut self.path_request_tx_times, now),
            protocol_violations: self.protocol_violations,
            ifac_violations: self.ifac_violations,
            packet_filter_hits: self.packet_filter_hits,
            announce_burst_active: false,
            path_request_burst_active: self.path_request_burst_active,
            ic_burst_count: None,
            ic_pr_burst_count: None,
        }
    }
}

impl InterfaceManager {
    pub fn record_inbound_traffic(
        &mut self,
        address: AddressHash,
        packet_type: PacketType,
        is_path_request: bool,
        wire_len: usize,
    ) -> bool {
        let Some(iface) = self.ifaces.iter_mut().find(|iface| iface.address == address) else {
            return false;
        };
        let bytes = wire_len as u64;
        let now = Instant::now();
        iface.traffic.rx_bytes = iface.traffic.rx_bytes.saturating_add(bytes);
        if packet_type == PacketType::Announce {
            iface.traffic.announce_rx_bytes = iface.traffic.announce_rx_bytes.saturating_add(bytes);
            iface.traffic.announce_rx_count = iface.traffic.announce_rx_count.saturating_add(1);
            InterfaceTraffic::push_time(&mut iface.traffic.announce_rx_times, now);
        } else if is_path_request {
            iface.traffic.path_request_rx_bytes =
                iface.traffic.path_request_rx_bytes.saturating_add(bytes);
            iface.traffic.path_request_rx_count =
                iface.traffic.path_request_rx_count.saturating_add(1);
            InterfaceTraffic::push_time(&mut iface.traffic.path_request_rx_times, now);
        }
        true
    }

    fn record_outbound_traffic(
        iface: &mut LocalInterface,
        packet_type: PacketType,
        is_path_request: bool,
        wire_len: usize,
        now: Instant,
    ) {
        let bytes = wire_len as u64;
        iface.traffic.tx_bytes = iface.traffic.tx_bytes.saturating_add(bytes);
        if packet_type == PacketType::Announce {
            iface.traffic.announce_tx_bytes = iface.traffic.announce_tx_bytes.saturating_add(bytes);
            iface.traffic.announce_tx_count = iface.traffic.announce_tx_count.saturating_add(1);
            InterfaceTraffic::push_time(&mut iface.traffic.announce_tx_times, now);
        } else if is_path_request {
            iface.traffic.path_request_tx_bytes =
                iface.traffic.path_request_tx_bytes.saturating_add(bytes);
            iface.traffic.path_request_tx_count =
                iface.traffic.path_request_tx_count.saturating_add(1);
            InterfaceTraffic::push_time(&mut iface.traffic.path_request_tx_times, now);
        }
    }

    pub fn record_protocol_violation(&mut self, address: AddressHash, description: &str) -> bool {
        let Some(iface) = self.ifaces.iter_mut().find(|iface| iface.address == address) else {
            return false;
        };
        iface.traffic.protocol_violations = iface.traffic.protocol_violations.saturating_add(1);
        log::debug!("protocol violation iface={address} description={description}");
        true
    }

    pub fn record_ifac_violation(&mut self, address: AddressHash, description: &str) -> bool {
        let Some(iface) = self.ifaces.iter_mut().find(|iface| iface.address == address) else {
            return false;
        };
        iface.traffic.ifac_violations = iface.traffic.ifac_violations.saturating_add(1);
        log::debug!("IFAC violation iface={address} description={description}");
        true
    }

    pub fn record_packet_filter_hit(&mut self, address: AddressHash) -> bool {
        let Some(iface) = self.ifaces.iter_mut().find(|iface| iface.address == address) else {
            return false;
        };
        iface.traffic.packet_filter_hits = iface.traffic.packet_filter_hits.saturating_add(1);
        true
    }

    pub fn should_ingress_limit_path_request(&mut self, address: AddressHash) -> bool {
        let Some(iface) = self.ifaces.iter_mut().find(|iface| iface.address == address) else {
            return false;
        };
        if iface.is_shared_instance || iface.shared_config.ingress_control == Some(false) {
            iface.traffic.path_request_burst_active = false;
            return false;
        }
        let now = Instant::now();
        let new_time = iface
            .shared_config
            .ic_new_time
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(Duration::from_secs_f64)
            .unwrap_or(DEFAULT_INGRESS_NEW_TIME);
        let threshold = if now.duration_since(iface.traffic.created_at) < new_time {
            iface.shared_config.ic_pr_burst_freq_new
        } else {
            iface.shared_config.ic_pr_burst_freq
        }
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(if now.duration_since(iface.traffic.created_at) < new_time {
            DEFAULT_PR_BURST_FREQ_NEW
        } else {
            DEFAULT_PR_BURST_FREQ
        });
        let hold = iface
            .shared_config
            .ic_burst_hold
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(Duration::from_secs_f64)
            .unwrap_or(DEFAULT_INGRESS_BURST_HOLD);
        let frequency = if iface.traffic.path_request_rx_times.len() <= 2 {
            0.0
        } else {
            InterfaceTraffic::frequency(&mut iface.traffic.path_request_rx_times, now)
        };

        if iface.traffic.path_request_burst_active {
            let activated_held = iface
                .traffic
                .path_request_burst_activated
                .is_some_and(|at| now.duration_since(at) > hold);
            let sustained_held = iface
                .traffic
                .path_request_burst_sustained
                .is_some_and(|at| now.duration_since(at) > hold);
            if frequency < threshold && activated_held && sustained_held {
                if iface.traffic.path_request_burst_cooldown == 0 {
                    iface.traffic.path_request_burst_active = false;
                } else {
                    iface.traffic.path_request_burst_cooldown -= 1;
                }
            } else {
                iface.traffic.path_request_burst_cooldown = 3;
                if frequency >= threshold {
                    iface.traffic.path_request_burst_sustained = Some(now);
                }
            }
            return true;
        }

        if frequency > threshold {
            iface.traffic.path_request_burst_active = true;
            iface.traffic.path_request_burst_activated = Some(now);
            iface.traffic.path_request_burst_sustained = Some(now);
            iface.traffic.path_request_burst_cooldown = 3;
            true
        } else {
            false
        }
    }

    pub fn traffic_snapshots(&mut self) -> Vec<InterfaceTrafficSnapshot> {
        let now = Instant::now();
        self.ifaces
            .iter_mut()
            .map(|iface| iface.traffic.snapshot(iface.address, iface.parent, now))
            .collect()
    }

    pub fn sample_traffic(&mut self) {
        let now = Instant::now();
        for iface in &mut self.ifaces {
            iface.traffic.update_speeds(now);
        }
    }
}

#[cfg(test)]
mod interface_traffic_tests {
    use super::*;

    #[test]
    fn rns_1_5_interface_traffic_counts_control_flow_and_violations() {
        let mut manager = InterfaceManager::new(4);
        let address = *manager.new_channel(4).address();

        assert!(manager.record_inbound_traffic(address, PacketType::Announce, false, 100));
        assert!(manager.record_inbound_traffic(address, PacketType::Data, true, 40));
        {
            let iface = manager.ifaces.first_mut().expect("interface");
            InterfaceManager::record_outbound_traffic(
                iface,
                PacketType::Announce,
                false,
                80,
                Instant::now(),
            );
            InterfaceManager::record_outbound_traffic(
                iface,
                PacketType::Data,
                true,
                30,
                Instant::now(),
            );
        }
        assert!(manager.record_protocol_violation(address, "bad packet"));
        assert!(manager.record_ifac_violation(address, "bad IFAC"));
        assert!(manager.record_packet_filter_hit(address));

        let snapshot = manager.traffic_snapshots().pop().expect("traffic snapshot");
        assert_eq!(snapshot.rx_bytes, 140);
        assert_eq!(snapshot.tx_bytes, 110);
        assert_eq!((snapshot.announce_rx_count, snapshot.announce_tx_count), (1, 1));
        assert_eq!((snapshot.announce_rx_bytes, snapshot.announce_tx_bytes), (100, 80));
        assert_eq!((snapshot.path_request_rx_count, snapshot.path_request_tx_count), (1, 1));
        assert_eq!((snapshot.path_request_rx_bytes, snapshot.path_request_tx_bytes), (40, 30));
        assert_eq!(snapshot.protocol_violations, 1);
        assert_eq!(snapshot.ifac_violations, 1);
        assert_eq!(snapshot.packet_filter_hits, 1);
    }

    #[test]
    fn rns_1_5_interface_traffic_rejects_unknown_interface_updates() {
        let mut manager = InterfaceManager::new(1);
        let missing = AddressHash::new_empty();
        assert!(!manager.record_inbound_traffic(missing, PacketType::Data, false, 10));
        assert!(!manager.record_protocol_violation(missing, "missing"));
        assert!(!manager.record_ifac_violation(missing, "missing"));
        assert!(!manager.record_packet_filter_hit(missing));
    }

    #[test]
    fn rns_1_5_interface_traffic_speed_is_bits_per_second() {
        let mut traffic = InterfaceTraffic::default();
        let sample_at = traffic.sample_at;
        traffic.rx_bytes = 100;
        traffic.announce_rx_bytes = 40;
        traffic.path_request_rx_bytes = 10;
        traffic.update_speeds(sample_at + Duration::from_secs(1));

        assert_eq!(traffic.rx_speed, 800.0);
        assert_eq!(traffic.announce_rx_speed, 320.0);
        assert_eq!(traffic.path_request_rx_speed, 80.0);
    }
}
