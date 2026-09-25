#[cfg(unix)]
use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::process::{Command, Stdio};

#[cfg(unix)]
#[derive(serde::Deserialize)]
struct PythonKissPtyBridge {
    rust_device: String,
    python_device: String,
}

#[cfg(unix)]
fn start_python_kiss_pty_bridge() -> (PythonKissPtyBridge, ChildGuard) {
    let python_bin = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let helper = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/support/python_kiss_pty_bridge.py");
    let child = Command::new(python_bin)
        .arg("-u")
        .arg(helper)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn Python KISS PTY bridge");
    let mut guard = ChildGuard { child: Some(child) };
    let stdout = guard
        .child
        .as_mut()
        .and_then(|child| child.stdout.take())
        .expect("Python KISS PTY bridge stdout");
    let line = BufReader::new(stdout)
        .lines()
        .next()
        .expect("Python KISS PTY bridge should report its devices")
        .expect("read Python KISS PTY bridge devices");
    let bridge = serde_json::from_str(&line).expect("parse Python KISS PTY bridge devices");
    (bridge, guard)
}

#[cfg(unix)]
fn write_python_kiss_ifac_config(dir: &std::path::Path, serial_device: &str) {
    fs::write(
        dir.join("config"),
        format!(
            "[reticulum]\nenable_transport = no\nshare_instance = no\n\n\
             [logging]\nloglevel = 7\n\n\
             [interfaces]\n  [[KISS IFAC Interface]]\n    type = KISSInterface\n\
             enabled = yes\n    port = {serial_device}\n    speed = 9600\n\
             networkname = {IFAC_NETWORK_NAME}\n    passphrase = {IFAC_PASSPHRASE}\n\
             ifac_size = {IFAC_SIZE_BITS}\n"
        ),
    )
    .expect("write Python KISS IFAC config");
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires the pinned Python Reticulum checkout and pyserial"]
async fn python_rust_ifac_kiss_serial_channel_roundtrip_over_pty() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-ifac-kiss-serial");
    fs::create_dir_all(&py_config_dir).expect("Python KISS config directory");

    let (bridge, _bridge_guard) = start_python_kiss_pty_bridge();
    write_python_kiss_ifac_config(&py_config_dir, &bridge.python_device);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-rust-ifac-kiss-serial-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let transport = Transport::new(config);
    let interface = rns_transport::iface::kiss::KissInterface::new(&bridge.rust_device, 9_600);
    let iface_manager = transport.iface_manager();
    let address = {
        let mut manager = iface_manager.lock().await;
        let address = manager
            .spawn(interface, rns_transport::iface::kiss::KissInterface::spawn);
        assert!(manager
            .try_set_shared_config(address, ifac_shared_config())
            .expect("configure Rust KISS serial IFAC"));
        address
    };

    let child = paths.spawn_endpoint(&py_config_dir, "channel");
    let mut python_guard = ChildGuard { child: Some(child) };
    let ready = read_ready(python_guard.child.as_mut().expect("Python endpoint child"))
        .expect("Python KISS endpoint ready");
    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let destination =
        wait_for_announce(&transport, target_hash, Duration::from_secs(20)).await;
    let mut link_events = transport.out_link_events();
    let link = transport.link(destination).await;
    let link_id =
        wait_for_out_link_active(&mut link_events, &link, Duration::from_secs(15)).await;

    let seen = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
    let seen_clone = seen.clone();
    let channel = transport.channel(link_id);
    channel
        .register_handler(MSG_TYPE, move |envelope| {
            if let Ok(decoded) = rmp_serde::from_slice::<(String, String)>(&envelope.payload) {
                seen_clone.lock().expect("seen lock").push(decoded);
                true
            } else {
                false
            }
        })
        .await
        .expect("register Python KISS Channel reply handler");

    let payload = rmp_serde::to_vec(&(
        String::from("rust-ifac-kiss"),
        String::from("hello-python-kiss"),
    ))
    .expect("encode KISS IFAC Channel message");
    let sequence = channel.send(MSG_TYPE, payload).await.expect("send KISS IFAC message");
    wait_for_reply_tuple(
        &seen,
        Duration::from_secs(15),
        "rust-ifac-kiss",
        "reply:hello-python-kiss",
    )
    .await;
    wait_for_channel_delivery(&transport, link_id, sequence).await;

    let snapshots = transport.interface_traffic_snapshots().await;
    let snapshot = snapshots
        .iter()
        .find(|snapshot| snapshot.address == address)
        .expect("Rust KISS interface traffic snapshot");
    assert!(snapshot.rx_bytes > 0, "Python KISS frames reach Rust");
    assert!(snapshot.tx_bytes > 0, "Rust KISS frames reach Python");
    assert_eq!(snapshot.ifac_violations, 0, "matching IFAC credentials are accepted");
}
