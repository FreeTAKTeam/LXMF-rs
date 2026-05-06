use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use rand_core::OsRng;
use rns_core::identity::PrivateIdentity;
use rns_transport::channel_buffer::Buffer;
use rns_transport::destination::link::{LinkEvent, LinkStatus};
use rns_transport::destination::{DestinationDesc, DestinationName};
use rns_transport::hash::{address_hash, AddressHash};
use rns_transport::identity::{lxmf_sign, lxmf_verify, Identity, PUBLIC_KEY_LENGTH};
use rns_transport::identity_bridge::to_transport_private_identity;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use rns_transport::resource::ResourceEventKind;
use rns_transport::transport::{SendPacketOutcome, Transport, TransportConfig};
use tokio::time::{sleep, timeout, Instant};

const MSG_TYPE: u16 = 0xABCD;
const SIGNATURE_LENGTH: usize = ed25519_dalek::SIGNATURE_LENGTH;

struct ChildGuard {
    child: Option<Child>,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

static PYTHON_INTEROP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn python_interop_guard() -> tokio::sync::MutexGuard<'static, ()> {
    PYTHON_INTEROP_LOCK.lock().await
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_channel_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child =
        spawn_python_endpoint(&python_bin, &reticulum_py_repo, &helper, &py_config_dir, "channel");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-channel-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
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

    let channel = transport.channel(link_id);
    let seen = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
    let seen_clone = seen.clone();
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
        .expect("register channel handler");

    let payload = rmp_serde::to_vec(&(String::from("rust-1"), String::from("hello-python")))
        .expect("encode channel message");
    channel.send(MSG_TYPE, payload).await.expect("send channel message");

