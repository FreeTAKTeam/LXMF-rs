use super::*;
use rns_transport::buffer::OutputBuffer;
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::iface::InterfaceMode;
use rns_transport::packet::{Packet, PacketDataBuffer};
use rns_transport::transport::TransportConfig;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::time::timeout;

pub(super) fn named_active<'a>(
    managed: &'a HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    name: &str,
) -> &'a ManagedHotApplyInterface {
    managed
        .values()
        .find(|active| active.record.name.as_deref() == Some(name))
        .expect("named native interface")
}

pub(super) fn has_named(
    managed: &HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    name: &str,
) -> bool {
    managed
        .values()
        .any(|active| active.native.is_some() && active.record.name.as_deref() == Some(name))
}

pub(super) fn info(name: &str, host: &str, port: u16) -> DiscoveredInterface {
    serde_json::from_value(json!({
        "discovery_hash": vec![7u8; 32], "type": "BackboneInterface", "transport": true,
        "impl_name": "RNS", "version": "1.5.5", "name": name,
        "received": rns_transport::time::now_epoch_secs_u64() as f64, "value": 1,
        "transport_id": "11111111111111111111111111111111", "network_id": "22222222222222222222222222222222",
        "hops": 1, "latitude": null, "longitude": null, "height": null,
        "reachable_on": host, "port": port,
    })).expect("discovery record")
}

pub(super) fn transport(attached: bool, forwarding: bool) -> Arc<Transport> {
    let identity = PrivateIdentity::new_from_name("native-discovery-owner");
    let mut config = TransportConfig::new("native-discovery-owner", &identity, true);
    config.set_connected_to_shared_instance(attached);
    config.set_transport_enabled(forwarding);
    Arc::new(Transport::new(config))
}

pub(super) fn discovery(
    path: &std::path::Path,
    maximum: u32,
    allow_unverified: bool,
) -> NativeDiscovery {
    NativeDiscovery::new(
        NativeDiscoveryConfig::new(
            path.to_owned(),
            &ReticulumRuntimePolicy {
                discover_interfaces: true,
                max_autoconnected_interfaces: maximum,
                ..ReticulumRuntimePolicy::default()
            },
            allow_unverified,
            &[],
            (3600, 5, 0),
        )
        .expect("enabled discovery"),
    )
}

