use alloc::{collections::VecDeque, vec::Vec};
use rand_core::CryptoRngCore;
use rns_core::{
    destination::{self, DestinationAnnounce, SingleInputDestination},
    hash::AddressHash,
    identity::{Identity, PrivateIdentity},
    packet::{
        ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
        PacketDataBuffer, PacketType, PropagationType,
    },
};

use lxmf_core::message::{Message as LxmfMessage, WireMessage};

use crate::{
    adapters::FrameLink,
    config::MiniNodeConfig,
    error::MiniNodeError,
    event::NodeEvent,
    store::{MiniNodeStore, NeighborSnapshot, NodeSnapshot},
    telemetry::{
        encode_recent_fields, PositionFix, TelemetryPoint, TelemetryQuery, TelemetrySample,
    },
};

#[derive(Clone)]
pub struct NeighborRecord {
    pub destination_hash: [u8; 16],
    pub identity: Identity,
    pub last_seen_ms: u64,
    pub app_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEnvelope {
    pub destination_hash: [u8; 16],
    pub message_id: [u8; 32],
}

pub struct MiniNode<S: MiniNodeStore> {
    config: MiniNodeConfig,
    store: S,
    identity: PrivateIdentity,
    delivery_destination: SingleInputDestination,
    neighbors: VecDeque<NeighborRecord>,
    recent_message_ids: VecDeque<[u8; 32]>,
    outbound_frames: VecDeque<Vec<u8>>,
    telemetry: VecDeque<TelemetryPoint>,
    latest_position: Option<PositionFix>,
    events: VecDeque<NodeEvent>,
    last_announce_ms: Option<u64>,
}

impl<S: MiniNodeStore> MiniNode<S> {
    pub fn load_or_create<R: CryptoRngCore + Copy>(
        rng: R,
        config: MiniNodeConfig,
        mut store: S,
    ) -> Result<Self, MiniNodeError> {
        let identity = match store.load_identity()? {
            Some(bytes) => PrivateIdentity::from_private_key_bytes(&bytes)?,
            None => {
                let identity = PrivateIdentity::new_from_rand(rng);
                store.save_identity(&identity.to_private_key_bytes())?;
                identity
            }
        };

        let snapshot = store.load_snapshot()?.unwrap_or_default();
        let mut node = Self::new_with_identity(identity, config, store, snapshot);
        node.push_event(NodeEvent::IdentityReady { destination_hash: node.destination_hash() });
        Ok(node)
    }

    pub fn new_with_identity(
        identity: PrivateIdentity,
        config: MiniNodeConfig,
        store: S,
        snapshot: NodeSnapshot,
    ) -> Self {
        let delivery_destination =
            destination::new_in(identity.clone(), config.app_name, config.aspect);
        let mut node = Self {
            config,
            store,
            identity,
            delivery_destination,
            neighbors: VecDeque::new(),
            recent_message_ids: VecDeque::new(),
            outbound_frames: VecDeque::new(),
            telemetry: VecDeque::new(),
            latest_position: None,
            events: VecDeque::new(),
            last_announce_ms: snapshot.last_announce_ms,
        };
        node.restore_snapshot(snapshot);
        node
    }

    pub fn tick<R: CryptoRngCore + Copy, L: FrameLink>(
        &mut self,
        now_ms: u64,
        rng: R,
        link: &mut L,
    ) -> Result<(), MiniNodeError> {
        self.maybe_auto_announce(now_ms, rng)?;
        self.flush_outbound(link)?;
        self.poll_inbound(now_ms, link)?;
        Ok(())
    }

    pub fn destination_hash(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out.copy_from_slice(self.delivery_destination.desc.address_hash.as_slice());
        out
    }

    pub fn identity(&self) -> &PrivateIdentity {
        &self.identity
    }

    pub fn neighbors(&self) -> impl Iterator<Item = &NeighborRecord> {
        self.neighbors.iter()
    }

    pub fn poll_event(&mut self) -> Option<NodeEvent> {
        self.events.pop_front()
    }

    pub fn queue_announce<R: CryptoRngCore + Copy>(
        &mut self,
        now_ms: u64,
        rng: R,
        app_data: Option<&[u8]>,
    ) -> Result<(), MiniNodeError> {
        if self.outbound_frames.len() >= self.config.max_outbound_frames {
            self.push_event(NodeEvent::AnnounceSkipped {
                destination_hash: self.destination_hash(),
            });
            return Ok(());
        }

        let packet = self.delivery_destination.announce(rng, app_data)?;
        let bytes = packet.to_bytes()?;
        self.outbound_frames.push_back(bytes);
        self.last_announce_ms = Some(now_ms);
        self.persist_snapshot()?;
        self.push_event(NodeEvent::AnnounceQueued { destination_hash: self.destination_hash() });
        Ok(())
    }

