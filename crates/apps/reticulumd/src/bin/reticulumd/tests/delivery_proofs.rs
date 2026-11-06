use crate::bootstrap::PropagationControlContext;
use crate::inbound_worker::spawn_inbound_worker;
use rand_core::OsRng;
use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
use rns_rpc::{MessagesStore, RpcDaemon, RpcRequest};
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::hash::AddressHash;
use rns_transport::identity::PrivateIdentity;
use rns_transport::identity_bridge::to_core_private_identity;
use rns_transport::iface::{IfaceSource, InterfaceChannel, RxMessage};
use rns_transport::packet::{Packet, PacketDataBuffer, PacketType};
use rns_transport::ratchets::encrypt_for_public_key;
use rns_transport::transport::{Transport, TransportConfig};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::timeout;

struct Peer {
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    recipient: PrivateIdentity,
    destination: AddressHash,
    iface: InterfaceChannel,
}

impl Peer {
    async fn start(daemon: RpcDaemon, recipient: PrivateIdentity) -> Self {
        Self::start_with_policy(daemon, recipient, true).await
    }

    async fn start_with_policy(
        daemon: RpcDaemon,
        recipient: PrivateIdentity,
        deferred: bool,
    ) -> Self {
        let transport =
            Arc::new(Transport::new(TransportConfig::new("proof-hub", &recipient, true)));
        let destination = transport
            .add_destination_with_deferred_proofs(
                recipient.clone(),
                DestinationName::new("lxmf", "delivery"),
            )
            .await;
        destination.lock().await.set_deferred_proofs(deferred);
        let address = destination.lock().await.desc.address_hash;
        let daemon = Arc::new(daemon);
        let iface = transport.iface_manager().lock().await.new_channel(32);
        spawn_inbound_worker(
            daemon.clone(),
            transport.clone(),
            PropagationControlContext {
                enabled: false,
                local_identity_hash: [0; 16],
                propagation_destination_hash_hex: None,
                control_destination_hash_hex: None,
                delivery_destination: Some(destination),
                allowed_control_identities: Vec::new(),
                validated_peer_links: Arc::new(Mutex::new(HashSet::new())),
                identified_peer_links: Arc::new(Mutex::new(HashMap::new())),
            },
            None,
        );
        Self { daemon, transport, recipient, destination: address, iface }
    }

    fn wire(&self, source: &PrivateIdentity, content: &str) -> Vec<u8> {
        let source_destination =
            SingleInputDestination::new(source.clone(), DestinationName::new("lxmf", "delivery"));
        build_wire_message_with_options(
            source_destination.desc.address_hash.as_slice().try_into().expect("source"),
            self.destination.as_slice().try_into().expect("destination"),
            "receipt test",
            content,
            None,
            &to_core_private_identity(source),
            None,
            None,
            None,
        )
        .expect("signed LXMF")
    }

    fn packet(&self, payload: &[u8]) -> Packet {
        let identity = self.recipient.as_identity();
        let ciphertext = encrypt_for_public_key(
            &identity.public_key,
            identity.address_hash.as_slice(),
            payload,
            OsRng,
        )
        .expect("encrypt");
        Packet {
            destination: self.destination,
            data: PacketDataBuffer::new_from_slice(&ciphertext),
            ..Packet::default()
        }
    }

    async fn inject(&self, packet: &Packet) {
        self.iface
            .rx_channel
            .send(RxMessage {
                address: self.iface.address,
                packet: packet.clone(),
                source: IfaceSource::None,
            })
            .await
            .expect("wire ingress");
    }

