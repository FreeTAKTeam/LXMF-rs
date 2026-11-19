//! RDO owner binding v1 network gate against Python Reticulum, both ways.
//! Ignored by default; run with
//! `RETICULUM_PY_REPO=/path/to/Reticulum cargo test -p rdo-core --all-features
//! --test python_interop -- --ignored --test-threads=1`.

mod common;

use std::io::{BufRead, BufReader};
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use rand_core::OsRng;
use rdo_core::cbor::{self, Value};
use rdo_core::lookup::announce_app_data;
use rdo_core::rns::{
    call, open_object_link, public_identity, register_object, resolve_object, system_clock,
    OwnerService,
};
use rdo_core::{Owner, Policy, PublicIdentity, Request, Revision, Status};
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::transport::{Transport, TransportConfig};
use serde_json::Value as Json;

const TIMEOUT: Duration = Duration::from_secs(25);

struct Fixture {
    owner_private: Vec<u8>,
    token: [u8; 16],
    destination: [u8; 16],
    genesis: Revision,
    successor: Revision,
}

fn fixture() -> Fixture {
    let fixtures = fixtures();
    let owner_public = owner(&fixtures).public();
    let property = fixtures["bindings"]
        .as_array()
        .expect("bindings")
        .iter()
        .find(|binding| binding["label"] == "Property")
        .expect("Property")
        .clone();
    let revision = |name: &str| {
        Revision::verify(&hex(&fixtures["revisions"][name]["cbor_hex"]), &owner_public)
            .expect("fixture revision")
    };
    Fixture {
        owner_private: hex(&fixtures["fixture_identity_private_hex"]),
        token: fixed(hex(&property["token_hex"])),
        destination: fixed(hex(&property["destination_hex"])),
        genesis: revision("property_genesis"),
        successor: revision("property_successor"),
    }
}

struct Killer(Child);

impl Drop for Killer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn python(config: &Path, args: &[String]) -> Killer {
    let repo = std::env::var("RETICULUM_PY_REPO").expect("set RETICULUM_PY_REPO");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/rdo_python.py");
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into());
    let child = Command::new(python)
        .arg("-u")
        .arg(script)
        .arg(args[0].as_str())
        .arg(config)
        .args(&args[1..])
        .env("PYTHONPATH", repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn python");
    Killer(child)
}

fn write_config(dir: &Path, interface: &str) {
    std::fs::create_dir_all(dir).expect("config dir");
    let config = format!(
        "[reticulum]\n  enable_transport = False\n  share_instance = No\n\
         panic_on_interface_error = Yes\n[logging]\n  loglevel = 2\n[interfaces]\n{interface}"
    );
    std::fs::write(dir.join("config"), config).expect("write config");
}

fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve port");
    listener.local_addr().expect("addr").port()
}