    wait_for_reply(&seen, Duration::from_secs(8)).await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_link_data_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-link-data");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = spawn_python_endpoint(
        &python_bin,
        &reticulum_py_repo,
        &helper,
        &py_config_dir,
        "link-data",
    );
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-link-data-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
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
    let mut received = transport.received_data_events();
    sleep(Duration::from_millis(100)).await;

    transport.send_to_out_links(&target_hash, b"hello-link-data").await;
    wait_for_link_data(&mut received, link_id, b"reply:hello-link-data", Duration::from_secs(8))
        .await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_request_response_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-request");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child =
        spawn_python_endpoint(&python_bin, &reticulum_py_repo, &helper, &py_config_dir, "request");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-request-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
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
    let mut received = transport.received_data_events();
    sleep(Duration::from_millis(100)).await;

    let payload = build_link_request_payload(
        "/test/request",
        rmpv::Value::String("hello-python-request".into()),
    )
    .expect("request payload");
    let request_id = send_link_context_packet(&transport, &link, PacketContext::Request, &payload)
        .await
        .expect("send request")
        .expect("request id");
    let response =
        wait_for_request_response(&mut received, link_id, request_id, Duration::from_secs(8)).await;
    assert_eq!(rmpv_to_string(&response).as_deref(), Some("reply:hello-python-request"));
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_resource_backed_request_response_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-large-request");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = spawn_python_endpoint(
        &python_bin,
        &reticulum_py_repo,
        &helper,
        &py_config_dir,
        "large-request",
    );
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-large-request-interop", &rust_identity, true);
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

    let request_text = format!("large:{}", "x".repeat(900));
    let packed_request = build_link_request_payload(
        "/test/request",
        rmpv::Value::String(request_text.clone().into()),
    )
    .expect("request payload");
    let request_id = address_hash(&packed_request);
    let mut resource_events = transport.resource_events();
    let request_hash = transport
        .send_request_resource(&link_id, request_id.to_vec(), packed_request, None)
        .await
        .expect("send large request resource");
    wait_for_outbound_resource_complete(&mut resource_events, request_hash, Duration::from_secs(8))
        .await;

    let response = wait_for_resource_response(
        &mut resource_events,
        link_id,
        request_id,
        Duration::from_secs(8),
    )
    .await;
    assert_eq!(
        rmpv_to_string(&response).as_deref(),
        Some(format!("reply:{request_text}").as_str())
    );
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_file_response_resource_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-file-response");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = spawn_python_endpoint(
        &python_bin,
        &reticulum_py_repo,
        &helper,
        &py_config_dir,
        "file-response",
    );
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-file-response-interop", &rust_identity, true);
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

    let payload =
        build_link_request_payload("/test/request", rmpv::Value::String("file-response".into()))
            .expect("request payload");
    let request_id = send_link_context_packet(&transport, &link, PacketContext::Request, &payload)
        .await
        .expect("send request")
        .expect("request id");
    let mut resource_events = transport.resource_events();
    let complete = wait_for_file_resource_response(
        &mut resource_events,
        link_id,
        request_id,
        Duration::from_secs(8),
    )
    .await;
    assert_eq!(complete.data, b"python-file-response");
    let metadata = complete.metadata.expect("file response metadata");
    let decoded: String = rmp_serde::from_slice(&metadata).expect("decode metadata");
    assert_eq!(decoded, "python-file-meta");
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_link_identify_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-identify");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child =
        spawn_python_endpoint(&python_bin, &reticulum_py_repo, &helper, &py_config_dir, "identify");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-identify-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
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

    let payload = build_link_identify_payload(&rust_identity, &link_id);
    send_link_context_packet(&transport, &link, PacketContext::LinkIdentify, &payload)
        .await
        .expect("send link identify");
    wait_for_identify_ack(&seen, Duration::from_secs(8)).await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_channel_buffer_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-buffer");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child =
        spawn_python_endpoint(&python_bin, &reticulum_py_repo, &helper, &py_config_dir, "buffer");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-buffer-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
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

    let pair = Buffer::create_bidirectional_buffer(0, 0, transport.channel(link_id))
        .await
        .expect("buffer pair");
    let written = pair.writer.write_all(b"Hi there").await.expect("write buffer");
    assert_eq!(written, "Hi there".len());

    wait_for_buffer_data(&pair.reader, b"Hi there back at you", Duration::from_secs(8)).await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_raw_resource_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child =
        spawn_python_endpoint(&python_bin, &reticulum_py_repo, &helper, &py_config_dir, "resource");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-interop", &rust_identity, true);
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

    let mut resource_events = transport.resource_events();
    let metadata = rmp_serde::to_vec(&String::from("rust-meta")).expect("metadata");
    let resource_hash = transport
        .send_resource(&link_id, b"rust-resource-data".to_vec(), Some(metadata))
        .await
        .expect("send resource");
    wait_for_outbound_resource_complete(
        &mut resource_events,
        resource_hash,
        Duration::from_secs(8),
    )
    .await;
    wait_for_resource_ack(&seen, Duration::from_secs(8)).await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_channel_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-channel-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let mut transport = Transport::new(config);
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
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };

    let child = spawn_python_channel_client(
        &python_bin,
        &reticulum_py_repo,
        &helper,
        &py_config_dir,
        &destination_hash,
        "channel",
    );
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    sleep(Duration::from_millis(50)).await;

    let channel = transport.channel(link_id);
    let seen = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
    let seen_clone = seen.clone();
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
        .expect("register channel handler");

    wait_for_python_message(&seen, Duration::from_secs(8)).await;
    let payload = rmp_serde::to_vec(&(String::from("python-1"), String::from("reply:hello-rust")))
        .expect("encode channel reply");
    channel.send(MSG_TYPE, payload).await.expect("send channel reply");

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python channel client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"reply:hello-rust\""),
        "python client did not report Rust channel reply: {stdout}"
    );
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_link_data_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-link-data-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-link-data-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let mut transport = Transport::new(config);
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
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };

    let child = spawn_python_channel_client(
        &python_bin,
        &reticulum_py_repo,
        &helper,
        &py_config_dir,
        &destination_hash,
        "link-data",
    );
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    let mut received = transport.received_data_events();
    wait_for_link_data(&mut received, link_id, b"hello-rust", Duration::from_secs(8)).await;
    let destination_hash = { destination.lock().await.desc.address_hash };
    transport.send_to_in_links(&destination_hash, b"reply:hello-rust").await;

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python link-data client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("reply:hello-rust"), "python client did not report link-data reply");
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_request_response_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-request-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-request-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let mut transport = Transport::new(config);
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
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };

    let child = spawn_python_channel_client(
        &python_bin,
        &reticulum_py_repo,
        &helper,
        &py_config_dir,
        &destination_hash,
        "request",
    );
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    let mut received = transport.received_data_events();
    let (request_id, request_data) =
        wait_for_request(&mut received, link_id, Duration::from_secs(8)).await;
    assert_eq!(rmpv_to_string(&request_data).as_deref(), Some("hello-rust"));
    send_link_response(
        &transport,
        link_id,
        request_id,
        rmpv::Value::String("reply:hello-rust".into()),
    )
    .await
    .expect("send request response");

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python request client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("reply:hello-rust"), "python client did not report request response");
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_resource_backed_request_response_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-large-request-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-large-request-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let mut transport = Transport::new(config);
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
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };

    let child = spawn_python_channel_client(
        &python_bin,
        &reticulum_py_repo,
        &helper,
        &py_config_dir,
        &destination_hash,
        "large-request",
    );
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    let mut resource_events = transport.resource_events();
    let packed_request = wait_for_inbound_resource_data_or_child_exit(
        &mut resource_events,
        link_id,
        guard.child.as_mut().expect("python child"),
        Duration::from_secs(8),
    )
    .await;
    let request_data = parse_request_payload(&packed_request).expect("large request payload");
    let request_text = rmpv_to_string(&request_data).expect("large request text");
    assert!(request_text.starts_with("large:"));
    assert!(request_text.len() > 900);

    let mut request_id = [0u8; 16];
    request_id.copy_from_slice(&address_hash(&packed_request));
    let response_payload = rmp_serde::to_vec(&rmpv::Value::Array(vec![
        rmpv::Value::Binary(request_id.to_vec()),
        rmpv::Value::String(format!("reply:{request_text}").into()),
    ]))
    .expect("large response payload");
    let response_hash = transport
        .send_response_resource(&link_id, request_id.to_vec(), response_payload, None)
        .await
        .expect("send large response resource");
    wait_for_outbound_resource_complete(
        &mut resource_events,
        response_hash,
        Duration::from_secs(8),
    )
    .await;

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python large request client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"reply:large:"),
        "python client did not report large request response"
    );
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_link_identify_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-identify-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-identify-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let mut transport = Transport::new(config);
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
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };

    let child = spawn_python_channel_client(
        &python_bin,
        &reticulum_py_repo,
        &helper,
        &py_config_dir,
        &destination_hash,
        "identify",
    );
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    let mut received = transport.received_data_events();
    let remote_identity =
        wait_for_link_identify(&mut received, link_id, Duration::from_secs(8)).await;
    assert_ne!(remote_identity.address_hash, *rust_identity.address_hash());

    let destination_hash = { destination.lock().await.desc.address_hash };
    transport.send_to_in_links(&destination_hash, b"reply:identified").await;

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python identify client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"identified\""), "python client did not report identify ack");
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_channel_buffer_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-buffer-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-buffer-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let mut transport = Transport::new(config);
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
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };

    let child = spawn_python_channel_client(
        &python_bin,
        &reticulum_py_repo,
        &helper,
        &py_config_dir,
        &destination_hash,
        "buffer",
    );
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    sleep(Duration::from_millis(50)).await;

    let pair = Buffer::create_bidirectional_buffer(0, 0, transport.channel(link_id))
        .await
        .expect("buffer pair");
    wait_for_buffer_data(&pair.reader, b"hello-rust", Duration::from_secs(8)).await;
    let written = pair.writer.write_all(b"hello-rust back at you").await.expect("write reply");
    assert_eq!(written, "hello-rust back at you".len());

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python buffer client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello-rust back at you"),
        "python client did not report Rust buffer reply: {stdout}"
    );
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_raw_resource_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-resource-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let mut transport = Transport::new(config);
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
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };

    let child = spawn_python_channel_client(
        &python_bin,
        &reticulum_py_repo,
        &helper,
        &py_config_dir,
        &destination_hash,
        "resource",
    );
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let _link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;

    let mut resource_events = transport.resource_events();
    wait_for_inbound_resource_complete(
        &mut resource_events,
        b"hello-rust",
        "python-meta",
        Duration::from_secs(8),
    )
    .await;

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python resource client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"complete\""), "python client did not report resource completion");
}