    pub fn send_message(
        &mut self,
        destination_hash: [u8; 16],
        content: &[u8],
    ) -> Result<MessageEnvelope, MiniNodeError> {
        self.send_message_with_fields(destination_hash, content, None)
    }

    pub fn send_message_with_latest_telemetry(
        &mut self,
        destination_hash: [u8; 16],
        content: &[u8],
        limit: usize,
    ) -> Result<MessageEnvelope, MiniNodeError> {
        let telemetry = self.telemetry.iter().rev().take(limit).cloned().collect::<Vec<_>>();
        let fields = if telemetry.is_empty() && self.latest_position.is_none() {
            None
        } else {
            Some(encode_recent_fields(&telemetry, self.latest_position.as_ref())?)
        };
        self.send_message_with_fields(destination_hash, content, fields)
    }

    pub fn record_telemetry(&mut self, sample: TelemetrySample) -> Result<(), MiniNodeError> {
        let (points, latest_position) = sample.into_points(self.destination_hash());
        let mut inserted = 0usize;
        let mut dropped = 0usize;

        if let Some(position) = latest_position {
            self.latest_position = Some(position);
        }

        for point in points {
            if self.telemetry.len() >= self.config.max_telemetry_points {
                self.telemetry.pop_front();
                dropped += 1;
            }
            self.telemetry.push_back(point);
            inserted += 1;
        }

        if inserted > 0 {
            self.push_event(NodeEvent::TelemetryRecorded { inserted });
        }
        if dropped > 0 {
            self.push_event(NodeEvent::TelemetryDropped { dropped });
        }

        self.persist_snapshot()?;
        Ok(())
    }

    pub fn query_telemetry(&self, query: &TelemetryQuery) -> Vec<TelemetryPoint> {
        let mut matches =
            self.telemetry.iter().filter(|point| query.matches(point)).cloned().collect::<Vec<_>>();

        if let Some(limit) = query.limit {
            if matches.len() > limit {
                matches = matches[matches.len() - limit..].to_vec();
            }
        }

        matches
    }

    fn send_message_with_fields(
        &mut self,
        destination_hash: [u8; 16],
        content: &[u8],
        fields: Option<rmpv::Value>,
    ) -> Result<MessageEnvelope, MiniNodeError> {
        if self.outbound_frames.len() >= self.config.max_outbound_frames {
            return Err(MiniNodeError::QueueFull);
        }

        let mut message = LxmfMessage::new();
        message.destination_hash = Some(destination_hash);
        message.source_hash = Some(self.destination_hash());
        message.set_content_from_bytes(content);
        message.fields = fields;

        let wire = message.to_wire(Some(&self.identity))?;
        let unpacked = WireMessage::unpack(&wire)?;
        let message_id = unpacked.message_id();

        let packet = Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination: AddressHash::new(destination_hash),
            transport: None,
            context: PacketContext::None,
            data: PacketDataBuffer::new_from_slice(&wire),
        };

        let bytes = packet.to_bytes()?;
        self.outbound_frames.push_back(bytes);
        self.remember_message_id(message_id);
        self.persist_snapshot()?;
        self.push_event(NodeEvent::MessageQueued { destination_hash, message_id });
        Ok(MessageEnvelope { destination_hash, message_id })
    }

    fn maybe_auto_announce<R: CryptoRngCore + Copy>(
        &mut self,
        now_ms: u64,
        rng: R,
    ) -> Result<(), MiniNodeError> {
        let should_announce = match self.last_announce_ms {
            Some(last) => now_ms.saturating_sub(last) >= self.config.announce_interval_ms,
            None => true,
        };

        if should_announce {
            self.queue_announce(now_ms, rng, None)?;
        }
        Ok(())
    }

    fn flush_outbound<L: FrameLink>(&mut self, link: &mut L) -> Result<(), MiniNodeError> {
        while let Some(frame) = self.outbound_frames.front() {
            if frame.len() > link.mtu() {
                return Err(MiniNodeError::MtuExceeded { frame_len: frame.len(), mtu: link.mtu() });
            }
            link.send_frame(frame)?;
            self.outbound_frames.pop_front();
        }
        Ok(())
    }

    fn poll_inbound<L: FrameLink>(
        &mut self,
        now_ms: u64,
        link: &mut L,
    ) -> Result<(), MiniNodeError> {
        for _ in 0..8 {
            let Some(frame) = link.poll_frame()? else {
                break;
            };
            self.handle_frame(now_ms, &frame)?;
        }
        Ok(())
    }

