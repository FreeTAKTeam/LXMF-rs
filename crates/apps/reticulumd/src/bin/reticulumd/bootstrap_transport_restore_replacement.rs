use super::restore_tests::{native_address, verify_unrelated_tcp};
use super::*;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, NativeDiscoveryConfig};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout, Duration};

#[tokio::test]
async fn public_bootstrap_endpoint_replacement_restores_one_original_recipe_in_its_current_slot() {
    let root = tempfile::tempdir().expect("state");
    let original = TcpListener::bind("127.0.0.1:0").await.expect("original peer");
    let replacement = TcpListener::bind("127.0.0.1:0").await.expect("replacement peer");
    let unrelated = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let candidate = TcpListener::bind("127.0.0.2:0").await.expect("candidate peer");
    let original_port = original.local_addr().expect("original endpoint").port();
    let replacement_port = replacement.local_addr().expect("replacement endpoint").port();
    let unrelated_port = unrelated.local_addr().expect("unrelated endpoint").port();
    let config = DaemonConfig::from_toml(&format!("interfaces=[{{type='udp',name='disabled-sentinel',enabled=false}},{{type='tcp_client',enabled=true,name='bootstrap-x',host='127.0.0.1',port={original_port},bootstrap_only=true}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={unrelated_port}}}]\n[reticulum]\nshare_instance=false\ndiscover_interfaces=true\nautoconnect_discovered_interfaces=1")).expect("config");
    let startup = start_fixture(&Args::parse_from(["reticulumd"]), &config, root.path()).await;
    let transport = startup.transport.expect("transport");
    let first_address = native_address(&startup.configured_interfaces[1]);
    let unrelated_address = native_address(&startup.configured_interfaces[2]);
    let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
    daemon.replace_interfaces(startup.configured_interfaces.clone());
    let discovery = NativeDiscoveryConfig::new(
        root.path().to_owned(),
        &reticulum_daemon::config::ReticulumRuntimePolicy {
            discover_interfaces: true,
            max_autoconnected_interfaces: 1,
            ..Default::default()
        },
        false,
        &config.interfaces,
        (3600, 5, 0),
    );
    let mut owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
        transport.clone(),
        startup.interfaces.into_owned_roots(&startup.configured_interfaces),
        Arc::downgrade(&daemon),
        discovery,
        startup.configured_startup,
    );
    daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
    let (mut first, _) = timeout(Duration::from_secs(2), original.accept())
        .await
        .expect("first deadline")
        .expect("first native connection");
    let (mut unaffected, _) = timeout(Duration::from_secs(2), unrelated.accept())
        .await
        .expect("unrelated deadline")
        .expect("unrelated native connection");
    verify_unrelated_tcp(&transport, first_address, &mut first, 0x81).await;
    verify_unrelated_tcp(&transport, unrelated_address, &mut unaffected, 0x82).await;

    let current = daemon.interface_records();
    let mut changed = current[1].clone();
    changed.port = Some(replacement_port);
    let mut events = daemon.subscribe_events();
    let response = daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "set_interfaces".into(),
            params: Some(serde_json::json!({"interfaces":[current[2].clone(), changed]})),
        })
        .expect("public Apply");
    assert!(response.error.is_none(), "{response:?}");
    timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("event");
            if event.event_type == "interfaces_updated" && event.payload["complete"] == true {
                break;
            }
        }
    })
    .await
    .expect("Apply completion");
    let (mut changed_peer, _) = timeout(Duration::from_secs(2), replacement.accept())
        .await
        .expect("replacement deadline")
        .expect("replacement native");
    assert_eq!(daemon.interface_records().len(), 2);
    let changed_address = native_address(&daemon.interface_records()[1]);
    assert_ne!(changed_address, first_address);
    verify_unrelated_tcp(&transport, changed_address, &mut changed_peer, 0x83).await;
    verify_unrelated_tcp(&transport, unrelated_address, &mut unaffected, 0x84).await;
    let mut drained = Vec::new();
    timeout(Duration::from_secs(2), first.read_to_end(&mut drained))
        .await
        .expect("first EOF deadline")
        .expect("first root joined");

    let info = serde_json::from_value(serde_json::json!({
        "discovery_hash": vec![8u8;32],"type":"BackboneInterface","transport":true,
        "impl_name":"RNS","version":"1.5.5","name":"replacement-discovered",
        "received":rns_transport::time::now_epoch_secs_u64() as f64,"value":1,
        "transport_id":"11111111111111111111111111111111",
        "network_id":"22222222222222222222222222222222","hops":1,
        "latitude":null,"longitude":null,"height":null,"reachable_on":"127.0.0.2",
        "port":candidate.local_addr().expect("candidate endpoint").port()
    }))
    .expect("qualified observation");
    owner.discovery_observations().observed(info).await.expect("observed");
    let (discovered, _) = timeout(Duration::from_secs(2), candidate.accept())
        .await
        .expect("discovery deadline")
        .expect("discovered native");
    timeout(Duration::from_secs(8), async {
        while daemon.interface_records()[1].enabled {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("replacement disabled");
    let mut drained = Vec::new();
    timeout(Duration::from_secs(2), changed_peer.read_to_end(&mut drained))
        .await
        .expect("replacement EOF deadline")
        .expect("replacement joined");
    verify_unrelated_tcp(&transport, unrelated_address, &mut unaffected, 0x85).await;
    drop(discovered);
    drop(candidate);

    let (mut restored, _) = timeout(Duration::from_secs(8), original.accept())
        .await
        .expect("original recipe restoration deadline")
        .expect("restored original native");
    timeout(Duration::from_secs(2), async {
        while !daemon.interface_records().iter().any(|record| {
            record.name.as_deref() == Some("bootstrap-x")
                && record.enabled
                && record.port == Some(original_port)
        }) {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("restored publication");
    let current = daemon.interface_records();
    assert_eq!(
        current.iter().filter(|record| record.name.as_deref() == Some("bootstrap-x")).count(),
        1,
        "restoration replaces the old owned occurrence instead of appending a duplicate"
    );
    assert_eq!(current.len(), 3, "unrelated, restored and discovered records only");
    assert_eq!(current[1].name.as_deref(), Some("bootstrap-x"));
    assert_eq!(current[1].port, Some(original_port));
    assert_eq!(native_address(&current[0]), unrelated_address);
    let restored_address = native_address(&current[1]);
    assert_ne!(restored_address, first_address);
    assert_ne!(restored_address, changed_address);
    verify_unrelated_tcp(&transport, restored_address, &mut restored, 0x86).await;
    verify_unrelated_tcp(&transport, unrelated_address, &mut unaffected, 0x87).await;
    owner.shutdown().await.expect("joined owner");
    assert!(transport.interface_hashes().await.is_empty());
}
