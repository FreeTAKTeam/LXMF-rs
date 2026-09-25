use std::sync::atomic::{AtomicU64, Ordering};

use crate::iface::{decode_packet_ifac, encode_packet_ifac, IfacState, InterfaceSharedConfig};

fn weave_ifac_state(passphrase: &str) -> IfacState {
    let config = InterfaceSharedConfig {
        network_name: Some("weave-ifac-test".to_string()),
        passphrase: Some(passphrase.to_string()),
        ..InterfaceSharedConfig::default()
    };
    Arc::new(std::sync::RwLock::new(
        config.ifac_context_with_default_size(8).expect("derive Weave IFAC context"),
    ))
}

#[tokio::test]
async fn weave_stream_ifac_rejects_wrong_key_and_authenticates_ingress_egress() {
    let (options, _manager, _parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let endpoint = [0x73_u8; ENDPOINT_ID_LEN];
    let accepted = Packet {
        destination: AddressHash::new([0x31; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated Weave ingress"),
        ..Packet::default()
    };
    let rejected = Packet {
        destination: AddressHash::new([0x32; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong Weave IFAC key"),
        ..Packet::default()
    };
    let outbound = Packet {
        destination: AddressHash::new([0x33; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated Weave egress"),
        ..Packet::default()
    };
    let ifac_state = weave_ifac_state("weave-ifac-credential");
    let wrong_ifac_state = weave_ifac_state("wrong-weave-ifac-credential");
    let violations = Arc::new(AtomicU64::new(0));

    let mut wrong_payload =
        encode_packet_ifac(&wrong_ifac_state, &rejected).expect("encode wrong-key test packet");
    wrong_payload.extend_from_slice(&endpoint);
    let mut accepted_payload =
        encode_packet_ifac(&ifac_state, &accepted).expect("encode authenticated test packet");
    accepted_payload.extend_from_slice(&endpoint);
    let alive = log_frame(local_switch, ET_PROTO_WEAVE_EP_ALIVE, &endpoint);
    let wrong =
        weave_wire_frame(&weave_wdcl_frame(local_switch, WDCL_T_ENDPOINT_PKT, &wrong_payload));
    let valid =
        weave_wire_frame(&weave_wdcl_frame(local_switch, WDCL_T_ENDPOINT_PKT, &accepted_payload));

    let (stream, mut peer) = duplex(8192);
    let (rx_tx, mut rx_rx) = tokio::sync::mpsc::channel(4);
    let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream_with_ifac(
        stream,
        options,
        WeaveStreamControl { cancel: cancel.clone(), iface_stop: CancellationToken::new() },
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
        IfacRuntime::from_parts(ifac_state.clone(), violations.clone()),
    ));

    let mut discovery = vec![0_u8; 256];
    let _ = peer.read(&mut discovery).await.expect("read discovery frame");
    let remote = PrivateIdentity::new_from_name("remote-weave-ifac-test");
    let mut discovery_payload = Vec::new();
    discovery_payload.extend_from_slice(remote.as_identity().verifying_key_bytes());
    discovery_payload.extend_from_slice(&remote.sign(&local_switch).to_bytes());
    peer.write_all(&weave_wire_frame(&weave_wdcl_frame(
        local_switch,
        WDCL_T_DISCOVER,
        &discovery_payload,
    )))
    .await
    .expect("inject authenticated discovery response");
    let mut handshake = vec![0_u8; 256];
    let _ = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut handshake))
        .await
        .expect("read authenticated handshake")
        .expect("handshake frame");
    peer.write_all(&alive).await.expect("inject endpoint alive event");
    peer.write_all(&wrong).await.expect("inject wrong-key packet");
    peer.write_all(&valid).await.expect("inject authenticated packet");
    let received = tokio::time::timeout(Duration::from_secs(1), rx_rx.recv())
        .await
        .expect("authenticated ingress timeout")
        .expect("authenticated ingress message");

    assert_eq!(received.packet.destination, accepted.destination);
    assert_eq!(received.packet.data, accepted.data);
    assert!(received.packet.ifac.is_some());
    assert_eq!(violations.load(Ordering::Relaxed), 1);

    tx_tx
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: outbound.clone() })
        .await
        .expect("queue authenticated egress packet");
    let mut wire = vec![0_u8; 1024];
    let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut wire))
        .await
        .expect("authenticated egress timeout")
        .expect("read egress frame");
    let frame = decode_one_wire_frame(&wire[..n]);
    assert_eq!(frame[4], WDCL_T_CMD);
    assert_eq!(&frame[7..7 + ENDPOINT_ID_LEN], &endpoint);
    let decoded = decode_packet_ifac(&ifac_state, &frame[7 + ENDPOINT_ID_LEN..])
        .expect("decode authenticated egress");
    assert_eq!(decoded.destination, outbound.destination);
    assert_eq!(decoded.data, outbound.data);
    assert!(decoded.ifac.is_some());

    cancel.cancel();
    task.await.expect("Weave stream task");
}
