//! RDO owner operations over real RNS: two transports on loopback TCP. The
//! client starts with only the object Destination.

mod common;

use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use rand_core::OsRng;
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

const TIMEOUT: Duration = Duration::from_secs(20);

struct EditorOnly(PublicIdentity, PublicIdentity);

impl Policy for EditorOnly {
    fn may_read(&self, who: &PublicIdentity) -> bool {
        *who == self.0 || *who == self.1
    }
    fn may_propose(&self, who: &PublicIdentity) -> bool {
        *who == self.0
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve port");
    listener.local_addr().expect("local addr").port()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn owner_operations_over_an_identified_link() {
    let fixtures = fixtures();
    let private = hex(&fixtures["fixture_identity_private_hex"]);
    let owner_identity = PrivateIdentity::from_private_key_bytes(&private).expect("owner key");
    let property = fixtures["bindings"]
        .as_array()
        .expect("bindings")
        .iter()
        .find(|binding| binding["label"] == "Property")
        .expect("Property binding")
        .clone();
    let token = fixed(hex(&property["token_hex"]));
    let wanted: [u8; 16] = fixed(hex(&property["destination_hex"]));
    let genesis_bytes = hex(&fixtures["revisions"]["property_genesis"]["cbor_hex"]);
    let genesis = Revision::verify(&genesis_bytes, &owner(&fixtures).public()).expect("genesis");

    let editor = PrivateIdentity::new_from_rand(OsRng);
    let reader = PrivateIdentity::new_from_rand(OsRng);
    let policy = Arc::new(EditorOnly(
        public_identity(editor.as_identity()),
        public_identity(reader.as_identity()),
    ));

    // Owner process.
    let port = free_port();
    let server =
        Arc::new(Transport::new(TransportConfig::new("rdo-owner", &owner_identity, false)));
    let manager = server.iface_manager();
    manager
        .lock()
        .await
        .spawn(TcpServer::new(format!("127.0.0.1:{port}"), manager.clone()), TcpServer::spawn);
    let destination = register_object(&server, &owner_identity, &token).await;
    let service = Arc::new(OwnerService::new(server.clone(), policy, system_clock));
    service.add(Owner::new(owner(&fixtures), vec![genesis.clone()]).expect("owner"));
    let runner = service.clone();
    tokio::spawn(async move { runner.run().await });
    let announcer = server.clone();
    tokio::spawn(async move {
        loop {
            announcer.send_announce(&destination, Some(&announce_app_data(&token))).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    // Client process: only the Destination is known.
    let client = Transport::new(TransportConfig::new("rdo-client", &editor, false));
    client
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{port}")), TcpClient::spawn);
    let (binding, desc) = resolve_object(&client, &wanted, TIMEOUT).await.expect("resolve");
    assert_eq!(binding.owner, owner(&fixtures).public());
    assert_eq!(binding.token, token);

    let now = system_clock();
    let link = open_object_link(&client, &editor, desc, TIMEOUT).await.expect("editor link");
    let read = call(&client, &link, &Request::read([1; 16], wanted, now + 60), TIMEOUT)
        .await
        .expect("read");
    assert_eq!(read.status, Status::Applied);
    assert_eq!(read.value.as_deref(), Some(genesis_bytes.as_slice()));

    let successor = fixtures["revisions"]["property_successor"].clone();
    let successor = Revision::verify(&hex(&successor["cbor_hex"]), &binding.owner).expect("rev");
    let propose = Request::propose([2; 16], wanted, genesis.id, successor.body.clone(), now + 60);
    let applied = call(&client, &link, &propose, TIMEOUT).await.expect("propose");
    assert_eq!(applied.status, Status::Applied);
    assert_eq!(applied.version, Some(successor.id), "deterministic owner signature");
    let retry = call(&client, &link, &propose, TIMEOUT).await.expect("retry");
    assert_eq!(retry, applied, "identical retry returns the retained result");
    let stale = Request::propose([3; 16], wanted, genesis.id, successor.body.clone(), now + 60);
    let stale = call(&client, &link, &stale, TIMEOUT).await.expect("stale");
    assert_eq!((stale.status, stale.version), (Status::Stale, Some(successor.id)));

    // A reader, a separate client, may read but not mutate.
    let reader_client = Transport::new(TransportConfig::new("rdo-reader", &reader, false));
    reader_client
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{port}")), TcpClient::spawn);
    let (_, desc) = resolve_object(&reader_client, &wanted, TIMEOUT).await.expect("resolve");
    let client = reader_client;
    let reader_link = open_object_link(&client, &reader, desc, TIMEOUT).await.expect("reader");
    let foreign = Request::propose([4; 16], wanted, successor.id, genesis.body.clone(), now + 60);
    let denied = call(&client, &reader_link, &foreign, TIMEOUT).await.expect("foreign");
    assert_eq!((denied.status, denied.version, denied.value), (Status::NotAuthorized, None, None));
    let replica = call(&client, &reader_link, &Request::read([5; 16], wanted, now + 60), TIMEOUT)
        .await
        .expect("reader read");
    let replica_bytes = replica.value.expect("signed revision");
    assert_eq!(service.owners().lock().expect("owners")[&wanted].history().len(), 2);

    // Offline replica: the owner's signature alone validates the cached copy.
    drop(service);
    let cached = Revision::verify(&replica_bytes, &binding.owner).expect("offline read");
    assert_eq!(cached.id, successor.id);
}
