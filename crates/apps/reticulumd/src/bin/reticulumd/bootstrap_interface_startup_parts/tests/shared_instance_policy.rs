async fn start_shared_config(
    transport: &Transport,
    iface: InterfaceConfig,
) -> (InterfaceRecord, super::InterfaceStartupBatch) {
    let mut config = DaemonConfig::from_toml("").expect("empty daemon configuration");
    config.interfaces = vec![iface];
    let iface = &config.interfaces[0];
    let mut records = vec![InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: true,
        host: iface.host.clone(),
        port: iface.port,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    }];
    let batch = startup_configured_interfaces(
        &test_args(),
        &config,
        &super::TcpServerSelection::default(),
        transport,
        &transport.iface_manager(),
        None,
        &mut records,
        std::path::Path::new("."),
        None,
        None,
    )
    .await;
    assert!(batch.startup_failures.is_empty(), "{:?}", batch.startup_failures);
    assert_eq!(batch.startup_successes, 1);
    (records.remove(0), batch)
}

fn shared_test_config(kind: &str, synthetic: bool) -> InterfaceConfig {
    InterfaceConfig {
        kind: kind.to_string(),
        name: Some("configured-shared".to_string()),
        enabled: Some(true),
        synthetic_shared_instance: synthetic,
        network_name: Some("configured-shared-network".to_string()),
        passphrase: Some("configured-shared-original".to_string()),
        ..Default::default()
    }
}

fn shared_test_policy(secret: &str) -> InterfaceSharedConfig {
    InterfaceSharedConfig {
        network_name: Some("configured-shared-network".to_string()),
        passphrase: Some(secret.to_string()),
        ..Default::default()
    }
}

async fn shared_first_ingress<S: tokio::io::AsyncWrite + Unpin>(
    peer: &mut S,
    ingress: &mut tokio::sync::broadcast::Receiver<rns_transport::iface::RxMessage>,
    plaintext: bool,
    secret: &str,
    payload: &[u8],
) {
    use rns_transport::iface::hdlc::Hdlc;
    use rns_transport::iface::{encode_ifac, IfacState};
    use rns_transport::packet::{Packet, PacketDataBuffer};
    let packet = Packet { data: PacketDataBuffer::new_from_slice(payload), ..Default::default() };
    let raw = packet.to_bytes().expect("first packet bytes");
    let state: IfacState = std::sync::Arc::new(std::sync::RwLock::new(
        shared_test_policy(secret).ifac_context().expect("IFAC"),
    ));
    // Send a negative control first. The following accepted frame is the stream barrier.
    let protected = encode_ifac(&state, &raw).expect("authenticated frame");
    let (rejected, accepted) = if plaintext { (protected, raw) } else { (raw, protected) };
    peer.write_all(&Hdlc::frame(&rejected).expect("negative HDLC")).await.expect("negative frame");
    peer.write_all(&Hdlc::frame(&accepted).expect("positive HDLC"))
        .await
        .expect("first accepted frame");
    let received = tokio::time::timeout(Duration::from_secs(2), ingress.recv())
        .await
        .expect("first ingress deadline")
        .expect("first ingress");
    assert_eq!(received.packet.data.as_slice(), payload);
    assert_eq!(received.packet.ifac.is_some(), !plaintext);
    assert!(ingress.try_recv().is_err(), "negative frame never reaches transport");
}

async fn stop_shared_config(transport: &Transport, root: AddressHash) {
    let manager = transport.iface_manager();
    let workers = {
        let mut manager = manager.lock().await;
        let workers = manager.take_worker_tree(root);
        assert!(manager.stop_interface(root));
        workers
    };
    for worker in workers {
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .expect("configured worker joins")
            .expect("worker");
    }
}

#[tokio::test]
async fn configured_native_tcp_sidecar_keeps_first_and_new_child_plaintext_after_rotation() {
    let probe = TcpListener::bind("127.0.0.1:0").await.expect("address reservation");
    let address = probe.local_addr().expect("address");
    drop(probe);
    let transport = Transport::new(TransportConfig::default());
    let mut iface = shared_test_config("local", true);
    iface.host = Some("127.0.0.1".to_string());
    iface.port = Some(address.port());
    let (record, _) = start_shared_config(&transport, iface).await;
    assert_eq!(record.settings.as_ref().expect("settings")["_runtime"]["startup_status"], "active");
    let root = *transport.interface_hashes().await.iter().next().expect("configured root");
    let mut ingress = transport.iface_rx();
    let mut first = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match tokio::net::TcpStream::connect(address).await {
                Ok(peer) => break peer,
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                    tokio::task::yield_now().await
                }
                Err(error) => panic!("sidecar connection failed: {error}"),
            }
        }
    })
    .await
    .expect("listener readiness");
    shared_first_ingress(
        &mut first,
        &mut ingress,
        true,
        "configured-shared-original",
        b"native sidecar first frame",
    )
    .await;
    assert!(transport
        .iface_manager()
        .lock()
        .await
        .try_set_shared_config(root, shared_test_policy("rotated"))
        .expect("parent rotation"));
    shared_first_ingress(
        &mut first,
        &mut ingress,
        true,
        "rotated",
        b"existing native sidecar after rotation",
    )
    .await;
    let mut second = tokio::net::TcpStream::connect(address).await.expect("new native client");
    shared_first_ingress(
        &mut second,
        &mut ingress,
        true,
        "rotated",
        b"new native sidecar after rotation",
    )
    .await;
    stop_shared_config(&transport, root).await;
}