    fn handle_frame(&mut self, now_ms: u64, frame: &[u8]) -> Result<(), MiniNodeError> {
        let packet = Packet::from_bytes(frame)?;
        match packet.header.packet_type {
            PacketType::Announce => self.handle_announce(now_ms, &packet),
            PacketType::Data
                if packet.destination == self.delivery_destination.desc.address_hash =>
            {
                self.handle_message(&packet)
            }
            _ => Ok(()),
        }
    }

    fn handle_announce(&mut self, now_ms: u64, packet: &Packet) -> Result<(), MiniNodeError> {
        let info = DestinationAnnounce::validate(packet)?;
        let mut destination_hash = [0u8; 16];
        destination_hash.copy_from_slice(info.destination.desc.address_hash.as_slice());
        let app_data = info.app_data.to_vec();

        if let Some(existing) =
            self.neighbors.iter_mut().find(|neighbor| neighbor.destination_hash == destination_hash)
        {
            existing.identity = info.destination.desc.identity;
            existing.last_seen_ms = now_ms;
            existing.app_data = app_data.clone();
        } else {
            if self.neighbors.len() >= self.config.max_neighbors {
                self.neighbors.pop_front();
            }
            self.neighbors.push_back(NeighborRecord {
                destination_hash,
                identity: info.destination.desc.identity,
                last_seen_ms: now_ms,
                app_data: app_data.clone(),
            });
        }

        self.persist_snapshot()?;
        self.push_event(NodeEvent::AnnounceReceived { destination_hash, app_data });
        Ok(())
    }

    fn handle_message(&mut self, packet: &Packet) -> Result<(), MiniNodeError> {
        let wire = WireMessage::unpack(packet.data.as_slice())?;
        let message_id = wire.message_id();
        if self.recent_message_ids.iter().any(|entry| entry == &message_id) {
            return Ok(());
        }

        let verified = self
            .neighbors
            .iter()
            .find(|neighbor| neighbor.destination_hash == wire.source)
            .map(|neighbor| wire.verify(&neighbor.identity).unwrap_or(false))
            .unwrap_or(false);

        let content = wire.payload.content.as_ref().map(|value| value.to_vec()).unwrap_or_default();
        self.remember_message_id(message_id);
        self.persist_snapshot()?;
        self.push_event(NodeEvent::MessageReceived {
            source_hash: wire.source,
            message_id,
            verified,
            content,
        });
        Ok(())
    }

    fn remember_message_id(&mut self, message_id: [u8; 32]) {
        if self.recent_message_ids.len() >= self.config.max_recent_messages {
            self.recent_message_ids.pop_front();
        }
        self.recent_message_ids.push_back(message_id);
    }

    fn restore_snapshot(&mut self, snapshot: NodeSnapshot) {
        self.last_announce_ms = snapshot.last_announce_ms;
        self.latest_position = snapshot.latest_position;

        for neighbor in snapshot.neighbors.into_iter().take(self.config.max_neighbors) {
            self.neighbors.push_back(NeighborRecord {
                destination_hash: neighbor.destination_hash,
                identity: neighbor.identity,
                last_seen_ms: neighbor.last_seen_ms,
                app_data: neighbor.app_data,
            });
        }

        let keep_recent_from =
            snapshot.recent_message_ids.len().saturating_sub(self.config.max_recent_messages);
        for message_id in snapshot.recent_message_ids.into_iter().skip(keep_recent_from) {
            self.recent_message_ids.push_back(message_id);
        }

        for point in snapshot.telemetry.into_iter().take(self.config.max_telemetry_points) {
            self.telemetry.push_back(point);
        }
    }

    fn persist_snapshot(&mut self) -> Result<(), MiniNodeError> {
        let snapshot = NodeSnapshot {
            last_announce_ms: self.last_announce_ms,
            neighbors: self
                .neighbors
                .iter()
                .cloned()
                .map(|neighbor| NeighborSnapshot {
                    destination_hash: neighbor.destination_hash,
                    identity: neighbor.identity,
                    last_seen_ms: neighbor.last_seen_ms,
                    app_data: neighbor.app_data,
                })
                .collect(),
            recent_message_ids: self.recent_message_ids.iter().copied().collect(),
            telemetry: self.telemetry.iter().cloned().collect(),
            latest_position: self.latest_position,
        };
        self.store.save_snapshot(&snapshot)
    }

