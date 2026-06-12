#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::{
        CaptureDefaults, EmbeddedNodeRuntime, NodeLifecycleState, NodeTransportMode, RuntimeConfig,
        RuntimeEvent, FRAME_KIND_ANNOUNCE, FRAME_KIND_LXMF_MESSAGE, FRAME_KIND_TEST_PING,
        FRAME_KIND_TEST_PONG,
    };
    use rns_embedded_core::{
        lxmf_min::{decode_envelope, encode_envelope, MinimalEnvelope},
        packet::PacketFrame,
        store::{EmbeddedStore, JournaledEmbeddedStore},
        transport::{FaultInjectingMockTransport, FaultMode, TransportCaps},
        EmbeddedError,
    };

    fn config() -> RuntimeConfig {
        RuntimeConfig {
            store_identity: [0xAB; 32],
            lxmf_address: [0xCD; 16],
            node_mode: NodeTransportMode::BleOnly,
            announce_interval_ms: 1_000,
            max_outbound_queue: 8,
            max_events: 16,
            capture_defaults: CaptureDefaults::default(),
        }
    }

    fn transport() -> FaultInjectingMockTransport {
        FaultInjectingMockTransport::new(TransportCaps { mtu_hint: 1024, ordered_delivery: true })
    }

    #[test]
    fn tick_bootstraps_and_sends_initial_announce() {
        let mut runtime = EmbeddedNodeRuntime::new(config()).expect("runtime");
        let mut store = JournaledEmbeddedStore::new();
        let mut tx = transport();

        runtime.tick(0, &mut tx, &mut store).expect("tick");

        let outbound = tx.drain_outbound();
        assert_eq!(outbound.len(), 1);
        assert_eq!(outbound[0].kind, FRAME_KIND_ANNOUNCE);
        assert_eq!(outbound[0].payload, vec![0xAB; 32]);

        let events = runtime.drain_events();
        assert!(events.contains(&RuntimeEvent::Bootstrapped { replay_floor: 0 }));
        assert!(events.contains(&RuntimeEvent::AnnounceQueued { sequence: 1 }));
        assert!(events.contains(&RuntimeEvent::FrameSent {
            kind: FRAME_KIND_ANNOUNCE,
            sequence: 1,
            bytes: 32,
        }));
    }

    #[test]
    fn queued_message_encodes_minimal_lxmf_envelope() {
        let mut runtime = EmbeddedNodeRuntime::new(config()).expect("runtime");
        let seq = runtime.queue_message([0xEF; 16], b"hello from esp").expect("queue message");
        assert_eq!(seq, 1);

        let mut store = JournaledEmbeddedStore::new();
        let mut tx = transport();
        runtime.tick(0, &mut tx, &mut store).expect("tick");

        let outbound = tx.drain_outbound();
        assert_eq!(outbound.len(), 2);
        assert_eq!(outbound[0].kind, FRAME_KIND_LXMF_MESSAGE);
        assert_eq!(outbound[1].kind, FRAME_KIND_ANNOUNCE);

        let envelope = decode_envelope(&outbound[0].payload).expect("decode envelope");
        assert_eq!(envelope.source, [0xCD; 16]);
        assert_eq!(envelope.destination, [0xEF; 16]);
        assert_eq!(envelope.sequence, 1);
        assert_eq!(envelope.body, b"hello from esp".to_vec());
    }

    #[test]
    fn inbound_replay_rejection_updates_store_once() {
        let mut runtime = EmbeddedNodeRuntime::new(config()).expect("runtime");
        let mut store = JournaledEmbeddedStore::new();
        let mut tx = transport();

        let inbound = PacketFrame::new(0x44, 7, b"status".to_vec()).expect("frame");
        tx.enqueue_inbound([inbound.clone()]);
        runtime.tick(0, &mut tx, &mut store).expect("tick");
        assert_eq!(store.load_replay_floor(&config().store_identity).expect("load replay"), 7);

        tx.enqueue_inbound([inbound]);
        runtime.tick(1, &mut tx, &mut store).expect("tick duplicate");

        let events = runtime.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            RuntimeEvent::FrameReceived { kind: 0x44, sequence: 7, bytes: 6 }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            RuntimeEvent::FrameRejected {
                kind: 0x44,
                sequence: 7,
                error: EmbeddedError::ReplayRejected,
            }
        )));
    }

    #[test]
    fn backpressure_keeps_message_queued_for_later_tick() {
        let mut runtime = EmbeddedNodeRuntime::new(config()).expect("runtime");
        runtime.queue_message([0xEF; 16], b"retry me").expect("queue message");

        let mut store = JournaledEmbeddedStore::new();
        let mut tx = transport().with_faults(vec![FaultMode::BackpressureEvery(1)]);

        runtime.tick(0, &mut tx, &mut store).expect("tick");
        assert_eq!(runtime.pending_outbound_len(), 2);

        let mut healthy_tx = transport();
        runtime.tick(1_000, &mut healthy_tx, &mut store).expect("retry tick");
        assert_eq!(runtime.pending_outbound_len(), 0);

        let stats = runtime.stats();
        assert_eq!(stats.outbound_deferred, 1);
        assert_eq!(stats.outbound_sent, 2);
    }

    #[test]
    fn inbound_test_ping_enqueues_test_pong_response() {
        let mut runtime = EmbeddedNodeRuntime::new(config()).expect("runtime");
        let mut store = JournaledEmbeddedStore::new();
        let mut transport = transport();
        transport.enqueue_inbound([
            PacketFrame::new(FRAME_KIND_TEST_PING, 7, b"ping".to_vec()).expect("ping frame")
        ]);

        runtime.tick(0, &mut transport, &mut store).expect("tick");

        let outbound = transport.drain_outbound();
        assert_eq!(outbound.len(), 2);
        assert_eq!(outbound[0].kind, FRAME_KIND_ANNOUNCE);
        assert_eq!(outbound[1].kind, FRAME_KIND_TEST_PONG);
        assert_eq!(outbound[1].payload, b"pong:ping");
    }

    #[test]
    fn inbound_lxmf_message_enqueues_lxmf_pong_response() {
        let mut runtime = EmbeddedNodeRuntime::new(config()).expect("runtime");
        let mut store = JournaledEmbeddedStore::new();
        let mut transport = transport();
        let inbound_envelope = MinimalEnvelope {
            source: [0xEE; 16],
            destination: [0xCD; 16],
            sequence: 77,
            body: b"hello".to_vec(),
        };
        let inbound_payload = encode_envelope(&inbound_envelope).expect("encode inbound envelope");
        transport.enqueue_inbound([
            PacketFrame::new(FRAME_KIND_LXMF_MESSAGE, 8, inbound_payload).expect("lxmf frame")
        ]);

        runtime.tick(0, &mut transport, &mut store).expect("tick");

        let outbound = transport.drain_outbound();
        assert_eq!(outbound.len(), 2);
        assert_eq!(outbound[0].kind, FRAME_KIND_ANNOUNCE);
        assert_eq!(outbound[1].kind, FRAME_KIND_LXMF_MESSAGE);
        let response = decode_envelope(&outbound[1].payload).expect("decode response envelope");
        assert_eq!(response.source, [0xCD; 16]);
        assert_eq!(response.destination, [0xEE; 16]);
        assert_eq!(response.body, b"pong:hello");
    }

    #[test]
    fn lifecycle_moves_to_tcp_online_when_provisioned_and_link_up() {
        let mut runtime = EmbeddedNodeRuntime::new(RuntimeConfig {
            node_mode: NodeTransportMode::TcpClient,
            ..config()
        })
        .expect("runtime");
        let mut store = JournaledEmbeddedStore::new();
        let mut tx = transport();
        runtime.set_network_provisioned(true);

        runtime.tick(0, &mut tx, &mut store).expect("tick");

        assert_eq!(runtime.lifecycle_state(), NodeLifecycleState::TcpOnline);
    }

    #[test]
    fn lifecycle_enters_ble_recovery_when_enabled() {
        let mut runtime = EmbeddedNodeRuntime::new(RuntimeConfig {
            node_mode: NodeTransportMode::TcpClient,
            ..config()
        })
        .expect("runtime");
        let mut store = JournaledEmbeddedStore::new();
        let mut tx = transport();
        runtime.set_network_provisioned(true);
        runtime.set_ble_recovery_active(true);

        runtime.tick(0, &mut tx, &mut store).expect("tick");

        assert_eq!(runtime.lifecycle_state(), NodeLifecycleState::BleRecovery);
    }
}
