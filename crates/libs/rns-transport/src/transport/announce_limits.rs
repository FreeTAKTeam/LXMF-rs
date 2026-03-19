use alloc::collections::btree_map::Entry;
use core::cmp::Reverse;

use alloc::collections::{BTreeMap, VecDeque};

use tokio::time::Duration;
use tokio::time::Instant;

use crate::hash::AddressHash;
use crate::packet::Packet;

pub struct AnnounceRateLimit {
    pub incoming_freq_samples: usize,
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
pub enum AnnounceLimitAction {
    Allow,
    Hold(Duration),
}

#[derive(Clone, Copy)]
struct HeldAnnounce {
    packet: Packet,
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

    fn hold(&mut self, packet: &Packet, now: Instant, rate_limit: &AnnounceRateLimit) -> bool {
        if let Entry::Occupied(mut entry) = self.held_announces.entry(packet.destination) {
            entry.insert(HeldAnnounce { packet: *packet, held_at: now });
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

        self.held_announces
            .insert(packet.destination, HeldAnnounce { packet: *packet, held_at: now });
        true
    }

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

    fn release_one(&mut self, now: Instant, rate_limit: &AnnounceRateLimit) -> Option<Packet> {
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
            .map(|(destination, held)| (*destination, held.packet));

        let (destination, packet) = selected?;

        self.held_announces.remove(&destination);
        self.held_release = now + rate_limit.held_release_interval;
        Some(packet)
    }
}

pub struct ReleasedAnnounce {
    pub iface: AddressHash,
    pub packet: Packet,
}

pub struct AnnounceLimits {
    limits: BTreeMap<AddressHash, AnnounceLimitEntry>,
    rate_limit: AnnounceRateLimit,
}

impl AnnounceLimits {
    pub fn new() -> Self {
        Self::with_rate_limit(Default::default())
    }

    pub(crate) fn with_rate_limit(rate_limit: AnnounceRateLimit) -> Self {
        Self { limits: BTreeMap::new(), rate_limit }
    }

    pub fn check(
        &mut self,
        iface: AddressHash,
        packet: &Packet,
        destination_known: bool,
    ) -> AnnounceLimitAction {
        let now = Instant::now();
        let entry = self.limits.entry(iface).or_insert_with(|| AnnounceLimitEntry::new(now));
        entry.record_announce(now, &self.rate_limit);

        if destination_known {
            return AnnounceLimitAction::Allow;
        }

        if entry.should_ingress_limit(now, &self.rate_limit)
            && entry.hold(packet, now, &self.rate_limit)
        {
            return AnnounceLimitAction::Hold(entry.next_release_delay(now, &self.rate_limit));
        }

        AnnounceLimitAction::Allow
    }

    pub fn release_ready(&mut self) -> Vec<ReleasedAnnounce> {
        let now = Instant::now();
        let mut released = Vec::new();

        for (iface, entry) in self.limits.iter_mut() {
            if let Some(packet) = entry.release_one(now, &self.rate_limit) {
                released.push(ReleasedAnnounce { iface: *iface, packet });
            } else {
                continue;
            }
        }

        released
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{Header, PacketType};
    use std::thread::sleep;
    use std::time::Duration as StdDuration;

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

        assert_eq!(
            limits.check(iface_a, &announce_packet(AddressHash::new([1; 16]), 1), false),
            AnnounceLimitAction::Allow
        );
        sleep(StdDuration::from_millis(5));
        assert!(matches!(
            limits.check(iface_a, &announce_packet(AddressHash::new([2; 16]), 1), false),
            AnnounceLimitAction::Hold(_)
        ));
        assert_eq!(
            limits.check(iface_b, &announce_packet(AddressHash::new([3; 16]), 1), false),
            AnnounceLimitAction::Allow
        );
    }

    #[test]
    fn held_announces_release_lowest_hops_first() {
        let mut limits = AnnounceLimits::with_rate_limit(test_rate_limit());
        let iface = AddressHash::new([0xCC; crate::hash::ADDRESS_HASH_SIZE]);

        assert_eq!(
            limits.check(iface, &announce_packet(AddressHash::new([1; 16]), 4), false),
            AnnounceLimitAction::Allow
        );
        sleep(StdDuration::from_millis(5));
        assert!(matches!(
            limits.check(iface, &announce_packet(AddressHash::new([2; 16]), 3), false),
            AnnounceLimitAction::Hold(_)
        ));
        sleep(StdDuration::from_millis(5));
        assert!(matches!(
            limits.check(iface, &announce_packet(AddressHash::new([3; 16]), 1), false),
            AnnounceLimitAction::Hold(_)
        ));

        sleep(StdDuration::from_millis(55));
        assert!(limits.release_ready().is_empty());

        sleep(StdDuration::from_millis(25));
        let released = limits.release_ready();
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].iface, iface);
        assert_eq!(released[0].packet.destination, AddressHash::new([3; 16]));

        sleep(StdDuration::from_millis(15));

        let released = limits.release_ready();
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].packet.destination, AddressHash::new([2; 16]));
    }

    #[test]
    fn held_announces_evict_worst_entry_when_capacity_is_reached() {
        let mut rate_limit = test_rate_limit();
        rate_limit.max_held_announces = 1;
        let mut limits = AnnounceLimits::with_rate_limit(rate_limit);
        let iface = AddressHash::new([0xDD; crate::hash::ADDRESS_HASH_SIZE]);

        assert_eq!(
            limits.check(iface, &announce_packet(AddressHash::new([1; 16]), 4), false),
            AnnounceLimitAction::Allow
        );
        sleep(StdDuration::from_millis(5));
        assert!(matches!(
            limits.check(iface, &announce_packet(AddressHash::new([2; 16]), 5), false),
            AnnounceLimitAction::Hold(_)
        ));
        sleep(StdDuration::from_millis(5));
        assert!(matches!(
            limits.check(iface, &announce_packet(AddressHash::new([3; 16]), 1), false),
            AnnounceLimitAction::Hold(_)
        ));

        sleep(StdDuration::from_millis(55));
        assert!(limits.release_ready().is_empty());

        sleep(StdDuration::from_millis(25));
        let released = limits.release_ready();
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].packet.destination, AddressHash::new([3; 16]));
    }
}
