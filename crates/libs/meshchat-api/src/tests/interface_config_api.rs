use super::*;

fn config_router(path: PathBuf) -> Router {
    let (events, _) = broadcast::channel(16);
    let state = Arc::new(AppState {
        daemon: Arc::new(RpcDaemon::test_instance()),
        local: Arc::new(RwLock::new(LocalState::default())),
        events,
        persist: None,
        config: Some(Arc::new(interface_control::ConfigStore::new(path))),
    });
    build_router(state, None)
}

#[tokio::test]
async fn saved_interfaces_and_transport_roundtrip_without_hot_apply() {
    let temp = tempfile::tempdir().expect("temp directory");
    let path = temp.path().join("daemon.toml");
    std::fs::write(&path, "# keep this comment\n[reticulum]\nenable_transport = false\n\n[[interfaces]]\nname = \"upstream\"\ntype = \"TCPClientInterface\"\ninterface_enabled = true\ntarget_host = \"rmap.world\"\ntarget_port = 4242\n").expect("fixture");
    let app = config_router(path.clone());
    let (status, listed) = call(app.clone(), "GET", "/api/v1/reticulum/interfaces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["interfaces"]["upstream"]["target_host"], "rmap.world");
    assert!(listed["interfaces"].is_object());

    let (status, _) = call(
        app.clone(),
        "POST",
        "/api/v1/reticulum/interfaces/disable",
        Some(json!({"name":"upstream"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(app.clone(), "POST", "/api/v1/reticulum/enable-transport", None).await;
    assert_eq!(status, StatusCode::OK);
    let contents = std::fs::read_to_string(&path).expect("saved config");
    assert!(contents.contains("# keep this comment"));
    assert!(contents.contains("interface_enabled = false"));
    assert!(contents.contains("enable_transport = true"));
    let (status, _) = call(app.clone(), "POST", "/api/v1/reticulum/interfaces/add", Some(json!({
        "name":"second", "type":"TCPClientInterface", "target_host":"127.0.0.1", "target_port":"4243"
    }))).await;
    assert_eq!(status, StatusCode::OK);
    let parsed: toml::Value =
        toml::from_str(&std::fs::read_to_string(&path).expect("saved config")).expect("valid TOML");
    assert_eq!(parsed["interfaces"][1]["target_port"].as_integer(), Some(4243));
    let (status, _) = call(
        app.clone(),
        "POST",
        "/api/v1/reticulum/interfaces/delete",
        Some(json!({"name":"second"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, listed) = call(app, "GET", "/api/v1/reticulum/interfaces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(listed["interfaces"].get("second").is_none());
}

#[tokio::test]
async fn named_tables_and_import_preview_are_supported() {
    let temp = tempfile::tempdir().expect("temp directory");
    let path = temp.path().join("daemon.toml");
    std::fs::write(&path, "[interfaces.radio]\ntype = \"RNodeInterface\"\ninterface_enabled = true\nport = \"/dev/ttyUSB1\"\nfrequency = 915000000\nbandwidth = 500000\ntxpower = 22\nspreadingfactor = 10\ncodingrate = 5\n").expect("fixture");
    let app = config_router(path.clone());
    let (status, listed) = call(app.clone(), "GET", "/api/v1/reticulum/interfaces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["interfaces"]["radio"]["frequency"], 915000000);
    let text =
        "[[relay]]\ntype = TCPClientInterface\ntarget_host = rmap.world\ntarget_port = 4242\n";
    let (status, preview) = call(
        app.clone(),
        "POST",
        "/api/v1/reticulum/interfaces/import-preview",
        Some(json!({"config":text})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["interfaces"][0]["name"], "relay");
    let (status, _) = call(
        app.clone(),
        "POST",
        "/api/v1/reticulum/interfaces/import",
        Some(json!({"config":text,"selected_interface_names":["relay"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, listed) = call(app, "GET", "/api/v1/reticulum/interfaces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["interfaces"]["relay"]["interface_enabled"], true);
}

#[tokio::test]
async fn interface_writes_fail_without_config_or_when_missing() {
    let app = router(Arc::new(RpcDaemon::test_instance()), None);
    let (status, _) = call(app, "POST", "/api/v1/reticulum/enable-transport", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let temp = tempfile::tempdir().expect("temp directory");
    let path = temp.path().join("daemon.toml");
    std::fs::write(&path, "[reticulum]\nenable_transport = false\n").expect("fixture");
    let app = config_router(path.clone());
    let (status, _) =
        call(app, "POST", "/api/v1/reticulum/interfaces/enable", Some(json!({"name":"missing"})))
            .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        std::fs::read_to_string(path).expect("config"),
        "[reticulum]\nenable_transport = false\n"
    );
}

#[tokio::test]
async fn interface_export_selection_and_invalid_write_preserve_config() {
    let temp = tempfile::tempdir().expect("temp directory");
    let path = temp.path().join("daemon.toml");
    let source = "# original\n[interfaces.first]\ntype = \"TCPClientInterface\"\ninterface_enabled = true\ntarget_host = \"rmap.world\"\ntarget_port = 4242\n\n[interfaces.second]\ntype = \"AutoInterface\"\n";
    std::fs::write(&path, source).expect("fixture");
    let app = config_router(path.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/reticulum/interfaces/export")
        .header("content-type", "application/json")
        .body(Body::from(json!({"selected_interface_names":["first"]}).to_string()))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=meshchat_interfaces"
    );
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.expect("export body");
    let export = String::from_utf8(bytes.to_vec()).expect("export UTF-8");
    assert!(export.contains("[[first]]"));
    assert!(!export.contains("[[second]]"));
    let (status, preview) = call(
        app.clone(),
        "POST",
        "/api/v1/reticulum/interfaces/import-preview",
        Some(json!({"config":export})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["interfaces"][0]["name"], "first");
    let (status, _) = call(app, "POST", "/api/v1/reticulum/interfaces/add", Some(json!({
        "name":"bad", "type":"TCPClientInterface", "target_host":"127.0.0.1", "target_port":"invalid"
    }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(std::fs::read_to_string(path).expect("config"), source);
}

#[tokio::test]
async fn rnode_multi_subinterfaces_survive_save_and_read() {
    let temp = tempfile::tempdir().expect("temp directory");
    let path = temp.path().join("daemon.toml");
    std::fs::write(&path, "[reticulum]\nenable_transport = false\n").expect("fixture");
    let app = config_router(path.clone());
    let (status, _) = call(
        app.clone(),
        "POST",
        "/api/v1/reticulum/interfaces/add",
        Some(json!({
            "name":"multi", "type":"RNodeMultiInterface", "port":"tcp://127.0.0.1:4242",
            "sub_interfaces":[{"name":"left", "frequency":915000000, "bandwidth":500000,
                "txpower":22, "spreadingfactor":10, "codingrate":5, "vport":1}]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let source = std::fs::read_to_string(&path).expect("saved config");
    let parsed: toml::Value = toml::from_str(&source).expect("valid TOML");
    assert_eq!(parsed["interfaces"][0]["left"]["vport"].as_integer(), Some(1));
    let (status, listed) = call(app, "GET", "/api/v1/reticulum/interfaces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["interfaces"]["multi"]["sub_interfaces"][0]["name"], "left");
}

struct DiscoveryBridge;

impl rns_rpc::PathLookupBridge for DiscoveryBridge {
    fn has_path(&self, _destination: &str) -> Result<bool, std::io::Error> {
        Ok(false)
    }

    fn request_path(&self, _destination: &str) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn discovered_interfaces(&self) -> Result<Value, std::io::Error> {
        Ok(json!([{
            "discovery_hash":[171, 205], "name":"Hub", "type":"BackboneInterface", "transport":true,
            "status":"available", "reachable_on":"hub.example", "port":4242, "hops":2,
            "network_id":"net", "transport_id":"tid", "stamp":[1, 2, 3], "value":16,
        }]))
    }
}

#[tokio::test]
async fn discovered_interfaces_and_discovery_policy_roundtrip() {
    let temp = tempfile::tempdir().expect("temp directory");
    let path = temp.path().join("daemon.toml");
    std::fs::write(&path, "[reticulum]\nenable_transport = true\n").expect("fixture");
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(DiscoveryBridge));
    let (events, _) = broadcast::channel(16);
    let app = build_router(
        Arc::new(AppState {
            daemon: Arc::new(daemon),
            local: Arc::new(RwLock::new(LocalState::default())),
            events,
            persist: None,
            config: Some(Arc::new(interface_control::ConfigStore::new(path.clone()))),
        }),
        None,
    );

    let (status, listed) =
        call(app.clone(), "GET", "/api/v1/reticulum/discovered-interfaces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["discovery_enabled"], false);
    assert_eq!(listed["interfaces"][0]["discovery_hash"], "abcd");
    assert_eq!(listed["interfaces"][0]["reachable_on"], "hub.example");
    assert!(listed["interfaces"][0].get("stamp").is_none());

    let (status, _) = call(app.clone(), "POST", "/api/v1/reticulum/enable-discovery", None).await;
    assert_eq!(status, StatusCode::OK);
    let contents = std::fs::read_to_string(&path).expect("saved config");
    assert!(contents.contains("enable_transport = true"));
    assert!(contents.contains("discover_interfaces = true"));
    let (_, listed) = call(app, "GET", "/api/v1/reticulum/discovered-interfaces", None).await;
    assert_eq!(listed["discovery_enabled"], true);
}

#[tokio::test]
async fn display_name_patch_reaches_daemon_and_rejects_empty_names() {
    let temp = tempfile::tempdir().expect("temp directory");
    let app = config_router(temp.path().join("daemon.toml"));
    let (status, saved) = call(
        app.clone(),
        "PATCH",
        "/api/v1/config",
        Some(json!({"config":{"display_name":"Corvo"}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["config"]["display_name"], "Corvo");

    let (status, _) =
        call(app, "PATCH", "/api/v1/config", Some(json!({"config":{"display_name":"  "}}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
