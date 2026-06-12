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