#[derive(serde::Deserialize)]
struct ReadyLine {
    ready: bool,
    destination_hash: String,
}

fn spawn_python_endpoint(
    python_bin: &str,
    reticulum_py_repo: &Path,
    helper: &Path,
    config_dir: &Path,
    payload_kind: &str,
) -> Child {
    Command::new(python_bin)
        .arg("-u")
        .arg(helper)
        .arg("--payload-kind")
        .arg(payload_kind)
        .arg("--config-dir")
        .arg(config_dir)
        .env("PYTHONPATH", reticulum_py_repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn python endpoint")
}

fn spawn_python_channel_client(
    python_bin: &str,
    reticulum_py_repo: &Path,
    helper: &Path,
    config_dir: &Path,
    destination_hash: &str,
    payload_kind: &str,
) -> Child {
    Command::new(python_bin)
        .arg("-u")
        .arg(helper)
        .arg("--mode")
        .arg("client")
        .arg("--payload-kind")
        .arg(payload_kind)
        .arg("--config-dir")
        .arg(config_dir)
        .arg("--destination-hash")
        .arg(destination_hash)
        .arg("--message-id")
        .arg("python-1")
        .arg("--message-data")
        .arg("hello-rust")
        .env("PYTHONPATH", reticulum_py_repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn python channel client")
}

fn read_ready(child: &mut Child) -> Option<ReadyLine> {
    let stdout = child.stdout.take().expect("python stdout");
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = line.expect("read ready line");
        if let Ok(ready) = serde_json::from_str::<ReadyLine>(&line) {
            if ready.ready {
                return Some(ready);
            }
        }
    }
    None
}

async fn wait_for_announce(
    transport: &Transport,
    target_hash: AddressHash,
    duration: Duration,
) -> DestinationDesc {
    let mut announces = transport.recv_announces().await;
    timeout(duration, async {
        loop {
            let event = announces.recv().await.expect("announce event");
            let destination = event.destination.lock().await.desc;
            if destination.address_hash == target_hash {
                return destination;
            }
        }
    })
    .await
    .expect("timed out waiting for Python announce")
}

async fn wait_for_out_link_active(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::destination::link::LinkEventData>,
    link: &Arc<tokio::sync::Mutex<rns_transport::destination::link::Link>>,
    duration: Duration,
) -> AddressHash {
    let expected = *link.lock().await.id();
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("link event");
            if event.id == expected && matches!(event.event, LinkEvent::Activated) {
                assert_eq!(link.lock().await.status(), LinkStatus::Active);
                return expected;
            }
        }
    })
    .await
    .expect("timed out waiting for Rust link activation")
}

