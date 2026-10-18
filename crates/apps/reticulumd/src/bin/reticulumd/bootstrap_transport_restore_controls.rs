use super::*;
use crate::interface_hot_apply::InterfaceHotApplyBridge;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::io::AsyncReadExt;
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn reconnect_synthesizer_is_owned_and_joined_with_discovery_disabled() {
    let root = tempfile::tempdir().expect("state");
    let peer = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("peer");
    let port = peer.local_addr().expect("endpoint").port();
    let config = DaemonConfig::from_toml(&format!("interfaces=[{{type='tcp_client',name='retained-reconnect',enabled=true,host='127.0.0.1',port={port}}}]\n[reticulum]\nshare_instance=false\ndiscover_interfaces=false")).expect("config");
    let startup = start_fixture(&Args::parse_from(["reticulumd"]), &config, root.path()).await;
    let transport = startup.transport.expect("transport");
    let manager = transport.iface_manager();
    let (mut connection, _) = timeout(Duration::from_secs(2), peer.accept())
        .await
        .expect("native deadline")
        .expect("native TCP");
    let context = startup.configured_startup.expect("context retained without discovery");
    assert_eq!(context.identity_hash.as_slice(), transport.internal_identity().await.as_slice());
    let weak = context.reconnect_events.downgrade();
    let task = context.reconnect_completion.abort_handle();
    assert!(!task.is_finished());
    let unrelated = manager.lock().await.new_channel(1);
    let mut owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
        transport,
        startup.interfaces.into_owned_roots(&startup.configured_interfaces),
        std::sync::Weak::new(),
        None,
        Some(context),
    );
    timeout(Duration::from_secs(2), owner.shutdown())
        .await
        .expect("joined owner deadline")
        .expect("joined native and reconnect task");
    assert!(task.is_finished(), "completion is joined rather than dropped");
    assert!(weak.upgrade().is_none(), "all native and retained senders retired");
    let mut bytes = Vec::new();
    timeout(Duration::from_secs(2), connection.read_to_end(&mut bytes))
        .await
        .expect("native EOF deadline")
        .expect("native EOF");
    assert_eq!(
        manager.lock().await.interface_hashes(),
        std::collections::HashSet::from([*unrelated.address()])
    );
}

struct CountDrop(Arc<AtomicUsize>);
impl Drop for CountDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn reconnect_faults_report_after_native_retirement_and_keep_cancelled_shutdown_joinable() {
    for panic in [true, false] {
        let root = tempfile::tempdir().expect("state");
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("port");
        let port = socket.local_addr().expect("endpoint").port();
        drop(socket);
        let config = DaemonConfig::from_toml(&format!("interfaces=[{{type='udp',enabled=true,host='127.0.0.1',port={port}}}]\n[reticulum]\nshare_instance=false")).expect("config");
        let startup = start_fixture(&Args::parse_from(["reticulumd"]), &config, root.path()).await;
        let transport = startup.transport.expect("transport");
        let manager = transport.iface_manager();
        let mut context = startup.configured_startup.expect("context");
        let (replacement_tx, mut replacement_rx) = tokio::sync::mpsc::channel(1);
        let old_sender = std::mem::replace(&mut context.reconnect_events, replacement_tx);
        let weak = context.reconnect_events.downgrade();
        let dropped = Arc::new(AtomicUsize::new(0));
        let retired = Arc::new(AtomicBool::new(false));
        let counter = dropped.clone();
        let observed = retired.clone();
        let observer = manager.clone();
        let task = tokio::spawn(async move {
            let _drop = CountDrop(counter);
            assert!(replacement_rx.recv().await.is_none(), "retained sender dropped before join");
            assert!(
                observer.lock().await.interface_hashes().is_empty(),
                "native roots retire first"
            );
            observed.store(true, Ordering::SeqCst);
            if panic {
                panic!("injected reconnect completion failure");
            }
            std::future::pending::<()>().await;
        });
        let original = std::mem::replace(&mut context.reconnect_completion, task);
        drop(old_sender);
        timeout(Duration::from_secs(2), original)
            .await
            .expect("original synthesizer deadline")
            .expect("original synthesizer settled before injection");
        let mut owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
            transport,
            startup.interfaces.into_owned_roots(&startup.configured_interfaces),
            std::sync::Weak::new(),
            None,
            Some(context),
        );
        if !panic {
            assert!(timeout(Duration::from_secs(1), owner.shutdown()).await.is_err());
            assert_eq!(dropped.load(Ordering::SeqCst), 0);
        }
        let error = owner.shutdown().await.expect_err("reconnect task fault remains visible");
        assert_eq!(
            error.kind(),
            if panic { std::io::ErrorKind::Other } else { std::io::ErrorKind::TimedOut }
        );
        assert!(error.to_string().contains("stream reconnect tunnel synthesizer"));
        assert!(retired.load(Ordering::SeqCst));
        assert!(weak.upgrade().is_none());
        assert_eq!(dropped.load(Ordering::SeqCst), 1, "faulted or aborted completion awaited");
        assert!(manager.lock().await.interface_hashes().is_empty());
    }
}

#[tokio::test]
async fn restored_i2p_receives_the_retained_storage_root_and_transport_identity() {
    let root = tempfile::tempdir().expect("state");
    let transport = Arc::new(Transport::new(TransportConfig::default()));
    let recipe = reticulum_daemon::config::InterfaceConfig {
        kind: "i2p".into(),
        enabled: Some(true),
        name: Some("retained-context".into()),
        sam_host: Some("127.0.0.1".into()),
        sam_port: Some(1),
        peers: Some(Vec::new()),
        connectable: Some(false),
        ..Default::default()
    };
    let context = ConfiguredStartupRuntime::for_test(&transport, root.path(), &[], false).await;
    let expected = rns_transport::iface::i2p::i2p_private_key_path_with_identity(
        root.path(),
        "retained-context",
        Some(&context.identity_hash),
    );
    let (restored, error) = context.restore(7, &recipe, &transport).await;
    assert!(error.is_none(), "{error:?}");
    assert_eq!(restored.index, 7);
    let metadata = &restored.record.settings.as_ref().expect("metadata")["_runtime"]["i2p"];
    assert_eq!(metadata["private_key_path"], expected.to_string_lossy().as_ref());
    assert_eq!(metadata["private_key_persisted"], false);
    assert!(!expected.exists(), "no connectable session or external SAM server is used");
    let native = restored.native.expect("real I2P parent");
    assert!(matches!(
        native.status,
        Some(crate::configured_interface_runtime::NativeRuntimeStatus::I2p(_))
    ));
    crate::interface_hot_apply::stop_owned_native(
        &transport.iface_manager(),
        Some(&transport),
        native,
    )
    .await
    .expect("I2P parent joined");
    timeout(Duration::from_secs(2), context.into_reconnect_completion())
        .await
        .expect("reconnect deadline")
        .expect("reconnect joined");
    assert!(transport.interface_hashes().await.is_empty());
}
