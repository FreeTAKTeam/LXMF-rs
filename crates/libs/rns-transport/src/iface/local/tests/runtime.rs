use super::*;
use crate::iface::hdlc::Hdlc;
use crate::packet::Packet;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn parses_abstract_config_labels() {
    let endpoint = LocalUnixEndpoint::from_config_value("@rns/default");
    assert_eq!(endpoint, LocalUnixEndpoint::Abstract("rns/default".to_string()));
    assert_eq!(endpoint.label(), "@rns/default");
    assert!(endpoint.is_abstract());
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[tokio::test]
async fn abstract_unix_endpoint_accepts_stream_without_filesystem_socket() {
    let name = format!(
        "rns/test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos()
    );
    let endpoint = LocalUnixEndpoint::Abstract(name.clone());
    let listener = UnixListener::bind(endpoint.tokio_path()).expect("bind abstract socket");

    let connect_path = endpoint.tokio_path();
    let accept_task = tokio::spawn(async move { listener.accept().await });
    let _client = UnixStream::connect(connect_path).await.expect("connect abstract socket");
    let (_server, _addr) = accept_task.await.expect("accept task").expect("accept abstract socket");

    assert!(!Path::new(format!("@{name}").as_str()).exists());
}

#[tokio::test]
async fn local_unix_server_stops_on_interface_stop_and_releases_socket() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("reticulum-local-server.sock");
    let endpoint = LocalUnixEndpoint::filesystem(path.clone());
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let context = {
        let mut manager_guard = manager.lock().await;
        manager_guard.new_context(LocalUnixServer::new(path.clone(), manager.clone()))
    };
    let iface_address = context.channel.address;
    let task = tokio::spawn(LocalUnixServer::spawn(context));

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("local unix server socket was not created");

    assert!(manager.lock().await.stop_interface(iface_address));
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("local unix server task timed out")
        .expect("local unix server task");

    assert!(!path.exists());
    LocalUnixServer::preflight_bind_available(&endpoint)
        .await
        .expect("socket path can be rebound after stop");
}

