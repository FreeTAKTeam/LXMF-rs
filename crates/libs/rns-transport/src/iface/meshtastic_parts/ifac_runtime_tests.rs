use super::*;
use crate::iface::{IfacState, InterfaceSharedConfig, TxMessage, TxMessageType};
use crate::packet::{Packet, PacketDataBuffer};
use std::sync::{atomic::Ordering, Arc};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

fn state_for(config: InterfaceSharedConfig) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        config.ifac_context_with_default_size(8).expect("derive IFAC context"),
    ))
}

fn tunneled_packet(
    config: MeshtasticInterfaceConfig,
    state: &IfacState,
    packet: &Packet,
    from: u32,
) -> Vec<MeshtasticReceivedFrame> {
    let wire = encode_packet_ifac(state, packet).expect("encode IFAC packet");
    let mut tunnel = MeshtasticTunnel::new(config);
    tunnel.queue_outgoing_packet(&wire).expect("queue tunneled packet");
    let mut frames = Vec::new();
    while let Some(frame) = tunnel.next_transmit() {
        frames.push(MeshtasticReceivedFrame::new(from, &frame.payload));
    }
    frames
}

#[tokio::test]
async fn meshtastic_tunnel_rejects_wrong_ifac_and_authenticates_ingress_egress() {
    let config = MeshtasticInterfaceConfig {
        max_payload_bytes: 64,
        send_delay: std::time::Duration::from_millis(5),
        ..MeshtasticInterfaceConfig::default()
    };
    let mut manager = InterfaceManager::new(8);
    let interface = MeshtasticInterface::new("in-memory Meshtastic", config.clone());
    let handle = interface.handle();
    let status = interface.runtime_status_handle();
    let context = manager.new_context(interface);
    let address = *context.channel.address();
    let receiver = manager.receiver();
    let ifac_violations = context.channel.ifac_violations.clone();
    assert!(manager.set_shared_config(
        address,
        InterfaceSharedConfig {
            network_name: Some("meshtastic-ifac-test".to_string()),
            passphrase: Some("meshtastic-ifac-secret".to_string()),
            ..InterfaceSharedConfig::default()
        }
    ));
    let ifac_state = context.channel.ifac_state.clone();
    let worker = tokio::spawn(MeshtasticInterface::spawn(context));

    let wrong_state = state_for(InterfaceSharedConfig {
        network_name: Some("meshtastic-ifac-test".to_string()),
        passphrase: Some("incorrect-meshtastic-secret".to_string()),
        ..InterfaceSharedConfig::default()
    });
    let rejected = Packet {
        destination: AddressHash::new_from_slice(&[0x71; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong Meshtastic IFAC key"),
        ..Packet::default()
    };
    for frame in tunneled_packet(config.clone(), &wrong_state, &rejected, 41) {
        handle.inject_received(frame).await.expect("inject wrong-key tunnel frame");
    }
    timeout(std::time::Duration::from_secs(1), async {
        while ifac_violations.load(Ordering::Relaxed) == 0 {
            sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("wrong IFAC key is recorded");
    let rejected_status = timeout(std::time::Duration::from_secs(1), async {
        loop {
            let snapshot = status.snapshot();
            if snapshot.decode_errors > 0 {
                return snapshot;
            }
            sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("wrong IFAC key updates interface error evidence");
    assert!(rejected_status.last_error.is_some());
    assert!(receiver.lock().await.try_recv().is_err(), "unauthenticated packet is not admitted");

    let inbound = Packet {
        destination: AddressHash::new_from_slice(&[0x72; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated Meshtastic ingress"),
        ..Packet::default()
    };
    for frame in tunneled_packet(config.clone(), &ifac_state, &inbound, 42) {
        handle.inject_received(frame).await.expect("inject authenticated tunnel frame");
    }
    let admitted = timeout(std::time::Duration::from_secs(1), async {
        loop {
            match receiver.lock().await.try_recv() {
                Ok(message) => return message,
                Err(mpsc::error::TryRecvError::Empty) => {
                    sleep(std::time::Duration::from_millis(5)).await;
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    panic!("Meshtastic transport receive queue disconnected");
                }
            }
        }
    })
    .await
    .expect("authenticated packet is admitted");
    assert_eq!(admitted.address, address);
    assert_eq!(admitted.packet.destination, inbound.destination);
    assert_eq!(admitted.packet.data.as_slice(), b"authenticated Meshtastic ingress");
    assert_eq!(admitted.packet.ifac.map(|ifac| ifac.length), Some(0));

    let outbound = Packet {
        destination: AddressHash::new_from_slice(&[0x73; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated Meshtastic egress"),
        ..Packet::default()
    };
    let trace = manager
        .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: outbound.clone() })
        .await;
    assert_eq!(trace.sent_ifaces, 1);
    let mut received_tunnel = MeshtasticTunnel::new(config);
    let decoded = timeout(std::time::Duration::from_secs(1), async {
        loop {
            let Some(frame) = handle.recv_transmit().await else {
                panic!("Meshtastic transmit queue closed");
            };
            if let Some(wire) = received_tunnel
                .process_received(MeshtasticReceivedFrame::new(99, &frame.payload))
                .expect("reassemble transmitted Meshtastic tunnel frame")
            {
                break decode_packet_ifac(&ifac_state, &wire).expect("authenticate egress packet");
            }
        }
    })
    .await
    .expect("Meshtastic authenticated egress timed out");
    assert_eq!(decoded.destination, outbound.destination);
    assert_eq!(decoded.data.as_slice(), b"authenticated Meshtastic egress");
    assert_eq!(ifac_violations.load(Ordering::Relaxed), 1);

    worker.abort();
    let _ = worker.await;
}

#[tokio::test]
async fn meshtastic_ifac_worker_stops_cleanly_without_clearing_authentication() {
    let config = MeshtasticInterfaceConfig {
        send_delay: std::time::Duration::from_millis(5),
        ..MeshtasticInterfaceConfig::default()
    };
    let interface = MeshtasticInterface::new("in-memory Meshtastic stop", config);
    let mut manager = InterfaceManager::new(4);
    let context = manager.new_context(interface);
    let stop = context.channel.stop.clone();
    let cancel = context.cancel.clone();
    let ifac_state = context.channel.ifac_state.clone();
    assert!(manager.set_shared_config(
        *context.channel.address(),
        InterfaceSharedConfig {
            network_name: Some("meshtastic-stop-test".to_string()),
            passphrase: Some("meshtastic-stop-secret".to_string()),
            ..InterfaceSharedConfig::default()
        }
    ));
    let probe = Packet {
        destination: AddressHash::new_from_slice(&[0x74; 16]),
        data: PacketDataBuffer::new_from_slice(b"IFAC remains required after worker stop"),
        ..Packet::default()
    };
    let authenticated_before_stop =
        encode_packet_ifac(&ifac_state, &probe).expect("configured IFAC state");

    let worker = tokio::spawn(MeshtasticInterface::spawn(context));
    tokio::task::yield_now().await;
    cancel.cancel();
    timeout(std::time::Duration::from_secs(1), worker)
        .await
        .expect("Meshtastic IFAC worker stops promptly")
        .expect("Meshtastic IFAC worker exits normally");

    assert!(stop.is_cancelled(), "worker publishes its stopped state");
    assert_eq!(
        encode_packet_ifac(&ifac_state, &probe).expect("IFAC remains configured after stop"),
        authenticated_before_stop
    );
}