#[cfg(unix)]
async fn configured_unix_server_case(synthetic: bool) {
    let temp = tempfile::tempdir().expect("socket directory");
    let path = temp.path().join("configured.sock");
    let transport = Transport::new(TransportConfig::default());
    let mut iface = shared_test_config("local", synthetic);
    iface.shared_instance_type = Some("unix".to_string());
    iface.socket_path = Some(path.to_string_lossy().into_owned());
    let (_, _) = start_shared_config(&transport, iface).await;
    let root = *transport.interface_hashes().await.iter().next().expect("configured root");
    tokio::time::timeout(Duration::from_secs(2), async {
        while !path.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Unix listener readiness");
    let mut ingress = transport.iface_rx();
    let mut first =
        tokio::net::UnixStream::connect(&path).await.expect("first configured Unix client");
    shared_first_ingress(
        &mut first,
        &mut ingress,
        synthetic,
        "configured-shared-original",
        b"configured Unix first frame",
    )
    .await;
    assert!(transport
        .iface_manager()
        .lock()
        .await
        .try_set_shared_config(root, shared_test_policy("rotated"))
        .expect("parent rotation"));
    shared_first_ingress(
        &mut first,
        &mut ingress,
        synthetic,
        "rotated",
        b"configured Unix existing child rotation",
    )
    .await;
    let mut second =
        tokio::net::UnixStream::connect(&path).await.expect("new configured Unix client");
    shared_first_ingress(
        &mut second,
        &mut ingress,
        synthetic,
        "rotated",
        b"configured Unix new child rotation",
    )
    .await;
    stop_shared_config(&transport, root).await;
    assert!(!path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn configured_native_unix_server_keeps_first_and_new_child_plaintext_after_rotation() {
    configured_unix_server_case(true).await;
}

#[cfg(unix)]
#[tokio::test]
async fn configured_explicit_unix_server_enforces_first_and_new_child_ifac_after_rotation() {
    configured_unix_server_case(false).await;
}

#[tokio::test]
async fn configured_tcp_attachment_commits_shared_mode_before_first_ingress() {
    use rns_transport::packet::Packet;
    for synthetic in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("owned shared endpoint");
        let address = listener.local_addr().expect("address");
        let mut config = TransportConfig::default();
        config.set_local_hops_delta(1);
        let transport = Transport::new(config);
        let manager = transport.iface_manager();
        let ordinary = *manager.lock().await.new_channel(8).address();
        assert!(transport.should_apply_delta(&Packet::default(), &ordinary).await);
        let mut iface = shared_test_config("local_client", synthetic);
        iface.host = Some("127.0.0.1".to_string());
        iface.port = Some(address.port());
        let mut ingress = transport.iface_rx();
        let (record, batch) = start_shared_config(&transport, iface).await;
        assert_eq!(
            record.settings.as_ref().expect("settings")["_runtime"]["startup_status"],
            "attached"
        );
        let root = batch.tunnel_synth_ifaces[0];
        assert!(
            !transport.should_apply_delta(&Packet::default(), &ordinary).await,
            "activation commits global attached mode"
        );
        let (mut peer, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("attachment deadline")
            .expect("attachment");
        shared_first_ingress(
            &mut peer,
            &mut ingress,
            synthetic,
            "configured-shared-original",
            b"configured TCP attached first frame",
        )
        .await;
        stop_shared_config(&transport, root).await;
        assert!(manager.lock().await.stop_interface(ordinary));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn configured_unix_attachment_commits_shared_mode_before_first_ingress() {
    use rns_transport::packet::Packet;
    for synthetic in [false, true] {
        let temp = tempfile::tempdir().expect("socket directory");
        let path = temp.path().join("owner.sock");
        let listener = tokio::net::UnixListener::bind(&path).expect("owned shared endpoint");
        let mut config = TransportConfig::default();
        config.set_local_hops_delta(1);
        let transport = Transport::new(config);
        let manager = transport.iface_manager();
        let ordinary = *manager.lock().await.new_channel(8).address();
        assert!(transport.should_apply_delta(&Packet::default(), &ordinary).await);
        let mut iface = shared_test_config("local_client", synthetic);
        iface.shared_instance_type = Some("unix".to_string());
        iface.socket_path = Some(path.to_string_lossy().into_owned());
        let mut ingress = transport.iface_rx();
        let (_, batch) = start_shared_config(&transport, iface).await;
        let root = batch.tunnel_synth_ifaces[0];
        assert!(!transport.should_apply_delta(&Packet::default(), &ordinary).await);
        let (mut peer, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("attachment deadline")
            .expect("attachment");
        shared_first_ingress(
            &mut peer,
            &mut ingress,
            synthetic,
            "configured-shared-original",
            b"configured Unix attached first frame",
        )
        .await;
        stop_shared_config(&transport, root).await;
        assert!(manager.lock().await.stop_interface(ordinary));
    }
}