async fn wait_for_in_link_active_with_announces(
    transport: &Transport,
    destination: &Arc<tokio::sync::Mutex<rns_transport::destination::SingleInputDestination>>,
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::destination::link::LinkEventData>,
    duration: Duration,
) -> AddressHash {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        transport.send_announce(destination, None).await;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let slice = remaining.min(Duration::from_millis(250));
        match timeout(slice, events.recv()).await {
            Ok(Ok(event)) if matches!(event.event, LinkEvent::Activated) => return event.id,
            Ok(Ok(_)) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                panic!("inbound link event channel closed")
            }
            Err(_) => {}
        }
    }
    panic!("timed out waiting for Python-initiated Rust link activation")
}

async fn wait_for_reply(seen: &Arc<StdMutex<Vec<(String, String)>>>, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        {
            let seen = seen.lock().expect("seen lock");
            if seen.iter().any(|(id, data)| id == "rust-1" && data == "reply:hello-python") {
                return;
            }
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for Python channel reply");
}

async fn wait_for_python_message(seen: &Arc<StdMutex<Vec<(String, String)>>>, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        {
            let seen = seen.lock().expect("seen lock");
            if seen.iter().any(|(id, data)| id == "python-1" && data == "hello-rust") {
                return;
            }
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for Python channel message");
}

async fn wait_for_buffer_data(
    reader: &rns_transport::channel_buffer::RawChannelReader,
    expected: &[u8],
    duration: Duration,
) {
    let deadline = Instant::now() + duration;
    let mut received = Vec::new();
    while Instant::now() < deadline {
        if let Some(chunk) = reader.read(usize::MAX) {
            received.extend_from_slice(&chunk);
            if received.windows(expected.len()).any(|window| window == expected) {
                return;
            }
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "timed out waiting for buffer data {:?}; received {:?}",
        String::from_utf8_lossy(expected),
        String::from_utf8_lossy(&received)
    );
}

async fn wait_for_outbound_resource_complete(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    expected_hash: rns_transport::hash::Hash,
    duration: Duration,
) {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("resource event");
            if event.hash == expected_hash
                && matches!(event.kind, ResourceEventKind::OutboundComplete)
            {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for outbound resource completion");
}

async fn wait_for_inbound_resource_complete(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    expected_data: &[u8],
    expected_metadata: &str,
    duration: Duration,
) {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("resource event");
            if let ResourceEventKind::Complete(complete) = event.kind {
                if complete.data == expected_data {
                    let metadata = complete.metadata.expect("resource metadata");
                    let decoded: String =
                        rmp_serde::from_slice(&metadata).expect("decode resource metadata");
                    assert_eq!(decoded, expected_metadata);
                    return;
                }
            }
        }
    })
    .await
    .expect("timed out waiting for inbound resource completion");
}

async fn wait_for_inbound_resource_data_or_child_exit(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    link_id: AddressHash,
    child: &mut Child,
    duration: Duration,
) -> Vec<u8> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        match timeout(Duration::from_millis(100), events.recv()).await {
            Ok(Ok(event)) if event.link_id == link_id => {
                if let ResourceEventKind::Complete(complete) = event.kind {
                    return complete.data;
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                panic!("resource event channel closed")
            }
            Err(_) => {}
        }
        if let Some(status) = child.try_wait().expect("poll python child") {
            let mut stdout = String::new();
            if let Some(mut pipe) = child.stdout.take() {
                let _ = pipe.read_to_string(&mut stdout);
            }
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            panic!(
                "python child exited before resource completion: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
            );
        }
    }
    let _ = child.kill();
    let status = child.wait().expect("wait for timed-out python child");
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    panic!(
        "timed out waiting for inbound resource data; python child status after kill: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

async fn wait_for_resource_ack(seen: &Arc<StdMutex<Vec<(String, String)>>>, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        {
            let seen = seen.lock().expect("seen lock");
            if seen.iter().any(|(id, data)| {
                id == "rust-resource" && data == "resource:rust-resource-data:rust-meta"
            }) {
                return;
            }
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for Python resource acknowledgement");
}

async fn wait_for_identify_ack(seen: &Arc<StdMutex<Vec<(String, String)>>>, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        {
            let seen = seen.lock().expect("seen lock");
            if seen
                .iter()
                .any(|(id, data)| id == "rust-identify" && data.starts_with("identified:"))
            {
                return;
            }
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for Python identify acknowledgement");
}

async fn wait_for_link_data(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    link_id: AddressHash,
    expected: &[u8],
    duration: Duration,
) {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("received data event");
            if event.destination == link_id && event.data.as_slice() == expected {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for link data");
}

async fn wait_for_link_identify(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    link_id: AddressHash,
    duration: Duration,
) -> Identity {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("received data event");
            if event.destination != link_id || event.context != Some(PacketContext::LinkIdentify) {
                continue;
            }
            if let Some(identity) = parse_link_identify_payload(link_id, event.data.as_slice()) {
                return identity;
            }
        }
    })
    .await
    .expect("timed out waiting for link identify")
}

fn build_link_identify_payload(
    identity: &rns_transport::identity::PrivateIdentity,
    link_id: &AddressHash,
) -> Vec<u8> {
    let mut public_key = Vec::with_capacity(PUBLIC_KEY_LENGTH * 2);
    public_key.extend_from_slice(identity.as_identity().public_key_bytes());
    public_key.extend_from_slice(identity.as_identity().verifying_key_bytes());

    let mut signed_data = Vec::with_capacity(link_id.as_slice().len() + public_key.len());
    signed_data.extend_from_slice(link_id.as_slice());
    signed_data.extend_from_slice(&public_key);

    let signature = lxmf_sign(identity, &signed_data);
    let mut payload = public_key;
    payload.extend_from_slice(&signature);
    payload
}

fn parse_link_identify_payload(link_id: AddressHash, bytes: &[u8]) -> Option<Identity> {
    if bytes.len() != PUBLIC_KEY_LENGTH * 2 + SIGNATURE_LENGTH {
        return None;
    }
    let public_key = &bytes[..PUBLIC_KEY_LENGTH];
    let verifying_key = &bytes[PUBLIC_KEY_LENGTH..PUBLIC_KEY_LENGTH * 2];
    let signature = &bytes[PUBLIC_KEY_LENGTH * 2..];
    let identity = Identity::new_from_slices(public_key, verifying_key);

    let mut signed_data = Vec::with_capacity(link_id.as_slice().len() + PUBLIC_KEY_LENGTH * 2);
    signed_data.extend_from_slice(link_id.as_slice());
    signed_data.extend_from_slice(&bytes[..PUBLIC_KEY_LENGTH * 2]);

    lxmf_verify(&identity, &signed_data, signature).then_some(identity)
}

fn build_link_request_payload(path: &str, data: rmpv::Value) -> Result<Vec<u8>, std::io::Error> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let path_hash = address_hash(path.as_bytes());
    rmp_serde::to_vec(&rmpv::Value::Array(vec![
        rmpv::Value::F64(timestamp),
        rmpv::Value::Binary(path_hash.to_vec()),
        data,
    ]))
    .map_err(std::io::Error::other)
}

async fn send_link_context_packet(
    transport: &Transport,
    link: &Arc<tokio::sync::Mutex<rns_transport::destination::link::Link>>,
    context: PacketContext,
    payload: &[u8],
) -> Result<Option<[u8; 16]>, std::io::Error> {
    let packet = {
        let guard = link.lock().await;
        let mut packet_data = PacketDataBuffer::new();
        let cipher_len = {
            let ciphertext = guard
                .encrypt(payload, packet_data.accuire_buf_max())
                .map_err(|_| std::io::Error::other("failed to encrypt link packet"))?;
            ciphertext.len()
        };
        packet_data.resize(cipher_len);

        Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination: *guard.id(),
            transport: None,
            context,
            data: packet_data,
        }
    };

    let request_id = if context == PacketContext::Request {
        let hash = packet.hash().to_bytes();
        let mut request_id = [0u8; 16];
        request_id.copy_from_slice(&hash[..16]);
        Some(request_id)
    } else {
        None
    };

    match transport.send_packet_with_outcome(packet).await {
        SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast => Ok(request_id),
        other => Err(std::io::Error::other(format!("request packet not sent: {other:?}"))),
    }
}

