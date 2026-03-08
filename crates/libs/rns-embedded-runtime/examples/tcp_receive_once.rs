use std::{
    env, process,
    time::{Duration, Instant},
};

use rns_embedded_runtime::{
    node::NODE_EXTENSION_ID_RECEIVED_SUMMARY, EmbeddedNode, NodeBackendConfig, NodeConfig,
    NodeEventKind, NodeTransportMode, PollResult, RuntimeConfig, TcpServerConfig,
};

const SERVER_LXMF_ADDRESS: [u8; 16] = [0x44; 16];
const SERVER_STORE_IDENTITY: [u8; 32] = [0x55; 32];

fn main() {
    let Some(port) = env::args().nth(1).and_then(|value| value.parse::<u16>().ok()) else {
        eprintln!("usage: cargo run -p rns-embedded-runtime --example tcp_receive_once -- <port>");
        process::exit(2);
    };

    println!("LISTENING port={port} destination={}", encode_hex(&SERVER_LXMF_ADDRESS));

    let node = EmbeddedNode::new();
    let subscription = node.subscribe_events().expect("subscribe");
    node.start(NodeConfig {
        runtime: RuntimeConfig {
            store_identity: SERVER_STORE_IDENTITY,
            lxmf_address: SERVER_LXMF_ADDRESS,
            node_mode: NodeTransportMode::TcpServer,
            announce_interval_ms: 1_000,
            max_outbound_queue: 8,
            max_events: 32,
            capture_defaults: Default::default(),
        },
        backend: NodeBackendConfig::TcpServer(TcpServerConfig { listen_port: port }),
    })
    .expect("start tcp server");
    node.set_network_provisioned(true).expect("set network provisioned");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match subscription.next(500).expect("poll") {
            PollResult::Event(event) => match event.kind {
                NodeEventKind::Extension {
                    extension_id: NODE_EXTENSION_ID_RECEIVED_SUMMARY,
                    value0,
                    value1,
                } => {
                    println!("RECEIVED sequence={value0} bytes={value1}");
                    break;
                }
                NodeEventKind::Error { error, .. } => {
                    eprintln!("ERROR {error:?}");
                    process::exit(1);
                }
                _ => {}
            },
            PollResult::NodeRestarted { .. } | PollResult::NodeStopped => continue,
            PollResult::Timeout if Instant::now() < deadline => continue,
            PollResult::Timeout => {
                eprintln!("timeout waiting for inbound message");
                process::exit(1);
            }
            other => {
                eprintln!("unexpected poll result: {other:?}");
            }
        }
    }

    node.stop().expect("stop");
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}
