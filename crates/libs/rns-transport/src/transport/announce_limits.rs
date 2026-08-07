use alloc::collections::btree_map::Entry;
use core::cmp::Reverse;

use alloc::collections::{BTreeMap, VecDeque};

use tokio::time::Duration;
use tokio::time::Instant;

use crate::hash::AddressHash;
use crate::iface::{IfaceSource, InterfaceSharedConfig};
use crate::packet::{Packet, PacketContext};
use crate::ratchets::now_secs;

#[derive(Clone)]
pub struct AnnounceRateLimit {
    #[allow(dead_code)]
    pub incoming_freq_samples: usize,
    #[allow(dead_code)]
    pub max_held_announces: usize,
    pub new_time: Duration,
    pub burst_freq_new: f64,
    pub burst_freq: f64,
    pub burst_hold: Duration,
    pub burst_penalty: Duration,
    pub held_release_interval: Duration,
}

impl Default for AnnounceRateLimit {
    fn default() -> Self {
        Self {
            incoming_freq_samples: 6,
            max_held_announces: 256,
            new_time: Duration::from_secs(2 * 60 * 60),
            burst_freq_new: 3.5,
            burst_freq: 12.0,
            burst_hold: Duration::from_secs(60),
            burst_penalty: Duration::from_secs(5 * 60),
            held_release_interval: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AnnounceLimitAction {
    Allow,
    Hold(Duration),
}

#[derive(Clone)]
struct HeldAnnounce {
    packet: Packet,
    source: IfaceSource,
    held_at: Instant,
}

struct AnnounceLimitEntry {
    created_at: Instant,
    incoming: VecDeque<Instant>,
    burst_active: bool,
    burst_activated: Option<Instant>,
    held_release: Instant,
    held_announces: BTreeMap<AddressHash, HeldAnnounce>,
}

impl AnnounceLimitEntry {
    #[allow(dead_code)]
    pub fn new(now: Instant) -> Self {
        Self {
            created_at: now,
            incoming: VecDeque::new(),
            burst_active: false,
            burst_activated: None,
            held_release: now,
            held_announces: BTreeMap::new(),
        }
    }

    #[allow(dead_code)]
    fn record_announce(&mut self, now: Instant, rate_limit: &AnnounceRateLimit) {
        self.incoming.push_back(now);
        while self.incoming.len() > rate_limit.incoming_freq_samples {
            self.incoming.pop_front();
        }
    }

    fn age(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.created_at)
    }

    fn incoming_announce_frequency(&self, now: Instant) -> f64 {
        if self.incoming.len() <= 1 {
            return 0.0;
        }

        let mut delta_sum = Duration::ZERO;
        for idx in 1..self.incoming.len() {
            delta_sum += self.incoming[idx].saturating_duration_since(self.incoming[idx - 1]);
        }
        if let Some(last) = self.incoming.back().copied() {
            delta_sum += now.saturating_duration_since(last);
        }

        if delta_sum.is_zero() {
            0.0
        } else {
            let avg = delta_sum.as_secs_f64() / self.incoming.len() as f64;
            if avg == 0.0 {
                0.0
            } else {
                1.0 / avg
            }
        }
    }

    fn threshold(&self, now: Instant, rate_limit: &AnnounceRateLimit) -> f64 {
        if self.age(now) < rate_limit.new_time {
            rate_limit.burst_freq_new
        } else {
            rate_limit.burst_freq
        }
    }

    fn should_ingress_limit(&mut self, now: Instant, rate_limit: &AnnounceRateLimit) -> bool {
        let freq_threshold = self.threshold(now, rate_limit);
        let incoming_freq = self.incoming_announce_frequency(now);

        if self.burst_active {
            if incoming_freq < freq_threshold {
                if let Some(activated_at) = self.burst_activated {
                    if now >= activated_at + rate_limit.burst_hold {
                        self.burst_active = false;
                        self.burst_activated = None;
                        self.held_release = now + rate_limit.burst_penalty;
                    }
                }
            }

            true
        } else if incoming_freq > freq_threshold {
            self.burst_active = true;
            self.burst_activated = Some(now);
            true
        } else {
            false
        }
    }

    #[allow(dead_code)]
    fn hold(
        &mut self,
        packet: &Packet,
        source: IfaceSource,
        now: Instant,
        rate_limit: &AnnounceRateLimit,
    ) -> bool {
        if let Entry::Occupied(mut entry) = self.held_announces.entry(packet.destination) {
            entry.insert(HeldAnnounce { packet: packet.clone(), source, held_at: now });
            return true;
        }

        if rate_limit.max_held_announces == 0 {
            return false;
        }

        if self.held_announces.len() >= rate_limit.max_held_announces {
            let worst_destination = self
                .held_announces
                .iter()
                .max_by_key(|(_, held)| (held.packet.header.hops, Reverse(held.held_at)))
                .map(|(destination, _)| *destination);

            if let Some(destination) = worst_destination {
                self.held_announces.remove(&destination);
            }
        }

        self.held_announces.insert(
            packet.destination,
            HeldAnnounce { packet: packet.clone(), source, held_at: now },
        );
        true
    }

    #[allow(dead_code)]
    fn next_release_delay(&self, now: Instant, rate_limit: &AnnounceRateLimit) -> Duration {
        if self.burst_active {
            let hold_until = self
                .burst_activated
                .map(|activated_at| activated_at + rate_limit.burst_hold + rate_limit.burst_penalty)
                .unwrap_or(now);
            return hold_until.saturating_duration_since(now);
        }

        self.held_release.saturating_duration_since(now)
    }

    fn release_one(
        &mut self,
        now: Instant,
        rate_limit: &AnnounceRateLimit,
    ) -> Option<(Packet, IfaceSource)> {
        if self.held_announces.is_empty() || self.should_ingress_limit(now, rate_limit) {
            return None;
        }

        if now < self.held_release {
            return None;
        }

        let selected = self
            .held_announces
            .iter()
            .min_by_key(|(_, held)| (held.packet.header.hops, held.held_at))
            .map(|(destination, held)| (*destination, held.packet.clone(), held.source));

        let (destination, packet, source) = selected?;

        self.held_announces.remove(&destination);
        self.held_release = now + rate_limit.held_release_interval;
        Some((packet, source))
    }
}

#[derive(Clone)]
struct AnnounceRateTargetEntry {
    last: Instant,
    last_epoch: f64,
    rate_violations: u64,
    blocked_until: Instant,
    blocked_until_epoch: f64,
    timestamps: VecDeque<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnnounceRateTableEntry {
    pub destination: AddressHash,
    pub last: f64,
    pub rate_violations: u64,
    pub blocked_until: f64,
    pub timestamps: Vec<f64>,
}

pub struct ReleasedAnnounce {
    pub iface: AddressHash,
    pub packet: Packet,
    pub source: IfaceSource,
}

pub struct AnnounceLimits {
    limits: BTreeMap<AddressHash, AnnounceLimitEntry>,
    announce_rate_targets: BTreeMap<AddressHash, AnnounceRateTargetEntry>,
    rate_limit: AnnounceRateLimit,
    interface_rate_limits: BTreeMap<AddressHash, AnnounceRateLimit>,
}

impl AnnounceLimits {
    pub fn new() -> Self {
        Self::with_rate_limit(Default::default())
    }

    pub(crate) fn with_rate_limit(rate_limit: AnnounceRateLimit) -> Self {
        Self {
            limits: BTreeMap::new(),
            announce_rate_targets: BTreeMap::new(),
            rate_limit,
            interface_rate_limits: BTreeMap::new(),
        }
    }

    #[allow(dead_code)]
    pub fn check(
        &mut self,
        iface: AddressHash,
        packet: &Packet,
        source: IfaceSource,
        destination_known: bool,
    ) -> AnnounceLimitAction {
        self.check_with_shared_config(
            iface,
            packet,
            source,
            destination_known,
            &InterfaceSharedConfig::default(),
        )
    }

    pub fn check_with_shared_config(
        &mut self,
        iface: AddressHash,
        packet: &Packet,
        source: IfaceSource,
        destination_known: bool,
        shared_config: &InterfaceSharedConfig,
    ) -> AnnounceLimitAction {
        self.check_with_shared_config_at(
            iface,
            packet,
            source,
            destination_known,
            shared_config,
            Instant::now(),
        )
    }

    #[allow(dead_code)]
    fn check_at(
        &mut self,
        iface: AddressHash,
        packet: &Packet,
        source: IfaceSource,
        destination_known: bool,
        now: Instant,
    ) -> AnnounceLimitAction {
        self.check_with_shared_config_at(
            iface,
            packet,
            source,
            destination_known,
            &InterfaceSharedConfig::default(),
            now,
        )
    }

    fn check_with_shared_config_at(
        &mut self,
        iface: AddressHash,
        packet: &Packet,
        source: IfaceSource,
        destination_known: bool,
        shared_config: &InterfaceSharedConfig,
        now: Instant,
    ) -> AnnounceLimitAction {
        if packet.context == PacketContext::PathResponse {
            return AnnounceLimitAction::Allow;
        }
        if shared_config.ingress_control == Some(false) {
            self.interface_rate_limits.remove(&iface);
            return AnnounceLimitAction::Allow;
        }

        let rate_limit = self.rate_limit_for(shared_config);
        self.interface_rate_limits.insert(iface, rate_limit.clone());
        let entry = self.limits.entry(iface).or_insert_with(|| AnnounceLimitEntry::new(now));
        entry.record_announce(now, &rate_limit);

        if destination_known {
            return AnnounceLimitAction::Allow;
        }

        if entry.should_ingress_limit(now, &rate_limit)
            && entry.hold(packet, source, now, &rate_limit)
        {
            return AnnounceLimitAction::Hold(entry.next_release_delay(now, &rate_limit));
        }

        AnnounceLimitAction::Allow
    }

    fn rate_limit_for(&self, shared_config: &InterfaceSharedConfig) -> AnnounceRateLimit {
        let mut rate_limit = self.rate_limit.clone();
        if let Some(value) =
            shared_config.ic_max_held_announces.and_then(|value| usize::try_from(value).ok())
        {
            rate_limit.max_held_announces = value;
        }
        if let Some(value) = nonnegative_duration_secs(shared_config.ic_new_time) {
            rate_limit.new_time = value;
        }
        if let Some(value) = nonnegative_f64(shared_config.ic_burst_freq_new) {
            rate_limit.burst_freq_new = value;
        }
        if let Some(value) = nonnegative_f64(shared_config.ic_burst_freq) {
            rate_limit.burst_freq = value;
        }
        if let Some(value) = nonnegative_duration_secs(shared_config.ic_burst_hold) {
            rate_limit.burst_hold = value;
        }
        if let Some(value) = nonnegative_duration_secs(shared_config.ic_burst_penalty) {
            rate_limit.burst_penalty = value;
        }
        if let Some(value) = nonnegative_duration_secs(shared_config.ic_held_release_interval) {
            rate_limit.held_release_interval = value;
        }
        rate_limit
    }

    pub fn should_suppress_rebroadcast(
        &mut self,
        packet: &Packet,
        shared_config: &InterfaceSharedConfig,
    ) -> bool {
        self.should_suppress_rebroadcast_at(packet, shared_config, Instant::now())
    }

    #[allow(dead_code)]
    fn should_suppress_rebroadcast_at(
        &mut self,
        packet: &Packet,
        shared_config: &InterfaceSharedConfig,
        now: Instant,
    ) -> bool {
        if packet.context == PacketContext::PathResponse {
            return false;
        }

        let Some(target) = shared_config
            .announce_rate_target
            .filter(|target| *target > 0)
            .map(Duration::from_secs)
        else {
            return false;
        };
        let grace = shared_config.announce_rate_grace.unwrap_or(0);
        let penalty = Duration::from_secs(shared_config.announce_rate_penalty.unwrap_or(0));

        let Entry::Occupied(mut entry) = self.announce_rate_targets.entry(packet.destination)
        else {
            let epoch = now_secs();
            self.announce_rate_targets.insert(
                packet.destination,
                AnnounceRateTargetEntry {
                    last: now,
                    last_epoch: epoch,
                    rate_violations: 0,
                    blocked_until: now,
                    blocked_until_epoch: 0.0,
                    timestamps: VecDeque::from([epoch]),
                },
            );
            return false;
        };

        let entry = entry.get_mut();
        let epoch = now_secs();
        entry.timestamps.push_back(epoch);
        while entry.timestamps.len() > 16 {
            entry.timestamps.pop_front();
        }
        if now <= entry.blocked_until {
            return true;
        }

        let current_rate = now.saturating_duration_since(entry.last);
        if current_rate < target {
            entry.rate_violations = entry.rate_violations.saturating_add(1);
        } else {
            entry.rate_violations = entry.rate_violations.saturating_sub(1);
        }

        if entry.rate_violations > grace {
            entry.blocked_until = entry.last + target + penalty;
            entry.blocked_until_epoch =
                entry.last_epoch + target.as_secs_f64() + penalty.as_secs_f64();
            true
        } else {
            entry.last = now;
            entry.last_epoch = epoch;
            false
        }
    }

    pub fn rate_table(&self) -> Vec<AnnounceRateTableEntry> {
        self.announce_rate_targets
            .iter()
            .map(|(destination, entry)| AnnounceRateTableEntry {
                destination: *destination,
                last: entry.last_epoch,
                rate_violations: entry.rate_violations,
                blocked_until: entry.blocked_until_epoch,
                timestamps: entry.timestamps.iter().copied().collect(),
            })
            .collect()
    }

    pub fn release_ready(&mut self) -> Vec<ReleasedAnnounce> {
        self.release_ready_at(Instant::now())
    }

    pub fn clear_held_announces(&mut self) -> usize {
        self.limits
            .values_mut()
            .map(|entry| {
                let count = entry.held_announces.len();
                entry.held_announces.clear();
                count
            })
            .sum()
    }

    fn release_ready_at(&mut self, now: Instant) -> Vec<ReleasedAnnounce> {
        let mut released = Vec::new();

        for (iface, entry) in self.limits.iter_mut() {
            let rate_limit = self.interface_rate_limits.get(iface).unwrap_or(&self.rate_limit);
            if let Some((packet, source)) = entry.release_one(now, rate_limit) {
                released.push(ReleasedAnnounce { iface: *iface, packet, source });
            } else {
                continue;
            }
        }

        released
    }
}

fn nonnegative_f64(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value >= 0.0)
}

fn nonnegative_duration_secs(value: Option<f64>) -> Option<Duration> {
    nonnegative_f64(value).map(Duration::from_secs_f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{Header, PacketType};

    fn test_rate_limit() -> AnnounceRateLimit {
        AnnounceRateLimit {
            incoming_freq_samples: 3,
            max_held_announces: 8,
            new_time: Duration::from_secs(3600),
            burst_freq_new: 100.0,
            burst_freq: 100.0,
            burst_hold: Duration::from_millis(20),
            burst_penalty: Duration::from_millis(20),
            held_release_interval: Duration::from_millis(10),
        }
    }

    fn announce_packet(destination: AddressHash, hops: u8) -> Packet {
        Packet {
            header: Header { packet_type: PacketType::Announce, hops, ..Default::default() },
            destination,
            ..Default::default()
        }
    }

    #[test]
    fn ingress_limiting_is_scoped_per_interface() {
        let mut limits = AnnounceLimits::with_rate_limit(test_rate_limit());
        let iface_a = AddressHash::new([0xAA; crate::hash::ADDRESS_HASH_SIZE]);
        let iface_b = AddressHash::new([0xBB; crate::hash::ADDRESS_HASH_SIZE]);
        let now = Instant::now();

        assert_eq!(
            limits.check_at(
                iface_a,
                &announce_packet(AddressHash::new([1; 16]), 1),
                IfaceSource::None,
                false,
                now,
            ),
            AnnounceLimitAction::Allow
        );
        assert!(matches!(
            limits.check_at(
                iface_a,
                &announce_packet(AddressHash::new([2; 16]), 1),
                IfaceSource::None,
                false,
                now + Duration::from_millis(5)
            ),
            AnnounceLimitAction::Hold(_)
        ));
        assert_eq!(
            limits.check_at(
                iface_b,
                &announce_packet(AddressHash::new([3; 16]), 1),
                IfaceSource::None,
                false,
                now + Duration::from_millis(5)
            ),
            AnnounceLimitAction::Allow
        );
    }

    #[test]
    fn held_announces_release_lowest_hops_first() {
        let mut limits = AnnounceLimits::with_rate_limit(test_rate_limit());
        let iface = AddressHash::new([0xCC; crate::hash::ADDRESS_HASH_SIZE]);
        let now = Instant::now();

        assert_eq!(
            limits.check_at(
                iface,
                &announce_packet(AddressHash::new([1; 16]), 4),
                IfaceSource::None,
                false,
                now,
            ),
            AnnounceLimitAction::Allow
        );
        assert!(matches!(
            limits.check_at(
                iface,
                &announce_packet(AddressHash::new([2; 16]), 3),
                IfaceSource::None,
                false,
                now + Duration::from_millis(5)
            ),
            AnnounceLimitAction::Hold(_)
        ));
        assert!(matches!(
            limits.check_at(
                iface,
                &announce_packet(AddressHash::new([3; 16]), 1),
                IfaceSource::None,
                false,
                now + Duration::from_millis(10)
            ),
            AnnounceLimitAction::Hold(_)
        ));

        assert!(limits.release_ready_at(now + Duration::from_millis(65)).is_empty());

        let released = limits.release_ready_at(now + Duration::from_millis(90));
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].iface, iface);
        assert_eq!(released[0].packet.destination, AddressHash::new([3; 16]));

        let released = limits.release_ready_at(now + Duration::from_millis(105));
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].packet.destination, AddressHash::new([2; 16]));
    }

    #[test]
    fn held_announces_release_one_lowest_hop_entry_per_interface() {
        let mut limits = AnnounceLimits::with_rate_limit(test_rate_limit());
        let iface_a = AddressHash::new([0xCA; crate::hash::ADDRESS_HASH_SIZE]);
        let iface_b = AddressHash::new([0xCB; crate::hash::ADDRESS_HASH_SIZE]);
        let now = Instant::now();

        assert_eq!(
            limits.check_at(
                iface_a,
                &announce_packet(AddressHash::new([0x11; 16]), 1),
                IfaceSource::None,
                false,
                now,
            ),
            AnnounceLimitAction::Allow
        );
        assert!(matches!(
            limits.check_at(
                iface_a,
                &announce_packet(AddressHash::new([0x12; 16]), 4),
                IfaceSource::None,
                false,
                now + Duration::from_millis(5),
            ),
            AnnounceLimitAction::Hold(_)
        ));
        assert!(matches!(
            limits.check_at(
                iface_a,
                &announce_packet(AddressHash::new([0x13; 16]), 2),
                IfaceSource::None,
                false,
                now + Duration::from_millis(10),
            ),
            AnnounceLimitAction::Hold(_)
        ));

        assert_eq!(
            limits.check_at(
                iface_b,
                &announce_packet(AddressHash::new([0x21; 16]), 1),
                IfaceSource::None,
                false,
                now,
            ),
            AnnounceLimitAction::Allow
        );
        assert!(matches!(
            limits.check_at(
                iface_b,
                &announce_packet(AddressHash::new([0x22; 16]), 5),
                IfaceSource::None,
                false,
                now + Duration::from_millis(5),
            ),
            AnnounceLimitAction::Hold(_)
        ));
        assert!(matches!(
            limits.check_at(
                iface_b,
                &announce_packet(AddressHash::new([0x23; 16]), 3),
                IfaceSource::None,
                false,
                now + Duration::from_millis(10),
            ),
            AnnounceLimitAction::Hold(_)
        ));

        assert!(limits.release_ready_at(now + Duration::from_millis(65)).is_empty());

        let mut released: Vec<_> = limits
            .release_ready_at(now + Duration::from_millis(90))
            .iter()
            .map(|released| (released.iface, released.packet.destination))
            .collect();
        released.sort();
        let mut expected =
            vec![(iface_a, AddressHash::new([0x13; 16])), (iface_b, AddressHash::new([0x23; 16]))];
        expected.sort();
        assert_eq!(released, expected);

        let mut released: Vec<_> = limits
            .release_ready_at(now + Duration::from_millis(105))
            .iter()
            .map(|released| (released.iface, released.packet.destination))
            .collect();
        released.sort();
        let mut expected =
            vec![(iface_a, AddressHash::new([0x12; 16])), (iface_b, AddressHash::new([0x22; 16]))];
        expected.sort();
        assert_eq!(released, expected);
    }

    #[test]
    fn held_announces_evict_worst_entry_when_capacity_is_reached() {
        let mut rate_limit = test_rate_limit();
        rate_limit.max_held_announces = 1;
        let mut limits = AnnounceLimits::with_rate_limit(rate_limit);
        let iface = AddressHash::new([0xDD; crate::hash::ADDRESS_HASH_SIZE]);
        let now = Instant::now();

        assert_eq!(
            limits.check_at(
                iface,
                &announce_packet(AddressHash::new([1; 16]), 4),
                IfaceSource::None,
                false,
                now,
            ),
            AnnounceLimitAction::Allow
        );
        assert!(matches!(
            limits.check_at(
                iface,
                &announce_packet(AddressHash::new([2; 16]), 5),
                IfaceSource::None,
                false,
                now + Duration::from_millis(5)
            ),
            AnnounceLimitAction::Hold(_)
        ));
        assert!(matches!(
            limits.check_at(
                iface,
                &announce_packet(AddressHash::new([3; 16]), 1),
                IfaceSource::None,
                false,
                now + Duration::from_millis(10)
            ),
            AnnounceLimitAction::Hold(_)
        ));

        assert!(limits.release_ready_at(now + Duration::from_millis(65)).is_empty());

        let released = limits.release_ready_at(now + Duration::from_millis(90));
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].packet.destination, AddressHash::new([3; 16]));
    }

    #[test]
    fn shared_ingress_control_false_disables_announce_holding() {
        let mut rate_limit = test_rate_limit();
        rate_limit.burst_freq_new = 0.1;
        rate_limit.burst_freq = 0.1;
        let mut limits = AnnounceLimits::with_rate_limit(rate_limit);
        let iface = AddressHash::new([0xEE; crate::hash::ADDRESS_HASH_SIZE]);
        let config = InterfaceSharedConfig { ingress_control: Some(false), ..Default::default() };
        let now = Instant::now();

        assert_eq!(
            limits.check_with_shared_config_at(
                iface,
                &announce_packet(AddressHash::new([1; 16]), 1),
                IfaceSource::None,
                false,
                &config,
                now,
            ),
            AnnounceLimitAction::Allow
        );
        assert_eq!(
            limits.check_with_shared_config_at(
                iface,
                &announce_packet(AddressHash::new([2; 16]), 1),
                IfaceSource::None,
                false,
                &config,
                now + Duration::from_millis(1),
            ),
            AnnounceLimitAction::Allow
        );
        assert!(limits.release_ready_at(now + Duration::from_secs(1)).is_empty());
    }

    #[test]
    fn shared_ingress_control_fields_override_default_limiter() {
        let mut default_limit = test_rate_limit();
        default_limit.burst_freq_new = 10_000.0;
        default_limit.burst_freq = 10_000.0;
        let mut limits = AnnounceLimits::with_rate_limit(default_limit);
        let iface = AddressHash::new([0xEF; crate::hash::ADDRESS_HASH_SIZE]);
        let config = InterfaceSharedConfig {
            ic_max_held_announces: Some(1),
            ic_burst_freq_new: Some(100.0),
            ic_burst_freq: Some(100.0),
            ic_burst_hold: Some(0.02),
            ic_burst_penalty: Some(0.02),
            ic_held_release_interval: Some(0.01),
            ..Default::default()
        };
        let now = Instant::now();

        assert_eq!(
            limits.check_with_shared_config_at(
                iface,
                &announce_packet(AddressHash::new([1; 16]), 3),
                IfaceSource::None,
                false,
                &config,
                now,
            ),
            AnnounceLimitAction::Allow
        );
        assert!(matches!(
            limits.check_with_shared_config_at(
                iface,
                &announce_packet(AddressHash::new([2; 16]), 5),
                IfaceSource::None,
                false,
                &config,
                now + Duration::from_millis(1),
            ),
            AnnounceLimitAction::Hold(_)
        ));
        assert!(matches!(
            limits.check_with_shared_config_at(
                iface,
                &announce_packet(AddressHash::new([3; 16]), 1),
                IfaceSource::None,
                false,
                &config,
                now + Duration::from_millis(2),
            ),
            AnnounceLimitAction::Hold(_)
        ));

        assert!(limits.release_ready_at(now + Duration::from_millis(60)).is_empty());

        let released = limits.release_ready_at(now + Duration::from_millis(90));
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].packet.destination, AddressHash::new([3; 16]));
    }

    #[test]
    fn announce_rate_target_suppresses_after_grace_is_exceeded() {
        let mut limits = AnnounceLimits::new();
        let destination = AddressHash::new([0x41; crate::hash::ADDRESS_HASH_SIZE]);
        let packet = announce_packet(destination, 1);
        let config = InterfaceSharedConfig {
            announce_rate_target: Some(60),
            announce_rate_grace: Some(1),
            announce_rate_penalty: Some(30),
            ..Default::default()
        };
        let now = Instant::now();

        assert!(!limits.should_suppress_rebroadcast_at(&packet, &config, now));
        assert!(!limits.should_suppress_rebroadcast_at(
            &packet,
            &config,
            now + Duration::from_secs(1),
        ));
        assert!(limits.should_suppress_rebroadcast_at(
            &packet,
            &config,
            now + Duration::from_secs(2),
        ));
        assert!(limits.should_suppress_rebroadcast_at(
            &packet,
            &config,
            now + Duration::from_secs(89),
        ));
        assert!(!limits.should_suppress_rebroadcast_at(
            &packet,
            &config,
            now + Duration::from_secs(92),
        ));

        let rows = limits.rate_table();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].destination, destination);
        assert_eq!(rows[0].rate_violations, 1);
        assert_eq!(rows[0].timestamps.len(), 5);
        assert!(rows[0].last > 0.0);
        assert!(rows[0].blocked_until > rows[0].last);
    }

    #[test]
    fn announce_rate_table_retains_python_timestamp_limit() {
        let mut limits = AnnounceLimits::new();
        let destination = AddressHash::new([0x43; crate::hash::ADDRESS_HASH_SIZE]);
        let packet = announce_packet(destination, 1);
        let config = InterfaceSharedConfig {
            announce_rate_target: Some(60),
            announce_rate_grace: Some(u64::MAX),
            ..Default::default()
        };
        let now = Instant::now();

        for offset in 0..20 {
            limits.should_suppress_rebroadcast_at(
                &packet,
                &config,
                now + Duration::from_secs(offset),
            );
        }

        assert_eq!(limits.rate_table()[0].timestamps.len(), 16);
    }

    #[test]
    fn announce_rate_target_ignores_path_responses_and_zero_targets() {
        let mut limits = AnnounceLimits::new();
        let destination = AddressHash::new([0x42; crate::hash::ADDRESS_HASH_SIZE]);
        let mut packet = announce_packet(destination, 1);
        let now = Instant::now();

        let zero_target =
            InterfaceSharedConfig { announce_rate_target: Some(0), ..Default::default() };
        assert!(!limits.should_suppress_rebroadcast_at(&packet, &zero_target, now));
        assert!(!limits.should_suppress_rebroadcast_at(
            &packet,
            &zero_target,
            now + Duration::from_secs(1),
        ));

        packet.context = PacketContext::PathResponse;
        let config = InterfaceSharedConfig {
            announce_rate_target: Some(60),
            announce_rate_grace: Some(0),
            ..Default::default()
        };
        assert!(!limits.should_suppress_rebroadcast_at(&packet, &config, now));
        assert!(!limits.should_suppress_rebroadcast_at(
            &packet,
            &config,
            now + Duration::from_secs(1),
        ));
    }
}