async fn send_link_response(
    transport: &Transport,
    link_id: AddressHash,
    request_id: [u8; 16],
    response: rmpv::Value,
) -> Result<(), std::io::Error> {
    let link = transport
        .find_in_link(&link_id)
        .await
        .ok_or_else(|| std::io::Error::other("inbound link not found"))?;
    let frame = rmpv::Value::Array(vec![rmpv::Value::Binary(request_id.to_vec()), response]);
    let payload = rmp_serde::to_vec(&frame).map_err(std::io::Error::other)?;
    let (packet, iface) = {
        let guard = link.lock().await;
        let iface = guard
            .ingress_iface()
            .ok_or_else(|| std::io::Error::other("inbound link ingress iface missing"))?;
        let mut packet_data = PacketDataBuffer::new();
        let cipher_len = {
            let ciphertext = guard
                .encrypt(payload.as_slice(), packet_data.accuire_buf_max())
                .map_err(|_| std::io::Error::other("failed to encrypt response"))?;
            ciphertext.len()
        };
        packet_data.resize(cipher_len);
        (
            Packet {
                header: Header {
                    ifac_flag: IfacFlag::Open,
                    header_type: HeaderType::Type1,
                    context_flag: ContextFlag::Unset,
                    propagation_type: PropagationType::Broadcast,
                    destination_type: DestinationType::Link,
                    packet_type: PacketType::Data,
                    hops: 0,
                },
                ifac: None,
                destination: *guard.id(),
                transport: None,
                context: PacketContext::Response,
                data: packet_data,
            },
            iface,
        )
    };
    transport.send_direct(iface, packet).await;
    Ok(())
}

