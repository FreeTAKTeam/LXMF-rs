use super::*;

#[derive(Clone, Copy)]
enum Stall {
    Destination,
    Session,
    Accept,
    Outgoing,
}

async fn stalled_shutdown(stall: Stall) {
    let root = tempfile::tempdir().expect("state");
    let reserved = reserve();
    let port = reserved.local_addr().expect("SAM endpoint").port();
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let incoming = !matches!(stall, Stall::Outgoing);
    let text =
        config(port, other.local_addr().expect("other endpoint").port(), incoming, INITIAL, 512);
    // The unavailable endpoint lets bootstrap's separate preparation return first.
    let (transport, daemon, mut owner) = start(&text, root.path()).await;
    let parent = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    published(&daemon, |s| {
        if incoming {
            s["accept_state"] == "reconnecting"
        } else {
            outgoing_state(s, "reconnecting")
        }
    })
    .await;
    let listener = reserved.listen(8).expect("activate SAM only after bootstrap returns");
    let mut sockets = Vec::new();
    match stall {
        Stall::Outgoing => {
            let mut peer = BufReader::new(accept(&listener).await);
            assert_eq!(line(&mut peer).await, "HELLO VERSION MIN=3.0 MAX=3.3");
            sockets.push(peer);
            let address = child(&transport, parent, &[]).await;
            let admitted = timeout(Duration::from_secs(3), async {
                for admitted in 0..512 {
                    let trace = transport
                        .iface_manager()
                        .lock()
                        .await
                        .send(TxMessage {
                            tx_type: TxMessageType::Direct(address),
                            packet: packet(0x94),
                        })
                        .await;
                    if trace.sent_ifaces == 0 {
                        return admitted;
                    }
                }
                panic!("native queues never reached their bounded full state");
            })
            .await
            .expect("native queue-saturation barrier");
            assert!(admitted >= 129, "fill the existing 128-entry peer queue before root pressure");
        }
        Stall::Destination => {
            let mut peer = hello(&listener).await;
            assert_eq!(line(&mut peer).await, "DEST GENERATE SIGNATURE_TYPE=7");
            sockets.push(peer);
        }
        Stall::Session => {
            generate(&listener, false).await;
            sockets.push(session_request(&listener, true).await.0);
        }
        Stall::Accept => {
            generate(&listener, false).await;
            let (control, id) = session(&listener, true).await;
            sockets.push(control);
            sockets.push(accept_request(&listener, &id).await);
        }
    }
    assert_eq!(native_address(&daemon.interface_records()[0]), parent);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x97).await;
    shutdown(&transport, &mut owner).await;
    for peer in &mut sockets {
        closed(peer).await;
    }
    closed(&mut BufReader::new(unaffected)).await;
}

#[tokio::test]
async fn configured_i2p_shutdown_joins_stalled_native_destination_generation() {
    stalled_shutdown(Stall::Destination).await;
}

#[tokio::test]
async fn configured_i2p_shutdown_joins_stalled_incoming_session_create() {
    stalled_shutdown(Stall::Session).await;
}

#[tokio::test]
async fn configured_i2p_shutdown_joins_pending_incoming_stream_accept() {
    stalled_shutdown(Stall::Accept).await;
}

#[tokio::test]
async fn configured_i2p_shutdown_joins_outgoing_setup_under_native_queue_pressure() {
    stalled_shutdown(Stall::Outgoing).await;
}