async fn deliver_on_discovered_stream(host: &str) {
    let root = tempfile::tempdir().expect("discovery root");
    let listener = TcpListener::bind((host, 0)).await.expect("native peer");
    let info = info("native-peer", host, listener.local_addr().expect("endpoint").port());
    let transport = transport(false, true);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let mut observed = transport.iface_rx();
    let mut runtime = discovery(root.path(), 1, false);
    let mut managed = HashMap::new();
    runtime.admit(&info, &manager, &mut managed, &transport, &daemon).await;
    let (mut peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("discovery connection deadline")
        .expect("discovery connection");
    let active = named_active(&managed, "native-peer");
    assert_eq!(active.record.kind, discovered_carrier_kind());
    assert_eq!(active.origin.endpoint_hash(), Some(endpoint_hash(&info)));
    let packet = Packet {
        data: PacketDataBuffer::new_from_slice(b"exact-discovered-carrier-bytes"),
        ..Packet::default()
    };
    let raw = packet.to_bytes().expect("packet");
    let mut frame = vec![0; raw.len() * 2 + 2];
    let length = Hdlc::encode(&raw, &mut OutputBuffer::new(&mut frame)).expect("HDLC");
    peer.write_all(&frame[..length]).await.expect("native peer ingress");
    let received = timeout(Duration::from_secs(2), observed.recv())
        .await
        .expect("packet deadline")
        .expect("packet event");
    assert_eq!(
        received.address,
        active.native.as_ref().expect("active native root").address,
        "selected native discovery interface"
    );
    assert_eq!(received.packet.data, packet.data, "exact receiver bytes");
    let guard = manager.lock().await;
    assert_eq!(
        guard
            .policy(&active.native.as_ref().expect("active native root").address)
            .expect("policy")
            .mode,
        InterfaceMode::Gateway
    );
    assert_eq!(
        guard
            .announce_pacing(&active.native.as_ref().expect("active native root").address)
            .expect("pacing")
            .0,
        5_000_000
    );
    let shared = guard
        .shared_config(&active.native.as_ref().expect("active native root").address)
        .expect("shared policy");
    assert_eq!(shared.announces_to_internal, None);
    assert_eq!(
        (shared.announce_rate_target, shared.announce_rate_grace, shared.announce_rate_penalty),
        (Some(3600), Some(5), Some(0))
    );
    drop(guard);
    detach_discovered(
        &manager,
        &mut managed,
        "native-peer",
        Some(&transport),
        Some(&Arc::downgrade(&daemon)),
    )
    .await
    .expect("joined native worker");
    assert!(manager.lock().await.interface_hashes().is_empty());
}

#[tokio::test]
async fn discovery_connects_real_native_backbone_and_preserves_exact_packet_provenance() {
    deliver_on_discovered_stream("127.0.0.2").await;
}

#[tokio::test]
async fn discovery_connects_ipv6_loopback_with_a_valid_stream_endpoint() {
    deliver_on_discovered_stream("::1").await;
}

#[tokio::test]
async fn initial_discovery_uses_retained_transport_entries_and_reserves_disabled_names() {
    let root = tempfile::tempdir().expect("discovery root");
    let listener = TcpListener::bind("127.0.0.2:0").await.expect("native peer");
    let mut retained =
        info("operator", "127.0.0.2", listener.local_addr().expect("endpoint").port());
    retained.received -= 4.0 * 86400.0;
    InterfaceDiscoveryStore::new(root.path())
        .observe(retained.clone())
        .expect("retained stale entry");
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(vec![InterfaceRecord {
        kind: "udp".into(),
        enabled: false,
        name: Some("operator".into()),
        host: None,
        port: None,
        settings: None,
    }]);
    let mut runtime = discovery(root.path(), 1, false);
    let mut managed = HashMap::new();
    runtime.initial(&manager, &mut managed, &transport, &daemon).await;
    let (_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("initial discovery deadline")
        .expect("initial connection");
    assert!(has_named(&managed, "operator (2)"));
    assert!(runtime.lifecycle.initial_autoconnect_ran());
    assert_eq!(
        manager
            .lock()
            .await
            .policy(
                &named_active(&managed, "operator (2)")
                    .native
                    .as_ref()
                    .expect("active native root")
                    .address
            )
            .expect("policy")
            .mode,
        InterfaceMode::Full
    );
    assert_eq!(daemon.interface_records()[0].name.as_deref(), Some("operator"));
    assert!(!daemon.interface_records()[0].enabled);
    detach_discovered(
        &manager,
        &mut managed,
        "operator (2)",
        Some(&transport),
        Some(&Arc::downgrade(&daemon)),
    )
    .await
    .expect("stop");
}

#[tokio::test]
async fn live_observation_has_no_transport_filter_but_rechecks_endpoint_slot_and_qualification() {
    let root = tempfile::tempdir().expect("discovery root");
    let listener = TcpListener::bind("127.0.0.2:0").await.expect("native peer");
    let mut candidate =
        info("live-peer", "127.0.0.2", listener.local_addr().expect("endpoint").port());
    candidate.transport = false;
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let mut runtime = discovery(root.path(), 1, false);
    let mut managed = HashMap::new();
    let mut unverified = candidate.clone();
    unverified.impl_name = Some("reticulumd".into());
    runtime.admit(&unverified, &manager, &mut managed, &transport, &daemon).await;
    assert!(managed.is_empty());
    runtime.admit(&candidate, &manager, &mut managed, &transport, &daemon).await;
    let (_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("live discovery deadline")
        .expect("live connection");
    runtime.admit(&candidate, &manager, &mut managed, &transport, &daemon).await;
    candidate.name = "second-slot".into();
    candidate.port = Some(candidate.port.expect("port") + 1);
    runtime.admit(&candidate, &manager, &mut managed, &transport, &daemon).await;
    assert_eq!(managed.len(), 1);
    assert_eq!(manager.lock().await.interface_hashes().len(), 1);
    detach_discovered(
        &manager,
        &mut managed,
        "live-peer",
        Some(&transport),
        Some(&Arc::downgrade(&daemon)),
    )
    .await
    .expect("stop");
}

#[tokio::test]
async fn external_apply_preserves_private_origin_and_rejects_forged_discovery_authority() {
    let root = tempfile::tempdir().expect("discovery root");
    let listener = TcpListener::bind("127.0.0.2:0").await.expect("native peer");
    let candidate =
        info("internal-peer", "127.0.0.2", listener.local_addr().expect("endpoint").port());
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let mut runtime = discovery(root.path(), 2, false);
    let mut managed = HashMap::new();
    runtime.admit(&candidate, &manager, &mut managed, &transport, &daemon).await;
    let (_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("native deadline")
        .expect("peer");
    let address = named_active(&managed, "internal-peer")
        .native
        .as_ref()
        .expect("active native root")
        .address;
    let mut conflicting = named_active(&managed, "internal-peer").record.clone();
    conflicting.kind = "tcp_client".into();
    conflicting.port = Some(1);
    let forged = InterfaceRecord {
        kind: "udp".into(),
        enabled: true,
        name: Some("forged-origin".into()),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        settings: Some(json!({ "_runtime": { "discovery": { "endpoint_hash": "00" } } })),
    };
    let refreshes = HotApplyRuntimeRefreshes::new();
    let applied = apply_hot_apply_interface_records(
        NativeInterfaceContext {
            iface_manager: &manager,
            transport: Some(&transport),
            reconnect_events: None,
        },
        &mut managed,
        vec![conflicting, forged],
        &refreshes,
        Some(&Arc::downgrade(&daemon)),
    )
    .await;
    assert_eq!(
        named_active(&managed, "internal-peer")
            .native
            .as_ref()
            .expect("active native root")
            .address,
        address
    );
    assert_eq!(named_active(&managed, "internal-peer").record.port, candidate.port);
    assert!(named_active(&managed, "forged-origin").origin.configuration_owned());
    assert_eq!(
        applied.iter().filter(|record| record.name.as_deref() == Some("internal-peer")).count(),
        1
    );
    apply_hot_apply_interface_records(
        NativeInterfaceContext {
            iface_manager: &manager,
            transport: Some(&transport),
            reconnect_events: None,
        },
        &mut managed,
        vec![],
        &refreshes,
        None,
    )
    .await;
    assert!(!has_named(&managed, "forged-origin"));
    assert_eq!(
        named_active(&managed, "internal-peer")
            .native
            .as_ref()
            .expect("active native root")
            .address,
        address
    );
    detach_discovered(
        &manager,
        &mut managed,
        "internal-peer",
        Some(&transport),
        Some(&Arc::downgrade(&daemon)),
    )
    .await
    .expect("stop");
}

#[tokio::test]
async fn attached_client_does_not_start_initial_or_live_discovery() {
    let root = tempfile::tempdir().expect("discovery root");
    let candidate = info("attached-exclusion", "127.0.0.2", 4242);
    InterfaceDiscoveryStore::new(root.path()).observe(candidate.clone()).expect("persist");
    let transport = transport(true, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let mut runtime = discovery(root.path(), 1, true);
    let mut managed = HashMap::new();
    runtime.initial(&manager, &mut managed, &transport, &daemon).await;
    runtime.admit(&candidate, &manager, &mut managed, &transport, &daemon).await;
    assert!(managed.is_empty());
    assert!(!runtime.lifecycle.initial_autoconnect_ran());
    assert!(manager.lock().await.interface_hashes().is_empty());
}

#[tokio::test(start_paused = true)]
async fn native_monitor_never_detaches_early_at_fractional_or_skipped_tick_boundaries() {
    let root = tempfile::tempdir().expect("discovery root");
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let mut runtime = discovery(root.path(), 1, false);
    let channel = manager.lock().await.new_channel(1);
    channel.set_online(false);
    let record = InterfaceRecord {
        kind: discovered_carrier_kind().into(),
        enabled: true,
        name: Some("fractional".into()),
        host: Some("127.0.0.2".into()),
        port: Some(4242),
        settings: None,
    };
    daemon.replace_interfaces(vec![record.clone()]);
    let mut managed = HashMap::from([(
        "fractional".into(),
        ManagedHotApplyInterface::active(
            record,
            *channel.address(),
            None,
            ManagedInterfaceOrigin::Discovered {
                endpoint: [4; 32],
                source: "network".into(),
                down_since: None,
            },
        ),
    )]);
    runtime.lifecycle.monitor_interface(configured_key("fractional").monitor_id());
    let refreshes = HotApplyRuntimeRefreshes::new();
    tokio::time::advance(Duration::from_millis(900)).await;
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon, None).await;
    tokio::time::advance(Duration::from_millis(11_200)).await;
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon, None).await;
    assert!(
        has_named(&managed, "fractional"),
        "integer rounding cannot authorize an early native detach"
    );
    channel.set_online(true);
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon, None).await;
    assert!(matches!(
        named_active(&managed, "fractional").origin,
        ManagedInterfaceOrigin::Discovered { down_since: None, .. }
    ));
    channel.set_online(false);
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon, None).await;
    tokio::time::advance(Duration::from_secs(30)).await;
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon, None).await;
    assert!(managed.is_empty());
    assert!(manager.lock().await.interface_hashes().is_empty());
    assert!(daemon.interface_records().is_empty());
}

#[tokio::test]
async fn discovery_observation_queue_backpressures_and_weak_sender_does_not_keep_owner_alive() {
    let (tx, mut rx) = channel(1);
    let observations = DiscoveryObservationSender::new(tx.downgrade());
    let candidate = info("queue", "127.0.0.2", 4242);
    observations.observed(candidate.clone()).await.expect("first queue entry");
    let mut pending = Box::pin(observations.observed(candidate));
    assert!(
        timeout(Duration::from_millis(10), &mut pending).await.is_err(),
        "full queue retains live observation until owner can consume"
    );
    assert!(matches!(rx.recv().await, Some(InterfaceHotApplyCommand::DiscoveryObserved(_))));
    pending.await.expect("backpressure completes after capacity");
    assert!(matches!(rx.recv().await, Some(InterfaceHotApplyCommand::DiscoveryObserved(_))));
    drop(tx);
    assert!(rx.recv().await.is_none(), "weak producer does not extend queue lifetime");
    assert_eq!(
        observations
            .observed(info("closed", "127.0.0.2", 4242))
            .await
            .expect_err("closed owner")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
}