async fn wait_for_request_response(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    link_id: AddressHash,
    request_id: [u8; 16],
    duration: Duration,
) -> rmpv::Value {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("received data event");
            if event.destination != link_id {
                continue;
            }
            if let Some((response_id, response)) =
                parse_request_response_frame(event.data.as_slice())
            {
                if response_id == request_id {
                    return response;
                }
            }
        }
    })
    .await
    .expect("timed out waiting for request response")
}

async fn wait_for_resource_response(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    link_id: AddressHash,
    request_id: [u8; 16],
    duration: Duration,
) -> rmpv::Value {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("resource event");
            if event.link_id != link_id {
                continue;
            }
            if let ResourceEventKind::Complete(complete) = event.kind {
                if let Some((response_id, response)) =
                    parse_request_response_frame(complete.data.as_slice())
                {
                    if response_id == request_id {
                        return response;
                    }
                }
            }
        }
    })
    .await
    .expect("timed out waiting for resource response")
}

async fn wait_for_file_resource_response(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    link_id: AddressHash,
    request_id: [u8; 16],
    duration: Duration,
) -> rns_transport::resource::ResourceComplete {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("resource event");
            if event.link_id != link_id {
                continue;
            }
            if let ResourceEventKind::Complete(complete) = event.kind {
                if complete.is_response
                    && !complete.is_request
                    && complete.request_id.as_deref() == Some(request_id.as_slice())
                {
                    return complete;
                }
            }
        }
    })
    .await
    .expect("timed out waiting for file resource response")
}

