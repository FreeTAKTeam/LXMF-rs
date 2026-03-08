use rns_embedded_core::{store::JournaledEmbeddedStore, transport::LinkState};
use rns_embedded_runtime::{
    ble::{BleShimConfig, BleShimTransport},
    EmbeddedNodeRuntime, RuntimeConfig,
};

fn main() {
    let mut runtime = EmbeddedNodeRuntime::new(RuntimeConfig::default()).expect("runtime");
    let mut store = JournaledEmbeddedStore::new();
    let mut transport = BleShimTransport::new(BleShimConfig::default()).expect("transport");

    transport.set_link_state(LinkState::Up);
    runtime.tick(0, &mut transport, &mut store).expect("tick");
    runtime.queue_message([0x44; 16], b"hello from manual tick").expect("queue message");
    runtime.tick(1_000, &mut transport, &mut store).expect("tick");

    while let Some(frame) = transport.take_outbound_wire() {
        println!("outbound frame bytes={}", frame.len());
    }
}
