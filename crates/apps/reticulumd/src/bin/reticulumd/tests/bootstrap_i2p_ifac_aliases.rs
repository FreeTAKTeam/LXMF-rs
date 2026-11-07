//! Legacy I2P credentials must reach the first native carrier frame literally.
use super::*;

#[tokio::test]
async fn configured_i2p_whitespace_ifac_aliases_authenticate_first_traffic() {
    for (network, key, canonical) in [
        (" ", "", ""),
        ("", " ", ""),
        (" ", " ", ""),
        (" ", " ", "network_name='',pass_phrase='',"),
    ] {
        let root = tempfile::tempdir().expect("state");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("owned SAM listener");
        let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
        let text = format!(
            "interfaces=[{{type='i2p',enabled=true,name='i2p-aliases',sam_host='127.0.0.1',sam_port={},peers=['peer.b32.i2p'],connectable=false,{canonical}ifac_netname='{network}',ifac_netkey='{key}'}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={}}}]\n[reticulum]\nshare_instance=false",
            listener.local_addr().expect("SAM endpoint").port(),
            other.local_addr().expect("other endpoint").port(),
        );
        let (transport, daemon, mut owner) = start(&text, root.path()).await;
        let parent = native_address(&daemon.interface_records()[0]);
        let (mut control, mut peer) = outgoing(&listener).await;
        let address = child(&transport, parent, &[]).await;
        let expected_state = Arc::new(std::sync::RwLock::new(
            InterfaceSharedConfig {
                network_name: (!network.is_empty()).then(|| network.to_string()),
                passphrase: (!key.is_empty()).then(|| key.to_string()),
                ..Default::default()
            }
            .ifac_context_with_default_size(16)
            .expect("literal credential derivation"),
        ));
        let mut ingress = transport.iface_rx();
        let inbound = packet(0xc1);
        let mut negative = Hdlc::frame(&inbound.to_bytes().expect("plain packet")).expect("HDLC");
        negative.extend(frame("wrong-alias-key", &inbound));
        peer.get_mut().write_all(&negative).await.expect("plaintext and wrong-key probes");
        timeout(Duration::from_secs(2), async {
            loop {
                let violations = transport
                    .iface_manager()
                    .lock()
                    .await
                    .traffic_snapshots()
                    .into_iter()
                    .find(|entry| entry.address == address)
                    .expect("native I2P peer")
                    .ifac_violations;
                if violations == 2 {
                    break;
                }
                if let Ok(event) = ingress.try_recv() {
                    assert_ne!(event.packet.destination, inbound.destination, "plaintext admitted");
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("plaintext must be rejected before first authenticated ingress");
        while let Ok(event) = ingress.try_recv() {
            assert_ne!(event.packet.destination, inbound.destination, "negative ingress");
        }
        let authenticated = packet(0xc4);
        let wire =
            encode_packet_ifac(&expected_state, &authenticated).expect("literal IFAC packet");
        assert_eq!(wire.len(), authenticated.to_bytes().expect("plain packet").len() + 16);
        peer.get_mut()
            .write_all(&Hdlc::frame(&wire).expect("HDLC"))
            .await
            .expect("authenticated probe");
        let received = timeout(Duration::from_secs(2), ingress.recv())
            .await
            .expect("authenticated ingress deadline")
            .expect("authenticated ingress");
        assert_eq!(received.address, address);
        assert_eq!(received.packet.destination, authenticated.destination);
        assert_eq!(received.packet.data, authenticated.data);

        let outbound = packet(0xc2);
        let expected_wire = Hdlc::frame(
            &encode_packet_ifac(&expected_state, &outbound).expect("literal outbound IFAC"),
        )
        .expect("HDLC");
        assert_eq!(
            transport
                .iface_manager()
                .lock()
                .await
                .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: outbound })
                .await
                .sent_ifaces,
            1
        );
        let mut actual_wire = vec![0; expected_wire.len()];
        timeout(Duration::from_secs(2), peer.read_exact(&mut actual_wire))
            .await
            .expect("first authenticated egress deadline")
            .expect("first authenticated egress");
        assert_eq!(actual_wire, expected_wire);
        let unrelated = native_address(&daemon.interface_records()[1]);
        let mut unaffected = accept(&other).await;
        verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0xc3).await;
        shutdown(&transport, &mut owner).await;
        closed(&mut peer).await;
        closed(&mut control).await;
        closed(&mut BufReader::new(unaffected)).await;
    }
}