async fn wait_for_request(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    link_id: AddressHash,
    duration: Duration,
) -> ([u8; 16], rmpv::Value) {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("received data event");
            if event.destination != link_id || event.context != Some(PacketContext::Request) {
                continue;
            }
            let Some(request_id) = event.request_id else {
                continue;
            };
            let Some(data) = parse_request_payload(event.data.as_slice()) else {
                continue;
            };
            return (request_id, data);
        }
    })
    .await
    .expect("timed out waiting for request")
}

fn parse_request_payload(bytes: &[u8]) -> Option<rmpv::Value> {
    let value = rmp_serde::from_slice::<rmpv::Value>(bytes).ok()?;
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() != 3 {
        return None;
    }
    entries.get(2).cloned()
}

fn parse_request_response_frame(bytes: &[u8]) -> Option<([u8; 16], rmpv::Value)> {
    let value = rmp_serde::from_slice::<rmpv::Value>(bytes).ok()?;
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() != 2 {
        return None;
    }
    let rmpv::Value::Binary(request_bytes) = entries.first()? else {
        return None;
    };
    if request_bytes.len() != 16 {
        return None;
    }
    let mut request_id = [0u8; 16];
    request_id.copy_from_slice(request_bytes.as_slice());
    Some((request_id, entries.get(1)?.clone()))
}

fn rmpv_to_string(value: &rmpv::Value) -> Option<String> {
    match value {
        rmpv::Value::String(text) => text.as_str().map(ToOwned::to_owned),
        rmpv::Value::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}

async fn wait_for_port(port: u16, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for Python TCP server on port {port}");
}

fn write_python_config(dir: &Path, port: u16) {
    let config = format!(
        "[reticulum]\n\
         enable_transport = no\n\
         share_instance = no\n\
         \n\
         [logging]\n\
         loglevel = 7\n\
         \n\
         [interfaces]\n\
           [[TCP Server Interface]]\n\
             type = TCPServerInterface\n\
             enabled = yes\n\
             listen_ip = 127.0.0.1\n\
             listen_port = {port}\n"
    );
    fs::write(dir.join("config"), config).expect("write python config");
}

fn write_python_client_config(dir: &Path, port: u16) {
    let config = format!(
        "[reticulum]\n\
         enable_transport = no\n\
         share_instance = no\n\
         \n\
         [logging]\n\
         loglevel = 7\n\
         \n\
         [interfaces]\n\
           [[TCP Client Interface]]\n\
             type = TCPClientInterface\n\
             enabled = yes\n\
             target_host = 127.0.0.1\n\
             target_port = {port}\n"
    );
    fs::write(dir.join("config"), config).expect("write python client config");
}

fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}
