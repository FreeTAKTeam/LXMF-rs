#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::{
        BleNodeBackendConfig, BroadcastOptions, EmbeddedNode, NodeBackendConfig, NodeConfig,
        NodeError, NodeEventKind, NodeLogLevel, NodeRunState, NodeTransportMode, PollResult,
        SendOptions,
    };
    use crate::{CaptureDefaults, RuntimeConfig};
    use rns_embedded_core::packet::decode_frame;
    use rns_embedded_core::transport::LinkState;

    #[cfg(feature = "std")]
    use std::{thread, time::Duration};

    fn config() -> NodeConfig {
        NodeConfig {
            runtime: RuntimeConfig {
                store_identity: [0x21; 32],
                lxmf_address: [0x42; 16],
                node_mode: NodeTransportMode::BleOnly,
                announce_interval_ms: 1_000,
                max_outbound_queue: 8,
                max_events: 16,
                capture_defaults: CaptureDefaults::default(),
            },
            backend: NodeBackendConfig::Ble(BleNodeBackendConfig::default()),
        }
    }

    #[test]
    fn node_starts_sends_and_exposes_ble_wire() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        assert_eq!(node.get_status().run_state, NodeRunState::Stopped);

        node.start(config()).expect("start");
        node.set_link_state(LinkState::Up).expect("link up");
        let receipt = node.send([0x99; 16], b"hello", SendOptions).expect("send");
        assert_eq!(receipt.epoch, 1);
        assert_eq!(receipt.target_count, 1);

        #[cfg(feature = "std")]
        thread::sleep(Duration::from_millis(60));
        #[cfg(not(feature = "std"))]
        node.tick(0).expect("tick");

        let first = node.take_outbound_wire().expect("take outbound").expect("frame");
        let second = node.take_outbound_wire().expect("take outbound").expect("frame");
        let decoded_first = decode_frame(&first).expect("decode first");
        let decoded_second = decode_frame(&second).expect("decode second");
        assert_eq!(decoded_first.kind, crate::FRAME_KIND_LXMF_MESSAGE);
        assert_eq!(decoded_second.kind, crate::FRAME_KIND_ANNOUNCE);

        let status = node.get_status();
        assert_eq!(status.run_state, NodeRunState::Running);
        assert_eq!(status.epoch, 1);

        assert_eq!(sub.next(0).expect("poll"), PollResult::NodeRestarted { epoch: 1 });
    }

    #[test]
    fn restart_increments_epoch_and_stop_is_idempotent() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        node.start(config()).expect("start");
        assert_eq!(node.get_status().epoch, 1);

        node.restart(config()).expect("restart");
        assert_eq!(node.get_status().epoch, 2);
        assert_eq!(sub.next(0).expect("signal"), PollResult::NodeRestarted { epoch: 1 });
        assert_eq!(sub.next(0).expect("signal"), PollResult::NodeStopped);
        assert_eq!(sub.next(0).expect("signal"), PollResult::NodeRestarted { epoch: 2 });

        node.stop().expect("stop");
        node.stop().expect("stop twice");
        let status = node.get_status();
        assert_eq!(status.run_state, NodeRunState::Stopped);
        assert_eq!(status.epoch, 2);
    }

    #[test]
    fn broadcast_requires_destinations_and_tracks_log_level() {
        let node = EmbeddedNode::new();
        node.start(config()).expect("start");
        node.set_log_level(NodeLogLevel::Debug).expect("log level");

        let err =
            node.broadcast(b"hello", BroadcastOptions::default()).expect_err("empty broadcast");
        assert_eq!(err, NodeError::InvalidConfig);

        let receipt = node
            .broadcast(b"hello", BroadcastOptions { destinations: vec![[0x11; 16], [0x22; 16]] })
            .expect("broadcast");
        assert_eq!(receipt.target_count, 2);
        assert_eq!(node.get_status().log_level, NodeLogLevel::Debug);
        assert_eq!(node.get_status().pending_outbound, 2);
    }

    #[test]
    fn queue_pressure_is_stable_and_broadcast_is_all_or_nothing() {
        let mut small = config();
        small.runtime.max_outbound_queue = 1;

        let node = EmbeddedNode::new();
        node.start(small).expect("start");

        let receipt = node.send([0xAA; 16], b"one", SendOptions).expect("first send");
        assert_eq!(receipt.target_count, 1);
        assert_eq!(node.get_status().pending_outbound, 1);

        let err = node.send([0xBB; 16], b"two", SendOptions).expect_err("queue pressure");
        assert_eq!(err, NodeError::QueuePressure);
        assert_eq!(node.get_status().pending_outbound, 1);

        node.stop().expect("stop");

        let node = EmbeddedNode::new();
        node.start(config()).expect("start");
        let err = node
            .broadcast(b"hello", BroadcastOptions { destinations: vec![[0x11; 16]; 9] })
            .expect_err("broadcast queue pressure");
        assert_eq!(err, NodeError::QueuePressure);
        assert_eq!(node.get_status().pending_outbound, 0);
    }

    #[test]
    fn subscriptions_observe_runtime_events_and_close() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        node.start(config()).expect("start");
        node.set_link_state(LinkState::Up).expect("link up");

        #[cfg(feature = "std")]
        thread::sleep(Duration::from_millis(60));
        #[cfg(not(feature = "std"))]
        node.tick(0).expect("tick");

        assert!(matches!(sub.next(0).expect("restart"), PollResult::NodeRestarted { epoch: 1 }));
        assert!(matches!(
            sub.next(0).expect("event"),
            PollResult::Event(crate::node::NodeEvent {
                kind: NodeEventKind::StatusChanged { .. },
                ..
            })
        ));

        sub.close().expect("close");
        assert_eq!(sub.next(0).expect("closed"), PollResult::Closed);
    }

    #[cfg(feature = "std")]
    #[test]
    fn subscription_timeout_overflow_returns_timeout() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");

        assert_eq!(sub.next(u64::MAX).expect("overflow timeout"), PollResult::Timeout);
    }

    #[cfg(feature = "std")]
    #[test]
    fn managed_mode_rejects_manual_tick() {
        let node = EmbeddedNode::new();
        node.start(config()).expect("start");

        assert_eq!(node.tick(0).expect_err("mode conflict"), NodeError::ModeConflict);
    }

    #[cfg(feature = "std")]
    #[test]
    fn blocked_next_wakes_on_stop() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        node.start(config()).expect("start");
        thread::sleep(Duration::from_millis(60));
        assert!(matches!(sub.next(0).expect("restart"), PollResult::NodeRestarted { .. }));
        while !matches!(sub.next(0).expect("drain"), PollResult::Timeout) {}

        let waiter = sub.clone();
        let handle = thread::spawn(move || waiter.next(5_000).expect("waiter"));
        thread::sleep(Duration::from_millis(50));
        node.stop().expect("stop");

        assert_eq!(handle.join().expect("join"), PollResult::NodeStopped);
    }

    #[cfg(feature = "std")]
    #[test]
    fn blocked_next_wakes_on_restart() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        node.start(config()).expect("start");
        thread::sleep(Duration::from_millis(60));
        assert!(matches!(sub.next(0).expect("restart"), PollResult::NodeRestarted { .. }));
        while !matches!(sub.next(0).expect("drain"), PollResult::Timeout) {}

        let waiter = sub.clone();
        let handle = thread::spawn(move || waiter.next(5_000).expect("waiter"));
        thread::sleep(Duration::from_millis(50));
        node.restart(config()).expect("restart");

        assert_eq!(handle.join().expect("join"), PollResult::NodeStopped);
        assert!(matches!(
            sub.next(0).expect("restart signal"),
            PollResult::NodeRestarted { epoch: 2 }
        ));
    }

    #[cfg(feature = "std")]
    #[test]
    fn closing_subscription_wakes_blocked_waiter() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        node.start(config()).expect("start");
        thread::sleep(Duration::from_millis(60));
        assert!(matches!(sub.next(0).expect("restart"), PollResult::NodeRestarted { .. }));
        while !matches!(sub.next(0).expect("drain"), PollResult::Timeout) {}

        let waiter = sub.clone();
        let handle = thread::spawn(move || waiter.next(5_000).expect("waiter"));
        thread::sleep(Duration::from_millis(50));
        sub.close().expect("close");

        assert_eq!(handle.join().expect("join"), PollResult::Closed);
    }
}