#[cfg(unix)]
#[tokio::test]
async fn local_unix_shared_clients_keep_plaintext_wire_policy_with_parent_ifac() {
    use crate::buffer::OutputBuffer;
    use crate::iface::{InterfaceSharedConfig, TxMessage, TxMessageType};
    use crate::packet::PacketDataBuffer;

    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("reticulum-shared-ifac.sock");
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let (context, parent_iface, receiver) = {
        let mut manager_guard = manager.lock().await;
        let context = manager_guard.new_context(
            LocalUnixServer::new(path.clone(), manager.clone()).with_plaintext_shared_clients(true),
        );
        let parent_iface = context.channel.address;
        assert!(manager_guard.set_shared_config(
            parent_iface,
            InterfaceSharedConfig {
                network_name: Some("local-shared-ifac".to_string()),
                passphrase: Some("local-shared-secret".to_string()),
                ..Default::default()
            },
        ));
        assert!(manager_guard.set_shared_instance(parent_iface, true));
        (context, parent_iface, manager_guard.receiver())
    };
    let task = tokio::spawn(LocalUnixServer::spawn(context));

    tokio::time::timeout(Duration::from_secs(2), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("local shared-instance socket was not created");

    let mut client = UnixStream::connect(&path).await.expect("connect local client");
    let child_iface = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let children = manager.lock().await.local_client_interfaces();
            if let Some(child) = children.into_iter().next() {
                break child;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("local client interface was not registered");
    {
        let mut manager_guard = manager.lock().await;
        assert!(manager_guard.set_shared_config(
            parent_iface,
            InterfaceSharedConfig {
                network_name: Some("rotated-local-shared-ifac".to_string()),
                passphrase: Some("rotated-local-shared-secret".to_string()),
                ..Default::default()
            },
        ));
        let child_config =
            manager_guard.shared_config(&child_iface).expect("attached local client config");
        assert!(child_config.network_name.is_none());
        assert!(child_config.passphrase.is_none());
    }

    let inbound = Packet {
        data: PacketDataBuffer::new_from_slice(b"plaintext shared-instance ingress"),
        ..Default::default()
    };
    client
        .write_all(
            &Hdlc::frame(&inbound.to_bytes().expect("serialize ingress packet"))
                .expect("frame ingress packet"),
        )
        .await
        .expect("write plaintext ingress frame");
    let received =
        tokio::time::timeout(Duration::from_secs(2), async { receiver.lock().await.recv().await })
            .await
            .expect("shared-instance ingress timed out")
            .expect("shared-instance receiver closed");
    assert_eq!(received.address, child_iface);
    assert_eq!(received.packet.data.as_slice(), b"plaintext shared-instance ingress");

    let outbound = Packet {
        data: PacketDataBuffer::new_from_slice(b"plaintext shared-instance egress"),
        ..Default::default()
    };
    let trace = manager
        .lock()
        .await
        .send(TxMessage { tx_type: TxMessageType::Direct(child_iface), packet: outbound })
        .await;
    assert_eq!(trace.sent_ifaces, 1);

    let mut framed = Vec::new();
    let mut chunk = [0_u8; 4096];
    let (start, end) = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let read = client.read(&mut chunk).await.expect("read shared-client egress");
            assert_ne!(read, 0, "local server closed before egress");
            framed.extend_from_slice(&chunk[..read]);
            if let Some(frame) = Hdlc::find(&framed) {
                break frame;
            }
        }
    })
    .await
    .expect("shared-instance egress timed out");
    let mut decoded = vec![0_u8; end - start];
    let decoded_len = Hdlc::decode(&framed[start..=end], &mut OutputBuffer::new(&mut decoded))
        .expect("decode shared-client egress frame");
    assert_eq!(decoded[0] & 0x80, 0, "shared-client egress must stay plaintext");
    let decoded_packet =
        Packet::from_bytes(&decoded[..decoded_len]).expect("deserialize shared-client egress");
    assert_eq!(decoded_packet.data.as_slice(), b"plaintext shared-instance egress");

    assert!(manager.lock().await.stop_interface(parent_iface));
    drop(client);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("local shared-instance server task timed out")
        .expect("local shared-instance server task");
}

#[tokio::test]
async fn local_unix_connect_client_retries_initial_connect_and_reconnects_after_close() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("reticulum-local.sock");
    let endpoint = LocalUnixEndpoint::filesystem(path.clone());
    let mut manager = InterfaceManager::new(8);
    let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::channel(32);
    let context = manager.new_context(
        LocalUnixClient::new_connect(endpoint.clone())
            .with_reconnect_wait(Duration::from_millis(20))
            .with_reconnect_events(reconnect_tx),
    );
    let iface_address = context.channel.address;
    let cancel = context.cancel.clone();
    let iface_stop = context.channel.stop.clone();
    let task = tokio::spawn(LocalUnixClient::spawn(context));

    let first_listener = UnixListener::bind(endpoint.tokio_path()).expect("bind first socket");
    let (first_stream, _) = tokio::time::timeout(Duration::from_secs(2), first_listener.accept())
        .await
        .expect("first reconnect timed out")
        .expect("first accept");
    drop(first_stream);
    drop(first_listener);
    remove_socket_file(&path).expect("remove first socket");

    let second_listener = UnixListener::bind(endpoint.tokio_path()).expect("bind second socket");
    let (second_stream, _) = tokio::time::timeout(Duration::from_secs(2), second_listener.accept())
        .await
        .expect("second reconnect timed out")
        .expect("second accept");
    let reconnected_iface = tokio::time::timeout(Duration::from_secs(2), reconnect_rx.recv())
        .await
        .expect("reconnect event timed out")
        .expect("reconnect event");

    cancel.cancel();
    drop(second_stream);
    drop(second_listener);
    let _ = remove_socket_file(&path);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("client task timed out")
        .expect("client task");

    assert!(iface_stop.is_cancelled());
    assert_eq!(reconnected_iface, iface_address);
}