    async fn proof(&mut self, original: &Packet) {
        let proof = timeout(Duration::from_secs(3), async {
            loop {
                let message = self.iface.tx_channel.recv().await.expect("wire output");
                if message.packet.header.packet_type == PacketType::Proof {
                    break message.packet;
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            let events: Vec<_> = std::iter::from_fn(|| self.daemon.take_event())
                .map(|event| (event.event_type, event.payload))
                .collect();
            panic!(
                "accepted delivery proof timed out; stored={:?}; events={events:?}",
                self.messages()
            );
        });
        let hash = original.hash();
        assert_eq!(proof.destination, AddressHash::new_from_hash(&hash));
        assert_eq!(&proof.data.as_slice()[..32], hash.as_slice());
        let signature =
            ed25519_dalek::Signature::from_slice(&proof.data.as_slice()[32..]).expect("signature");
        self.recipient
            .as_identity()
            .verify(hash.as_slice(), &signature)
            .expect("sender validates receiver proof");
    }

    async fn wait_drop(&self, reason: &str) {
        timeout(Duration::from_secs(3), async {
            loop {
                while let Some(event) = self.daemon.take_event() {
                    if event.event_type == "inbound_dropped" && event.payload["reason"] == reason {
                        return;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("production rejection event");
    }

    async fn no_proof(&mut self) {
        assert!(
            timeout(Duration::from_millis(150), async {
                loop {
                    if self
                        .iface
                        .tx_channel
                        .recv()
                        .await
                        .expect("wire output")
                        .packet
                        .header
                        .packet_type
                        == PacketType::Proof
                    {
                        break;
                    }
                }
            })
            .await
            .is_err(),
            "unadmitted data must not be proved"
        );
    }

    fn messages(&self) -> Vec<Value> {
        self.daemon
            .handle_rpc(RpcRequest { id: 1, method: "list_messages".into(), params: None })
            .expect("list")
            .result
            .expect("result")["messages"]
            .as_array()
            .expect("messages")
            .clone()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn deferred_delivery_proofs_wait_for_commit_and_recover_duplicate_reconnect_restart() {
    let dir = tempfile::tempdir().expect("temporary store");
    let path = dir.path().join("messages.db");
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let store = MessagesStore::open(&path).expect("store");
    store.enable_durable_broker(32 * 1024 * 1024).expect("reported RCH broker profile");
    let daemon = RpcDaemon::with_store(store, "hub".into());
    let mut peer = Peer::start(daemon, identity.clone()).await;
    let wire = peer.wire(&PrivateIdentity::new_from_rand(OsRng), "durable packet");
    let packet = peer.packet(&wire[16..]);
    let blocker = rusqlite::Connection::open(&path).expect("external writer");
    blocker.execute_batch("BEGIN IMMEDIATE").expect("block commit");
    peer.inject(&packet).await;
    // A broker transaction reads before upgrading to a writer. With an external
    // writer it rejects that upgrade rather than waiting with an old snapshot.
    peer.wait_drop("inbound_persistence_failed").await;
    peer.no_proof().await;
    assert!(peer.messages().is_empty());
    assert_eq!(broker_inbound_count(&blocker), 0);
    blocker.execute_batch("COMMIT").expect("release commit");
    peer.inject(&packet).await;
    peer.proof(&packet).await;
    assert_eq!(peer.messages().len(), 1);
    assert_eq!(broker_inbound_count(&blocker), 1);
    while peer.daemon.take_event().is_some() {}

    // Exact ciphertext retry crosses the production membership and queue filters.
    peer.inject(&packet).await;
    peer.proof(&packet).await;
    assert_eq!(peer.messages().len(), 1);
    assert_eq!(
        broker_inbound_count(&blocker),
        1,
        "retry must not append another durable inbound event"
    );
    let events: Vec<_> = std::iter::from_fn(|| peer.daemon.take_event()).collect();
    assert!(!events.iter().any(|event| event.event_type == "inbound"));
    assert!(events.iter().any(|event| event.payload["reason"] == "duplicate"));

    // A reconnect must return the proof on the new ingress interface.
    peer.transport.stop_interface(peer.iface.address).await;
    peer.iface = peer.transport.iface_manager().lock().await.new_channel(32);
    peer.inject(&packet).await;
    peer.proof(&packet).await;
    peer.transport.stop_interface(peer.iface.address).await;
    drop(peer);
    let reopened = MessagesStore::open(&path).expect("reopen");
    assert!(reopened.durable_broker_enabled().expect("persisted broker profile"));
    let daemon = RpcDaemon::with_store(reopened, "hub".into());
    let mut restarted = Peer::start(daemon, identity).await;
    restarted.inject(&packet).await;
    restarted.proof(&packet).await;
    assert_eq!(restarted.messages().len(), 1);
    assert_eq!(broker_inbound_count(&blocker), 1, "reopen must retain one durable delivery event");
    restarted.transport.stop_interface(restarted.iface.address).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn deferred_delivery_proofs_reject_malformed_policy_and_persistence_failures() {
    let dir = tempfile::tempdir().expect("temporary store");
    let path = dir.path().join("messages.db");
    let daemon = RpcDaemon::with_store(MessagesStore::open(&path).expect("store"), "hub".into());
    let mut peer = Peer::start(daemon, PrivateIdentity::new_from_rand(OsRng)).await;
    let malformed = peer.packet(b"malformed LXMF");
    peer.inject(&malformed).await;
    peer.wait_drop("decode_failed").await;
    peer.no_proof().await;
    assert!(peer.messages().is_empty());

    let source = PrivateIdentity::new_from_rand(OsRng);
    let wire = peer.wire(&source, "blocked packet");
    let packet = peer.packet(&wire[16..]);
    let source_hash = hex::encode(
        SingleInputDestination::new(source, DestinationName::new("lxmf", "delivery"))
            .desc
            .address_hash
            .as_slice(),
    );
    peer.daemon
        .handle_rpc(RpcRequest {
            id: 2,
            method: "set_delivery_policy".into(),
            params: Some(json!({"ignored_destinations": [source_hash]})),
        })
        .expect("delivery policy");
    peer.inject(&packet).await;
    peer.wait_drop("delivery_policy_rejected").await;
    peer.no_proof().await;
    assert!(peer.messages().is_empty());
    peer.daemon
        .handle_rpc(RpcRequest {
            id: 3,
            method: "set_delivery_policy".into(),
            params: Some(json!({"ignored_destinations": []})),
        })
        .expect("clear policy");

    peer.daemon
        .handle_rpc(RpcRequest {
            id: 4,
            method: "stamp_policy_set".into(),
            params: Some(json!({"target_cost": 4, "flexibility": 0})),
        })
        .expect("require stamp");
    peer.inject(&packet).await;
    peer.wait_drop("stamp_policy_rejected").await;
    peer.no_proof().await;
    assert!(peer.messages().is_empty());
    peer.daemon
        .handle_rpc(RpcRequest {
            id: 5,
            method: "stamp_policy_set".into(),
            params: Some(json!({"target_cost": 0, "flexibility": 0})),
        })
        .expect("clear stamp requirement");

    let connection = rusqlite::Connection::open(&path).expect("trigger connection");
    connection.execute_batch("CREATE TRIGGER reject_test_intake BEFORE INSERT ON messages BEGIN SELECT RAISE(FAIL, 'test intake rejection'); END").expect("reject storage");
    peer.inject(&packet).await;
    peer.wait_drop("inbound_persistence_failed").await;
    peer.no_proof().await;
    assert!(peer.messages().is_empty());
    connection.execute_batch("DROP TRIGGER reject_test_intake").expect("restore storage");
    peer.inject(&packet).await;
    peer.proof(&packet).await;
    assert_eq!(peer.messages().len(), 1);
    peer.transport.stop_interface(peer.iface.address).await;
}

#[tokio::test]
async fn deferred_delivery_proofs_preserve_signature_metadata_admission() {
    let mut peer =
        Peer::start(RpcDaemon::test_instance(), PrivateIdentity::new_from_rand(OsRng)).await;
    let source = PrivateIdentity::new_from_rand(OsRng);
    let mut source_destination =
        SingleInputDestination::new(source.clone(), DestinationName::new("lxmf", "delivery"));
    let wire = peer.wire(&source, "unknown source identity");
    let packet = peer.packet(&wire[16..]);
    peer.inject(&packet).await;
    peer.proof(&packet).await;
    assert_eq!(
        peer.messages()[0]["fields"]["_lxmf"]["signature_status"],
        "source_identity_unknown"
    );

    let announce = source_destination.announce(OsRng, None).expect("source announce");
    peer.inject(&announce).await;
    timeout(Duration::from_secs(3), async {
        while peer
            .transport
            .destination_identity(&source_destination.desc.address_hash)
            .await
            .is_none()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("learn source");
    for (content, invalid, expected) in
        [("valid", false, "verified"), ("invalid", true, "signature_invalid")]
    {
        let mut wire = peer.wire(&source, content);
        if invalid {
            wire[32] ^= 1;
        }
        let packet = peer.packet(&wire[16..]);
        peer.inject(&packet).await;
        peer.proof(&packet).await;
        let messages = peer.messages();
        let stored =
            messages.iter().find(|row| row["content"] == content).expect("accepted message");
        assert_eq!(stored["fields"]["_lxmf"]["signature_status"], expected);
    }
    peer.transport.stop_interface(peer.iface.address).await;
}

#[tokio::test]
async fn legacy_none_delivery_policy_stores_without_proving_negative_control() {
    let mut peer = Peer::start_with_policy(
        RpcDaemon::test_instance(),
        PrivateIdentity::new_from_rand(OsRng),
        false,
    )
    .await;
    let wire = peer.wire(&PrivateIdentity::new_from_rand(OsRng), "old None proof policy");
    let packet = peer.packet(&wire[16..]);
    peer.inject(&packet).await;
    timeout(Duration::from_secs(3), async {
        while peer.messages().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("old policy accepts and stores");
    peer.no_proof().await;
    assert_eq!(peer.messages().len(), 1, "stored is not transport proof");
    peer.transport.stop_interface(peer.iface.address).await;
}

fn broker_inbound_count(connection: &rusqlite::Connection) -> i64 {
    connection
        .query_row("SELECT COUNT(*) FROM broker_events WHERE event_type='inbound'", [], |row| {
            row.get(0)
        })
        .expect("durable inbound event count")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn deferred_delivery_proofs_wait_while_nonbroker_storage_commit_is_blocked() {
    let dir = tempfile::tempdir().expect("temporary store");
    let path = dir.path().join("messages.db");
    let daemon = RpcDaemon::with_store(MessagesStore::open(&path).expect("store"), "hub".into());
    let mut peer = Peer::start(daemon, PrivateIdentity::new_from_rand(OsRng)).await;
    let wire = peer.wire(&PrivateIdentity::new_from_rand(OsRng), "wait for durable commit");
    let packet = peer.packet(&wire[16..]);
    let blocker = rusqlite::Connection::open(&path).expect("external writer");
    blocker.execute_batch("BEGIN IMMEDIATE").expect("block commit");
    peer.inject(&packet).await;
    peer.no_proof().await;
    assert!(peer.messages().is_empty());
    blocker.execute_batch("COMMIT").expect("release commit");
    peer.proof(&packet).await;
    assert_eq!(peer.messages().len(), 1);
    peer.transport.stop_interface(peer.iface.address).await;
}
