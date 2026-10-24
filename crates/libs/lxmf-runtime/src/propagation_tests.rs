use super::*;
use lxmf_core::message::Payload;
use rand_core::OsRng;
use rns_transport::{
    destination::{DestinationName, SingleOutputDestination},
    identity::PrivateIdentity,
    transport::{Transport, TransportConfig},
};
use tokio::runtime::Handle;

fn fixture(path: &Path) -> (LocalPropagation, PreparedOutgoing) {
    let owner = PrivateIdentity::new_from_rand(OsRng);
    let recipient = PrivateIdentity::new_from_rand(OsRng);
    let transport =
        Arc::new(Transport::new(TransportConfig::new("embedded-propagation-test", &owner, true)));
    let source = SingleOutputDestination::new(
        *owner.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )
    .desc
    .address_hash;
    let mut config =
        InProcessBackendConfig::new("test", Handle::current(), transport, owner.clone(), source);
    config.link_connect_timeout = Duration::from_millis(30);
    let outbox = LocalPropagation::open(config, path, StoreLimits::default())
        .expect("valid test fixture or operation");
    let remote = SingleOutputDestination::new(
        *recipient.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )
    .desc;
    let receiver = lxmf_core::identity::Identity::new_from_slices(
        remote.identity.public_key_bytes(),
        remote.identity.verifying_key_bytes(),
    );
    let mut wire = WireMessage::new(
        lxmf_propagation::prepared::delivery_address(&receiver),
        lxmf_propagation::prepared::delivery_address(outbox.owner.as_identity()),
        Payload::new(1.0, Some(b"offline original".to_vec()), None, None, None),
    );
    wire.sign(&outbox.owner).expect("valid test fixture or operation");
    let message = PreparedMessage::new(&wire, &outbox.owner, Some(&receiver))
        .expect("valid test fixture or operation");
    (outbox, PreparedOutgoing { wire, remote, message })
}

#[tokio::test]
async fn no_interfaces_commits_original_only_after_direct_failure() {
    let dir = tempfile::tempdir().expect("valid test fixture or operation");
    let (outbox, prepared) = fixture(&dir.path().join("outbox.db"));
    let id = prepared.message_id().expect("valid test fixture or operation");
    let request = SendRequest::new(
        "unused",
        prepared.destination(),
        serde_json::json!({"content_base64":"aGk="}),
    );
    assert!(outbox
        .send_direct(&prepared, &request, Duration::from_millis(5), || true)
        .await
        .is_err());
    assert!(outbox.status(10).expect("valid test fixture or operation").is_empty());
    let status = outbox
        .deposit_own(prepared, MessageKind::Chat, 10, None)
        .expect("valid test fixture or operation");
    assert_eq!(status.message_id, id);
    assert_eq!(status.state, lxmf_propagation::State::StoredLocally);
    assert!(outbox
        .recover_direct(&status.transient_id, 11, 41, PropagationRouteLimits::default(), |_, _| {
            false
        })
        .await
        .expect("valid test fixture or operation")
        .is_none());
    assert!(outbox
        .delivered(&status.transient_id, &id, false, 12)
        .expect("valid test fixture or operation"));
    assert_eq!(outbox.status(12).expect("valid test fixture or operation")[0].encrypted_bytes, 0);
}

#[tokio::test]
async fn cancelled_recovery_releases_the_shared_transfer_owner() {
    let dir = tempfile::tempdir().expect("valid test fixture or operation");
    let (outbox, prepared) = fixture(&dir.path().join("outbox.db"));
    let status = outbox
        .deposit_own(prepared, MessageKind::Chat, 10, None)
        .expect("valid test fixture or operation");
    let worker = outbox.clone();
    let id = status.transient_id.clone();
    let task = tokio::spawn(async move {
        worker.recover_direct(&id, 11, 41, PropagationRouteLimits::default(), |_, _| true).await
    });
    tokio::task::yield_now().await;
    task.abort();
    let _ = task.await;
    let recovery = outbox
        .lock()
        .expect("valid test fixture or operation")
        .recover(&status.transient_id, 42, 72, |_| true)
        .expect("valid test fixture or operation");
    assert!(recovery.is_some());
}

#[tokio::test]
async fn unresolved_identity_is_typed_and_never_admitted() {
    let dir = tempfile::tempdir().expect("valid test fixture or operation");
    let (outbox, _) = fixture(&dir.path().join("outbox.db"));
    let request =
        SendRequest::new("unused", "11".repeat(16), serde_json::json!({"content_base64":"aGk="}));
    let error = outbox.prepare(&request).await.err().expect("valid test fixture or operation");
    assert_eq!(error.cause_code.as_deref(), Some("UnresolvedRecipient"));
    assert!(outbox.status(10).expect("valid test fixture or operation").is_empty());
}

#[test]
fn recovery_respects_the_bearer_packet_limit() {
    let wire = vec![0; 300];
    assert_eq!(representation(&wire, None), MessageMethod::Packet);
    assert_eq!(representation(&wire, Some(212)), MessageMethod::Resource);
}