fn lines(child: &mut Killer) -> Vec<Json> {
    let stdout = child.0.stdout.take().expect("stdout");
    BufReader::new(stdout)
        .lines()
        .map_while(|line| line.ok())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct EditorOnly(PublicIdentity);

impl Policy for EditorOnly {
    fn may_read(&self, who: &PublicIdentity) -> bool {
        *who == self.0
    }
    fn may_propose(&self, who: &PublicIdentity) -> bool {
        *who == self.0
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires local Python Reticulum checkout (RETICULUM_PY_REPO)"]
async fn rust_owner_python_client() {
    let fixture = fixture();
    let owner_identity =
        PrivateIdentity::from_private_key_bytes(&fixture.owner_private).expect("owner");
    let client = PrivateIdentity::new_from_rand(OsRng);
    let port = free_port();
    let server =
        Arc::new(Transport::new(TransportConfig::new("rdo-owner", &owner_identity, false)));
    let manager = server.iface_manager();
    manager
        .lock()
        .await
        .spawn(TcpServer::new(format!("127.0.0.1:{port}"), manager.clone()), TcpServer::spawn);
    let destination = register_object(&server, &owner_identity, &fixture.token).await;
    let policy = Arc::new(EditorOnly(public_identity(client.as_identity())));
    let service = Arc::new(OwnerService::new(server.clone(), policy, system_clock));
    let signer =
        rdo_core::SigningIdentity::from_private_bytes(&fixed(fixture.owner_private.clone()));
    service.add(Owner::new(signer, vec![fixture.genesis.clone()]).expect("owner"));
    let runner = service.clone();
    tokio::spawn(async move { runner.run().await });
    let token = fixture.token;
    tokio::spawn(async move {
        loop {
            server.send_announce(&destination, Some(&announce_app_data(&token))).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    let dir = tempfile::tempdir().expect("tempdir");
    write_config(
        dir.path(),
        &format!(
            "  [[Rust]]\n    type = TCPClientInterface\n    enabled = yes\n    \
             target_host = 127.0.0.1\n    target_port = {port}\n"
        ),
    );
    let body = cbor::encode(&Value::Map(fixture.successor.body.clone()));
    let mut child = python(
        dir.path(),
        &[
            "client".into(),
            hex_string(&client.to_private_key_bytes()),
            hex_string(&fixture.destination),
            hex_string(&owner(&fixtures()).public().0),
            hex_string(&body),
        ],
    );
    let steps = tokio::task::spawn_blocking(move || lines(&mut child)).await.expect("join");
    let step = |name: &str| {
        steps.iter().find(|s| s["step"] == name).unwrap_or_else(|| panic!("{name}: {steps:?}"))
    };
    assert_eq!(step("resolved")["bound"], true);
    assert_eq!(step("read")["status"], 0);
    assert_eq!(step("read")["verified"], true);
    assert_eq!(step("read")["head"], hex_string(&fixture.genesis.id));
    assert_eq!(step("propose")["status"], 0);
    assert_eq!(step("propose")["version"], hex_string(&fixture.successor.id));
    assert_eq!(step("retry")["same"], true);
    assert_eq!(step("stale")["status"], Status::Stale as u64);
    assert_eq!(service.owners().lock().expect("owners")[&fixture.destination].history().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires local Python Reticulum checkout (RETICULUM_PY_REPO)"]
async fn python_owner_rust_client() {
    let fixture = fixture();
    let editor = PrivateIdentity::new_from_rand(OsRng);
    let port = free_port();
    let dir = tempfile::tempdir().expect("tempdir");
    write_config(
        dir.path(),
        &format!(
            "  [[Rust]]\n    type = TCPServerInterface\n    enabled = yes\n    \
             listen_ip = 127.0.0.1\n    listen_port = {port}\n"
        ),
    );
    let _owner = python(
        dir.path(),
        &[
            "owner".into(),
            hex_string(&fixture.owner_private),
            hex_string(&fixture.token),
            hex_string(&fixture.genesis.bytes),
            hex_string(editor.as_identity().address_hash.as_slice()),
        ],
    );
    tokio::time::sleep(Duration::from_secs(2)).await;

    let client = Transport::new(TransportConfig::new("rdo-client", &editor, false));
    client
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{port}")), TcpClient::spawn);
    let (binding, desc) =
        resolve_object(&client, &fixture.destination, TIMEOUT).await.expect("resolve");
    assert_eq!(binding.owner, owner(&fixtures()).public());
    let link = open_object_link(&client, &editor, desc, TIMEOUT).await.expect("link");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let now = system_clock();
    let wanted = fixture.destination;
    let read = call(&client, &link, &Request::read([1; 16], wanted, now + 60), TIMEOUT)
        .await
        .expect("read");
    assert_eq!((read.status, read.version), (Status::Applied, Some(fixture.genesis.id)));
    let cached = Revision::verify(&read.value.expect("revision"), &binding.owner).expect("verify");
    let propose =
        Request::propose([2; 16], wanted, cached.id, fixture.successor.body.clone(), now + 60);
    let applied = call(&client, &link, &propose, TIMEOUT).await.expect("propose");
    assert_eq!(applied.status, Status::Applied);
    assert_eq!(applied.version, Some(fixture.successor.id), "Python signs identical bytes");
    assert_eq!(call(&client, &link, &propose, TIMEOUT).await.expect("retry"), applied);
    let stale = Request::propose([3; 16], wanted, cached.id, fixture.successor.body, now + 60);
    let stale = call(&client, &link, &stale, TIMEOUT).await.expect("stale");
    assert_eq!(stale.status, Status::Stale);
}
