use rns_rpc::RpcRequest;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn discovery_stops_both_identical_unnamed_configured_kiss_roots() {
    let root = tempfile::tempdir().expect("state");
    let bootstrap_peer = TcpListener::bind("127.0.0.1:0").await.expect("bootstrap peer");
    let candidate_peer = TcpListener::bind("127.0.0.2:0").await.expect("discovered peer");
    let candidate_port = candidate_peer.local_addr().expect("port").port();
    let candidate = super::tests::info("replacement", "127.0.0.2", candidate_port);
    rns_transport::discovery::InterfaceDiscoveryStore::new(root.path())
        .observe(candidate)
        .expect("candidate");
    let recipe = format!("[[interfaces]]\ntype='kiss_tcp_client'\nhost='127.0.0.1'\nport={}\nenabled=true\nbootstrap_only=true\n", bootstrap_peer.local_addr().expect("port").port());
    let config = root.path().join("config.toml");
    std::fs::write(&config, format!("[reticulum]\nshare_instance=false\ndiscover_interfaces=true\nautoconnect_discovered_interfaces=1\n{recipe}{recipe}")).expect("config");
    let db = root.path().join("state.db");
    let rpc = root.path().join("rpc.sock");
    let args = <crate::Args as clap::Parser>::parse_from([
        "reticulumd",
        "--db",
        db.to_str().expect("db"),
        "--config",
        config.to_str().expect("config"),
        "--rpc-unix",
        rpc.to_str().expect("rpc"),
    ]);
    let mut context = crate::bootstrap::bootstrap(args).await;
    let (first, _) = timeout(Duration::from_secs(2), bootstrap_peer.accept())
        .await
        .expect("first deadline")
        .expect("first root");
    let (second, _) = timeout(Duration::from_secs(2), bootstrap_peer.accept())
        .await
        .expect("second deadline")
        .expect("second root");
    let (candidate, _) = timeout(Duration::from_secs(2), candidate_peer.accept())
        .await
        .expect("candidate deadline")
        .expect("discovered root");
    let before = context.daemon.interface_records();
    let roots: Vec<_> = before.iter().filter(|record| record.kind == "kiss_tcp_client").collect();
    assert_eq!(roots.len(), 2);
    let addresses: Vec<_> = roots
        .iter()
        .map(|record| {
            record.settings.as_ref().expect("runtime")["_runtime"]["iface"]
                .as_str()
                .expect("native address")
                .to_owned()
        })
        .collect();
    assert_ne!(addresses[0], addresses[1], "distinct native roots for identical recipes");
    // Exercise the public four-kind path before the monitor disables the roots.
    let response = context
        .daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "set_interfaces".into(),
            params: Some(json!({"interfaces": []})),
        })
        .expect("apply RPC");
    assert!(response.error.is_none(), "{response:?}");
    timeout(Duration::from_secs(8), async {
        loop {
            let records = context.daemon.interface_records();
            let roots: Vec<_> =
                records.iter().filter(|record| record.kind == "kiss_tcp_client").collect();
            if roots.len() == 2 && roots.iter().all(|record| !record.enabled) {
                assert!(roots
                    .iter()
                    .all(|record| record.settings.as_ref().expect("runtime")["_runtime"]["iface"]
                        .is_null()));
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("both original configured occurrences must be disabled");
    peer_eof(first).await;
    peer_eof(second).await;
    context.interface_owner.as_mut().expect("owner").shutdown().await.expect("native owner joined");
    peer_eof(candidate).await;
}

async fn configured_context(
    root: &std::path::Path,
    interfaces: &str,
) -> crate::bootstrap::BootstrapContext {
    let path = root.join("config.toml");
    std::fs::write(&path, format!("[reticulum]\nshare_instance=false\n{interfaces}"))
        .expect("config");
    let db = root.join("state.db");
    let rpc = root.join("rpc.sock");
    let args = <crate::Args as clap::Parser>::parse_from([
        "reticulumd",
        "--db",
        db.to_str().expect("db"),
        "--config",
        path.to_str().expect("config"),
        "--rpc-unix",
        rpc.to_str().expect("rpc"),
    ]);
    crate::bootstrap::bootstrap(args).await
}

async fn peer_eof(mut peer: tokio::net::TcpStream) {
    use tokio::io::AsyncReadExt;
    let mut received = Vec::new();
    timeout(Duration::from_secs(2), peer.read_to_end(&mut received))
        .await
        .expect("native stream closed before completion")
        .expect("peer EOF");
}

#[tokio::test]
async fn colliding_four_kind_recipes_keep_both_native_roots_even_with_a_disabled_duplicate() {
    let root = tempfile::tempdir().expect("state");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("peer");
    let port = listener.local_addr().expect("port").port();
    let recipe =
        format!("[[interfaces]]\ntype='tcp_client'\nhost='127.0.0.1'\nport={port}\nenabled=true\n");
    let disabled = recipe.replace("enabled=true", "enabled=false");
    let mut context = configured_context(root.path(), &format!("{recipe}{recipe}{disabled}")).await;
    let (first, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("first deadline")
        .expect("first root");
    let (second, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("second deadline")
        .expect("second root");
    tokio::task::yield_now().await;
    let records = context.daemon.interface_records();
    assert_eq!(records.len(), 3);
    let addresses: Vec<_> = records[..2]
        .iter()
        .map(|record| {
            record.settings.as_ref().expect("settings")["_runtime"]["iface"]
                .as_str()
                .expect("actual native root")
                .to_owned()
        })
        .collect();
    assert_ne!(addresses[0], addresses[1]);
    assert!(!records[2].enabled);
    let mut events = context.daemon.subscribe_events();
    context
        .daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "set_interfaces".into(),
            params: Some(json!({"interfaces": []})),
        })
        .expect("queued Apply");
    timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("native completion event");
            if event.event_type == "interfaces_updated" && event.payload["complete"] == true {
                break;
            }
        }
    })
    .await
    .expect("owner completed Apply before preservation is checked");
    let preserved = context.daemon.interface_records();
    assert_eq!(preserved.len(), 3, "each colliding startup occurrence stays protected");
    assert!(preserved[..2].iter().all(|record| record.enabled));
    let owner = context.interface_owner.as_mut().expect("owner");
    owner.shutdown().await.expect("confirmed native shutdown");
    peer_eof(first).await;
    peer_eof(second).await;
    owner.shutdown().await.expect("idempotent completion");
    let response = context
        .daemon
        .handle_rpc(RpcRequest {
            id: 2,
            method: "set_interfaces".into(),
            params: Some(json!({"interfaces": []})),
        })
        .expect("closed bridge response");
    assert!(response.error.is_some(), "daemon-held Sender cannot restart a joined owner");
}

