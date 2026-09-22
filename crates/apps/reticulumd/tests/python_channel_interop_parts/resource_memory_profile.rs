use std::io::{BufWriter, Write};
use std::path::Path;

use sha2::Sha256 as ResourceSha256;

const RESOURCE_SIZE: usize = 50 * 1024 * 1024;
const RESOURCE_MEMORY_BUDGET_KIB: u64 = 512 * 1024;

fn write_streamed_fixture(path: &Path, size: usize) -> String {
    let file = fs::File::create(path).expect("create streamed resource fixture");
    let mut writer = BufWriter::new(file);
    let mut hasher = ResourceSha256::new();
    let mut state = 0x6051_5eed_u64;
    let mut chunk = vec![0_u8; 64 * 1024];
    let mut remaining = size;

    while remaining > 0 {
        let count = remaining.min(chunk.len());
        for byte in &mut chunk[..count] {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = (state >> 24) as u8;
        }
        writer.write_all(&chunk[..count]).expect("write streamed resource fixture");
        hasher.update(&chunk[..count]);
        remaining -= count;
    }
    writer.flush().expect("flush streamed resource fixture");
    hex::encode(hasher.finalize())
}

fn assert_memory_budget(direction: &str, peak_rss_kib: u64) {
    assert!(
        peak_rss_kib <= RESOURCE_MEMORY_BUDGET_KIB,
        "{direction} exceeded the {RESOURCE_MEMORY_BUDGET_KIB} KiB process RSS budget: {peak_rss_kib} KiB"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires local Python Reticulum checkout and Linux /proc memory counters"]
async fn rust_reader_to_python_50_mib_peak_memory() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();
    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource-memory-server");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "resource");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let python_pid = child.id();
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-memory-rust-sender", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{server_port}")), TcpClient::spawn);

    let destination = wait_for_announce(&transport, target_hash, Duration::from_secs(8)).await;
    let mut link_events = transport.out_link_events();
    let link = transport.link(destination).await;
    let link_id = wait_for_out_link_active(&mut link_events, &link, Duration::from_secs(8)).await;
    sleep(Duration::from_millis(100)).await;

    let seen = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
    let seen_clone = seen.clone();
    transport
        .channel(link_id)
        .register_handler(MSG_TYPE, move |envelope| {
            if let Ok(decoded) = rmp_serde::from_slice::<(String, String)>(&envelope.payload) {
                seen_clone.lock().expect("seen lock").push(decoded);
                true
            } else {
                false
            }
        })
        .await
        .expect("register channel handler");

    let rust_baseline_rss_kib =
        current_process_peak_rss_kib().expect("Linux Rust peak RSS counter");
    let python_baseline_rss_kib =
        process_peak_rss_kib(python_pid).expect("Python endpoint peak RSS counter");
    let payload_path = temp.path().join("resource-memory.bin");
    let expected_digest = write_streamed_fixture(&payload_path, RESOURCE_SIZE);
    let reader = fs::File::open(&payload_path).expect("open streamed resource fixture");
    let mut resource_events = transport.resource_events();
    let resource_hash = transport
        .send_resource_from_reader(&link_id, reader, RESOURCE_SIZE as u64, None)
        .await
        .expect("send streamed resource");
    wait_for_outbound_resource_complete(&mut resource_events, resource_hash, Duration::from_secs(180))
        .await;
    wait_for_resource_digest_ack(
        &seen,
        RESOURCE_SIZE,
        &expected_digest,
        Duration::from_secs(180),
    )
    .await;

    let rust_peak_rss_kib = current_process_peak_rss_kib().expect("Rust peak RSS counter");
    let python_peak_rss_kib = process_peak_rss_kib(python_pid).expect("Python peak RSS counter");
    assert_memory_budget("Rust reader -> Python", rust_peak_rss_kib);
    assert_memory_budget("Python receiver", python_peak_rss_kib);
    println!(
        "resource_memory_profile {}",
        serde_json::json!({
            "direction": "Rust reader -> Python receiver",
            "bytes": RESOURCE_SIZE,
            "sha256": expected_digest,
            "rust_baseline_peak_rss_kib": rust_baseline_rss_kib,
            "rust_peak_rss_kib": rust_peak_rss_kib,
            "python_baseline_peak_rss_kib": python_baseline_rss_kib,
            "python_peak_rss_kib": python_peak_rss_kib,
            "budget_kib": RESOURCE_MEMORY_BUDGET_KIB,
        })
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires local Python Reticulum checkout and Linux /proc memory counters"]
async fn python_to_rust_50_mib_peak_memory() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();
    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-memory-rust-receiver", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpServer::new(format!("127.0.0.1:{server_port}"), iface_manager), TcpServer::spawn);
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let destination = transport
        .add_destination(rust_identity.clone(), DestinationName::new("test", "channel"))
        .await;
    let rust_baseline_rss_kib =
        current_process_peak_rss_kib().expect("Linux Rust peak RSS counter");
    let mut resource_events = transport.resource_events();
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };
    let py_config_dir = temp.path().join("python-rns-resource-memory-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);
    let child = paths.spawn_resource_client(&py_config_dir, &destination_hash, RESOURCE_SIZE, 180.0);
    let python_pid = child.id();
    let mut guard = ChildGuard { child: Some(child) };
    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(20),
    )
    .await;
    let complete = wait_for_inbound_resource_data(&mut resource_events, link_id, Duration::from_secs(180))
        .await;

    let child = guard.child.take().expect("Python resource client");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join Python resource client")
        .expect("wait for Python resource client");
    assert!(
        output.status.success(),
        "Python resource client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .expect("Python resource completion report");
    let expected_digest = digest_hex(&complete.data);
    assert_eq!(complete.data.len(), RESOURCE_SIZE);
    assert_eq!(report["resource"], "complete");
    assert_eq!(report["size"].as_u64(), Some(RESOURCE_SIZE as u64));
    assert_eq!(report["sha256"].as_str(), Some(expected_digest.as_str()));
    let python_peak_rss_kib = report["peak_rss_kib"]
        .as_u64()
        .expect("Python resource report peak RSS");
    let rust_peak_rss_kib = current_process_peak_rss_kib().expect("Rust peak RSS counter");
    assert_memory_budget("Python sender", python_peak_rss_kib);
    assert_memory_budget("Rust receiver", rust_peak_rss_kib);
    println!(
        "resource_memory_profile {}",
        serde_json::json!({
            "direction": "Python sender -> Rust receiver",
            "bytes": RESOURCE_SIZE,
            "sha256": expected_digest,
            "rust_baseline_peak_rss_kib": rust_baseline_rss_kib,
            "rust_peak_rss_kib": rust_peak_rss_kib,
            "python_peak_rss_kib": python_peak_rss_kib,
            "budget_kib": RESOURCE_MEMORY_BUDGET_KIB,
            "python_report": report,
            "python_pid": python_pid,
        })
    );
}
