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
    let outbox = LocalPropagation::open(config, path, StoreLimits::default()).unwrap();
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
    wire.sign(&outbox.owner).unwrap();
    let message = PreparedMessage::new(&wire, &outbox.owner, Some(&receiver)).unwrap();
    (outbox, PreparedOutgoing { wire, remote, message })
}

#[tokio::test]
async fn no_interfaces_commits_original_only_after_direct_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (outbox, prepared) = fixture(&dir.path().join("outbox.db"));
    let id = prepared.message_id().unwrap();
    let request = SendRequest::new(
        "unused",
        prepared.destination(),
        serde_json::json!({"content_base64":"aGk="}),
    );
    assert!(outbox.send_direct(&prepared, &request, Duration::from_millis(5)).await.is_err());
    assert!(outbox.status(10).unwrap().is_empty());
    let status = outbox.deposit_own(prepared, MessageKind::Chat, 10, None).unwrap();
    assert_eq!(status.message_id, id);
    assert_eq!(status.state, lxmf_propagation::State::StoredLocally);
    assert!(outbox
        .recover_direct(&status.transient_id, 11, 41, |_| false)
        .await
        .unwrap()
        .is_none());
    assert!(outbox.delivered(&status.transient_id, &id, false, 12).unwrap());
    assert_eq!(outbox.status(12).unwrap()[0].encrypted_bytes, 0);
}

#[tokio::test]
async fn cancelled_recovery_releases_the_shared_transfer_owner() {
    let dir = tempfile::tempdir().unwrap();
    let (outbox, prepared) = fixture(&dir.path().join("outbox.db"));
    let status = outbox.deposit_own(prepared, MessageKind::Chat, 10, None).unwrap();
    let worker = outbox.clone();
    let id = status.transient_id.clone();
    let task = tokio::spawn(async move { worker.recover_direct(&id, 11, 41, |_| true).await });
    tokio::task::yield_now().await;
    task.abort();
    let _ = task.await;
    let recovery = outbox.lock().unwrap().recover(&status.transient_id, 42, 72, |_| true).unwrap();
    assert!(recovery.is_some());
}

#[tokio::test]
async fn unresolved_identity_is_typed_and_never_admitted() {
    let dir = tempfile::tempdir().unwrap();
    let (outbox, _) = fixture(&dir.path().join("outbox.db"));
    let request =
        SendRequest::new("unused", "11".repeat(16), serde_json::json!({"content_base64":"aGk="}));
    let error = outbox.prepare(&request).await.err().unwrap();
    assert_eq!(error.cause_code.as_deref(), Some("UnresolvedRecipient"));
    assert!(outbox.status(10).unwrap().is_empty());
}
