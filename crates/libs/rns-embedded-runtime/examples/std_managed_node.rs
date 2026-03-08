use std::{thread, time::Duration};

use rns_embedded_core::transport::LinkState;
use rns_embedded_runtime::{
    BleNodeBackendConfig, EmbeddedNode, NodeBackendConfig, NodeConfig, NodeEventKind,
    NodeTransportMode, PollResult, RuntimeConfig, SendOptions,
};

fn main() {
    let node = EmbeddedNode::new();
    let subscription = node.subscribe_events().expect("subscribe");

    let config = NodeConfig {
        runtime: RuntimeConfig {
            store_identity: [0x11; 32],
            lxmf_address: [0x22; 16],
            node_mode: NodeTransportMode::BleOnly,
            announce_interval_ms: 1_000,
            max_outbound_queue: 8,
            max_events: 16,
            capture_defaults: Default::default(),
        },
        backend: NodeBackendConfig::Ble(BleNodeBackendConfig::default()),
    };

    node.start(config).expect("start");
    node.set_link_state(LinkState::Up).expect("link up");
    node.send([0x42; 16], b"hello from managed std", SendOptions).expect("send");

    thread::sleep(Duration::from_millis(75));

    loop {
        match subscription.next(0).expect("poll") {
            PollResult::Event(event) => {
                if let NodeEventKind::PacketSent { sequence, .. } = event.kind {
                    println!("packet sent with sequence {sequence}");
                    break;
                }
            }
            PollResult::Timeout => break,
            other => println!("poll result: {other:?}"),
        }
    }

    node.stop().expect("stop");
}