#[tokio::test]
async fn startup_policy_update_preserves_native_binding_and_named_detach_clears_it() {
    let root = tempfile::tempdir().expect("state");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("peer");
    let port = listener.local_addr().expect("port").port();
    let mut context = configured_context(root.path(), &format!("[[interfaces]]\ntype='tcp_client'\nname='policy-peer'\nhost='127.0.0.1'\nport={port}\nenabled=true\n[[interfaces]]\ntype='FutureOwnerRow'\nenabled=true\n")).await;
    let (peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("deadline")
        .expect("native root");
    let before = context.daemon.interface_records();
    let runtime = before[0].settings.as_ref().expect("settings")["_runtime"].clone();
    assert!(runtime["iface"].is_string());
    let failed = before[1].clone();
    let mut requested = before[0].clone();
    requested.settings.as_mut().expect("settings")["gravity"] = json!(7);
    context
        .daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "set_interfaces".into(),
            params: Some(json!({"interfaces": [requested]})),
        })
        .expect("queue policy");
    timeout(Duration::from_secs(2), async {
        loop {
            let records = context.daemon.interface_records();
            if records[0].settings.as_ref().expect("settings")["gravity"] == 7 {
                let after = &records[0].settings.as_ref().expect("settings")["_runtime"];
                assert_eq!(after["iface"], runtime["iface"]);
                assert_eq!(after["startup_status"], runtime["startup_status"]);
                assert_eq!(records[1], failed, "a rejected family acquires no native state");
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("native policy completion");
    let daemon = context.daemon.clone();
    let response = tokio::task::spawn_blocking(move || {
        daemon.handle_rpc(RpcRequest {
            id: 2,
            method: "manage_interface".into(),
            params: Some(json!({"operation":"detach","name":"policy-peer"})),
        })
    })
    .await
    .expect("RPC task")
    .expect("RPC");
    assert!(response.error.is_none(), "{response:?}");
    let detached = context.daemon.interface_records();
    assert!(!detached[0].enabled);
    let metadata = &detached[0].settings.as_ref().expect("settings")["_runtime"];
    assert!(metadata["iface"].is_null());
    assert!(metadata["tcp"].is_null());
    assert_eq!(metadata["runtime_status"], "stopped");
    peer_eof(peer).await;
    context.interface_owner.as_mut().expect("owner").shutdown().await.expect("owner stop");
}

#[tokio::test]
async fn discovered_detach_rebinds_a_configured_root_moved_by_apply() {
    let root = tempfile::tempdir().expect("state");
    let x_peer = TcpListener::bind("127.0.0.1:0").await.expect("X");
    let y_peer = TcpListener::bind("127.0.0.1:0").await.expect("Y");
    let d_peer = TcpListener::bind("127.0.0.2:0").await.expect("D");
    let candidate = super::tests::info(
        "slot-discovery",
        "127.0.0.2",
        d_peer.local_addr().expect("port").port(),
    );
    rns_transport::discovery::InterfaceDiscoveryStore::new(root.path())
        .observe(candidate)
        .expect("candidate");
    let mut context = configured_context(root.path(), &format!("discover_interfaces=true\nautoconnect_discovered_interfaces=1\n[[interfaces]]\ntype='FutureProtected'\nenabled=true\n[[interfaces]]\ntype='tcp_client'\nname='X'\nhost='127.0.0.1'\nport={}\nenabled=true\n", x_peer.local_addr().expect("port").port())).await;
    let (x, _) = timeout(Duration::from_secs(2), x_peer.accept())
        .await
        .expect("X deadline")
        .expect("X root");
    let (d, _) = timeout(Duration::from_secs(2), d_peer.accept())
        .await
        .expect("D deadline")
        .expect("D root");
    let before = context.daemon.interface_records();
    let x_record = before[1].clone();
    let address = x_record.settings.as_ref().expect("settings")["_runtime"]["iface"].clone();
    let y_record = rns_rpc::InterfaceRecord {
        kind: "tcp_client".into(),
        name: Some("Y".into()),
        enabled: true,
        host: Some("127.0.0.1".into()),
        port: Some(y_peer.local_addr().expect("port").port()),
        settings: None,
    };
    context
        .daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "set_interfaces".into(),
            params: Some(json!({"interfaces":[y_record, x_record]})),
        })
        .expect("Apply");
    let (y, _) = timeout(Duration::from_secs(2), y_peer.accept())
        .await
        .expect("Y deadline")
        .expect("Y root");
    timeout(Duration::from_secs(2), async {
        loop {
            let records = context.daemon.interface_records();
            if records.len() == 4 && records[3].name.as_deref() == Some("X") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("private X slot moves after discovery row");
    let daemon = context.daemon.clone();
    let response = tokio::task::spawn_blocking(move || {
        daemon.handle_rpc(RpcRequest {
            id: 2,
            method: "manage_interface".into(),
            params: Some(json!({"operation":"detach","name":"slot-discovery"})),
        })
    })
    .await
    .expect("RPC task")
    .expect("detach RPC");
    assert!(response.error.is_none(), "{response:?}");
    peer_eof(d).await;
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let after = context.daemon.interface_records();
    assert_eq!(after.len(), 3, "publication cannot append a duplicate stale-slot root");
    assert_eq!(after[1].name.as_deref(), Some("Y"));
    assert_eq!(after[2].name.as_deref(), Some("X"));
    assert_eq!(after[2].settings.as_ref().expect("settings")["_runtime"]["iface"], address);
    context.interface_owner.as_mut().expect("owner").shutdown().await.expect("joined owner");
    peer_eof(x).await;
    peer_eof(y).await;
}

#[tokio::test]
async fn uniquely_named_failed_startup_can_be_repaired_by_native_named_attach() {
    let root = tempfile::tempdir().expect("state");
    let mut context = configured_context(
        root.path(),
        "[[interfaces]]\ntype='pipe'\nname='repair'\nenabled=true\ncommand=\"'\"\n",
    )
    .await;
    let failed = context.daemon.interface_records();
    assert_eq!(failed.len(), 1);
    assert!(failed[0].settings.as_ref().expect("settings")["_runtime"]["iface"].is_null());
    std::fs::write(root.path().join("config.toml"), "[reticulum]\nshare_instance=false\n[[interfaces]]\ntype='pipe'\nname='repair'\nenabled=true\ncommand='cat'\n").expect("operator repairs trusted config");
    let daemon = context.daemon.clone();
    let response = tokio::task::spawn_blocking(move || {
        daemon.handle_rpc(RpcRequest {
            id: 1,
            method: "manage_interface".into(),
            params: Some(json!({"operation":"attach","name":"repair"})),
        })
    })
    .await
    .expect("RPC task")
    .expect("attach RPC");
    assert!(response.error.is_none(), "{response:?}");
    let attached = context.daemon.interface_records();
    assert!(attached[0].enabled);
    assert!(attached[0].settings.as_ref().expect("settings")["_runtime"]["iface"].is_string());
    assert_eq!(
        attached[0].settings.as_ref().expect("settings")["_runtime"]["pipe"]["status"]
            ["process_state"],
        "running"
    );
    context
        .interface_owner
        .as_mut()
        .expect("owner")
        .shutdown()
        .await
        .expect("repaired worker joined");
}
