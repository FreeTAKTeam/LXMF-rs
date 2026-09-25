use std::sync::atomic::{AtomicU64, Ordering};

use crate::iface::{decode_packet_ifac, encode_packet_ifac, IfacRuntime, InterfaceSharedConfig};

fn rnode_multi_ifac_state(passphrase: &str) -> IfacState {
    let config = InterfaceSharedConfig {
        network_name: Some("rnode-multi-ifac-test".to_string()),
        passphrase: Some(passphrase.to_string()),
        ..InterfaceSharedConfig::default()
    };
    Arc::new(std::sync::RwLock::new(
        config
            .ifac_context_with_default_size(8)
            .expect("derive RNodeMulti IFAC context"),
    ))
}

#[tokio::test]
async fn rnode_multi_kiss_vport_rejects_wrong_ifac_and_authenticates_ingress_egress() {
    let child = AddressHash::new([0x62; 16]);
    let vport = 2;
    let (stream, mut peer) = duplex(8192);
    let (rx_tx, mut rx_rx) = tokio::sync::mpsc::channel(4);
    let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let options = test_options(child, vport);
    let ifac_state = rnode_multi_ifac_state("rnode-multi-ifac-secret");
    let wrong_ifac_state = rnode_multi_ifac_state("incorrect-rnode-multi-ifac-secret");
    let violations = Arc::new(AtomicU64::new(0));
    let task = tokio::spawn(run_rnode_multi_stream_with_ifac(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        IfacRuntime::from_parts(ifac_state.clone(), violations.clone()),
    ));

    let rejected = Packet {
        destination: AddressHash::new([0x63; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong RNodeMulti IFAC key"),
        ..Packet::default()
    };
    let wrong_payload = encode_packet_ifac(&wrong_ifac_state, &rejected)
        .expect("encode wrong-key RNodeMulti packet");
    peer.write_all(&encode_command_frame(0x20, &wrong_payload))
        .await
        .expect("inject wrong-key packet on KISS vport 2");
    tokio::time::timeout(Duration::from_secs(1), async {
        while violations.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("wrong IFAC key is recorded");
    assert!(rx_rx.try_recv().is_err(), "unauthenticated packet is not admitted");

    let inbound = valid_test_packet();
    let inbound_payload =
        encode_packet_ifac(&ifac_state, &inbound).expect("encode authenticated ingress");
    peer.write_all(&encode_command_frame(0x20, &inbound_payload))
        .await
        .expect("inject authenticated packet on KISS vport 2");
    let received = tokio::time::timeout(Duration::from_secs(1), rx_rx.recv())
        .await
        .expect("authenticated ingress timeout")
        .expect("authenticated ingress message");
    assert_eq!(received.address, child);
    assert_eq!(received.packet.destination, inbound.destination);
    assert_eq!(received.packet.data, inbound.data);
    assert!(received.packet.ifac.is_some());
    assert_eq!(violations.load(Ordering::Relaxed), 1);

    let outbound = Packet {
        destination: AddressHash::new([0x64; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated RNodeMulti egress"),
        ..Packet::default()
    };
    tx_tx
        .send(TxMessage { tx_type: TxMessageType::Direct(child), packet: outbound.clone() })
        .await
        .expect("queue authenticated child egress");
    let mut wire = vec![0_u8; 1024];
    let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut wire))
        .await
        .expect("authenticated egress timeout")
        .expect("read egress KISS frames");
    let frames = decode_frames(&wire[..n], 1024).expect("decode egress KISS frames");
    assert_eq!(
        frames.first(),
        Some(&KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![vport])))
    );
    let payload = frames
        .iter()
        .find_map(|frame| match frame {
            KissFrame::Data(payload) => Some(payload),
            _ => None,
        })
        .expect("RNodeMulti egress data frame");
    let decoded = decode_packet_ifac(&ifac_state, payload).expect("decode authenticated egress");
    assert_eq!(decoded.destination, outbound.destination);
    assert_eq!(decoded.data, outbound.data);
    assert!(decoded.ifac.is_some());

    cancel.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(1), task)
        .await
        .expect("RNodeMulti stream should stop after cancellation")
        .expect("RNodeMulti stream task should not panic");
}
