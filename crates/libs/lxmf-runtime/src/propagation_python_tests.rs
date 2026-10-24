use super::propagation_fetch::{fetch_retained, PropagationFetchLimits};
use super::*;
use lxmf_core::{message::Payload, WireMessage};
use rand_core::OsRng;
use rns_transport::{
    destination::{DestinationName, SingleOutputDestination},
    identity::PrivateIdentity,
    iface::tcp_client::TcpClient,
    transport::{Transport, TransportConfig},
};
use std::{
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::runtime::Handle;

struct Python(Child);
impl Drop for Python {
    fn drop(&mut self) {
        if let Err(error) = self.0.kill() {
            eprintln!("Python fixture cleanup: {error}");
        }
        if let Err(error) = self.0.wait() {
            eprintln!("Python fixture wait: {error}");
        }
    }
}
async fn wait_file(path: &Path) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("Python fixture readiness");
}

#[tokio::test]
#[ignore = "requires pinned Python RNS/LXMF refs on PYTHONPATH"]
async fn native_recipient_retains_failed_import_then_confirms_stock_python_relay() {
    let root = tempfile::tempdir().expect("fixture");
    let recipient = PrivateIdentity::new_from_rand(OsRng);
    let owner = lxmf_core::identity::PrivateIdentity::from_private_key_bytes(
        &recipient.to_private_key_bytes(),
    )
    .expect("identity");
    let destination = SingleOutputDestination::new(
        *recipient.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )
    .desc
    .address_hash;
    let mut wire = WireMessage::new(
        destination.as_slice().try_into().expect("destination"),
        destination.as_slice().try_into().expect("source"),
        Payload::new(
            1.0,
            Some(b"embedded Python relay interoperability".to_vec()),
            None,
            None,
            None,
        ),
    );
    wire.sign(&owner).expect("sign");
    let packed = wire.pack().expect("wire");
    let transient =
        wire.pack_propagation_transient_with_rng(owner.as_identity(), OsRng).expect("transient").0;
    std::fs::write(root.path().join("transient"), transient).expect("ciphertext fixture");
    let socket = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    let address = socket.local_addr().expect("address");
    drop(socket);
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_fetch_relay.py");
    let _python = Python(
        Command::new(std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into()))
            .arg(script)
            .arg(root.path())
            .arg(address.port().to_string())
            .stdout(Stdio::null())
            .spawn()
            .expect("stock Python relay"),
    );
    wait_file(&root.path().join("ready.json")).await;
    let ready: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("ready.json")).expect("ready"))
            .expect("JSON");
    let relay = rns_transport::hash::AddressHash::new_from_hex_string(
        ready["relay"].as_str().expect("relay hash"),
    )
    .expect("hash");
    let transport =
        Arc::new(Transport::new(TransportConfig::new("native-python-fetch", &recipient, true)));
    let context =
        transport.iface_manager().lock().await.new_context(TcpClient::new(address.to_string()));
    let stop = context.channel.stop.clone();
    let tcp = tokio::spawn(TcpClient::spawn(context));
    tokio::time::timeout(Duration::from_secs(10), async {
        while transport.destination_identity(&relay).await.is_none() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("native relay announce");
    let config = Arc::new(InProcessBackendConfig::new(
        "interop",
        Handle::current(),
        transport,
        recipient,
        destination,
    ));
    let limits = PropagationFetchLimits {
        max_messages: 1,
        max_response_bytes: 2048,
        request_timeout: Duration::from_secs(10),
        link_timeout: Duration::from_secs(5),
        link_attempts: 1,
    };
    let first = fetch_retained(config.clone(), relay, limits, None, |_| async { false })
        .await
        .expect("retained first fetch");
    assert_eq!((first.fetched, first.imported, first.confirmed), (1, 0, 0));
    let saved = root.path().join("imported");
    let second = fetch_retained(config, relay, limits, None, move |wire| {
        assert_eq!(wire, packed);
        let mut file = std::fs::File::create(&saved).expect("durable import file");
        file.write_all(&wire).expect("persist wire");
        file.sync_all().expect("durable commit");
        std::future::ready(true)
    })
    .await
    .expect("confirmed second fetch");
    assert_eq!((second.fetched, second.imported, second.confirmed), (1, 1, 1));
    std::fs::write(root.path().join("done"), []).expect("completion");
    wait_file(&root.path().join("result.json")).await;
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("result.json")).expect("result"))
            .expect("JSON");
    assert_eq!(result["remaining"], 0);
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), tcp)
        .await
        .expect("native TCP stop")
        .expect("task");
}