    fn push_event(&mut self, event: NodeEvent) {
        if self.events.len() >= self.config.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

#[cfg(test)]
mod tests {
    use rand_core::{CryptoRng, RngCore};

    use super::MiniNode;
    use crate::{
        adapters::MemoryLink,
        config::MiniNodeConfig,
        event::NodeEvent,
        store::{MemoryStore, NodeSnapshot},
        telemetry::{BatteryStatus, PositionFix, TelemetryQuery, TelemetrySample},
    };

    #[derive(Clone, Copy)]
    struct TestRng(u64);

    impl RngCore for TestRng {
        fn next_u32(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            (self.0 >> 32) as u32
        }

        fn next_u64(&mut self) -> u64 {
            let hi = u64::from(self.next_u32());
            let lo = u64::from(self.next_u32());
            (hi << 32) | lo
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            rand_core::impls::fill_bytes_via_next(self, dest);
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    impl CryptoRng for TestRng {}

    fn transfer(src: &mut MemoryLink, dst: &mut MemoryLink) {
        for frame in src.drain_outbound() {
            dst.push_inbound(frame);
        }
    }

    #[test]
    fn announce_roundtrip_discovers_neighbor() {
        let mut link_a = MemoryLink::new(512);
        let mut link_b = MemoryLink::new(512);
        let mut node_a =
            MiniNode::load_or_create(TestRng(1), MiniNodeConfig::default(), MemoryStore::new())
                .expect("node a");
        let mut node_b =
            MiniNode::load_or_create(TestRng(2), MiniNodeConfig::default(), MemoryStore::new())
                .expect("node b");

        node_a.tick(0, TestRng(3), &mut link_a).expect("tick a");
        transfer(&mut link_a, &mut link_b);
        node_b.tick(0, TestRng(4), &mut link_b).expect("tick b");

        let neighbors = node_b.neighbors().collect::<Vec<_>>();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].destination_hash, node_a.destination_hash());
    }

    #[test]
    fn direct_message_roundtrip_emits_event() {
        let mut link_a = MemoryLink::new(512);
        let mut link_b = MemoryLink::new(512);
        let mut node_a =
            MiniNode::load_or_create(TestRng(10), MiniNodeConfig::default(), MemoryStore::new())
                .expect("node a");
        let mut node_b =
            MiniNode::load_or_create(TestRng(11), MiniNodeConfig::default(), MemoryStore::new())
                .expect("node b");

        node_a.tick(0, TestRng(12), &mut link_a).expect("announce a");
        transfer(&mut link_a, &mut link_b);
        node_b.tick(0, TestRng(13), &mut link_b).expect("receive announce");

        node_a.send_message(node_b.destination_hash(), b"hello wishmesh").expect("send message");
        node_a.tick(100, TestRng(14), &mut link_a).expect("flush message");
        transfer(&mut link_a, &mut link_b);
        node_b.tick(100, TestRng(15), &mut link_b).expect("receive message");

        let mut received = None;
        while let Some(event) = node_b.poll_event() {
            if let NodeEvent::MessageReceived { content, .. } = event {
                received = Some(content);
            }
        }

        assert_eq!(received.expect("message event"), b"hello wishmesh");
    }

    #[test]
    fn telemetry_query_and_export_work() {
        let mut node =
            MiniNode::load_or_create(TestRng(21), MiniNodeConfig::default(), MemoryStore::new())
                .expect("node");

        node.record_telemetry(TelemetrySample::PositionFix(PositionFix {
            ts_ms: 1_000,
            lat: 44.6500,
            lon: -63.5700,
            alt_m: Some(50.0),
            speed_mps: Some(1.2),
            heading_deg: Some(90.0),
            accuracy_m: Some(4.5),
        }))
        .expect("position");

        node.record_telemetry(TelemetrySample::BatteryStatus(BatteryStatus {
            ts_ms: 2_000,
            pct: 87,
            millivolts: 4012,
            charging: true,
        }))
        .expect("battery");

        let battery = node.query_telemetry(&TelemetryQuery {
            from_ts_ms: None,
            to_ts_ms: None,
            key_prefix: Some("battery".into()),
            limit: Some(4),
        });
        assert_eq!(battery.len(), 3);

        let envelope = node
            .send_message_with_latest_telemetry([0x33; 16], b"status", 8)
            .expect("telemetry message");
        assert_eq!(envelope.destination_hash, [0x33; 16]);
    }

    #[test]
    fn restore_snapshot_keeps_most_recent_message_ids_when_trimmed() {
        let identity = rns_core::identity::PrivateIdentity::new_from_name("restore-trimmed");
        let config = MiniNodeConfig { max_recent_messages: 2, ..MiniNodeConfig::default() };
        let snapshot = NodeSnapshot {
            recent_message_ids: vec![[0x11; 32], [0x22; 32], [0x33; 32]],
            ..NodeSnapshot::default()
        };

        let node = MiniNode::new_with_identity(identity, config, MemoryStore::new(), snapshot);

        assert_eq!(node.recent_message_ids.len(), 2);
        assert_eq!(node.recent_message_ids[0], [0x22; 32]);
        assert_eq!(node.recent_message_ids[1], [0x33; 32]);
    }
}
