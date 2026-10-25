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
    let captured =
        crate::CapturedOutgoing { wire: prepared.wire.pack().unwrap(), remote: prepared.remote };
    let prepared = outbox.prepare_captured(captured, None).expect("same failed wire");
    assert_eq!(prepared.message_id().unwrap(), id);
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
    let (mut outbox, _) = fixture(&dir.path().join("outbox.db"));
    Arc::make_mut(&mut outbox.config).link_connect_timeout = Duration::from_secs(75);
    let request =
        SendRequest::new("unused", "11".repeat(16), serde_json::json!({"content_base64":"aGk="}));
    let error = tokio::time::timeout(Duration::from_millis(250), outbox.prepare(&request))
        .await
        .expect("no discovery deadline")
        .err()
        .expect("typed error");
    assert_eq!(error.cause_code.as_deref(), Some("UnresolvedRecipient"));
    assert!(outbox.status(10).expect("valid test fixture or operation").is_empty());
}

#[test]
fn recovery_respects_the_bearer_packet_limit() {
    let wire = vec![0; 300];
    assert_eq!(representation(&wire, None), MessageMethod::Packet);
    assert_eq!(representation(&wire, Some(212)), MessageMethod::Resource);
}

#[tokio::test]
async fn library_retrieval_holds_shared_admission_without_starting_an_endpoint() {
    let dir = tempfile::tempdir().expect("fixture");
    let (outbox, prepared) = fixture(&dir.path().join("outbox.db"));
    let recipient = prepared.remote.identity;
    let status = outbox.deposit_own(prepared, MessageKind::Chat, 10, None).expect("deposit");
    assert!(outbox.retrieve_for(&recipient, 11, 2048, |_, _| false).expect("denied").is_none());
    assert!(outbox.try_transfer().is_ok());
    let response =
        outbox.retrieve_for(&recipient, 11, 2048, |_, _| true).expect("retrieve").expect("entry");
    assert_eq!(response.message().status, status);
    assert!(outbox.try_transfer().is_err());
    assert_eq!(outbox.message(&status.message_id).expect("unchanged custody"), Some(status));
    drop(response);
    assert!(outbox.try_transfer().is_ok());
}

#[tokio::test]
async fn cached_recipient_prepares_after_restart_without_learning_a_route() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("outbox.db");
    let (outbox, prepared) = fixture(&path);
    let destination = prepared.remote.address_hash;
    let recipient = lxmf_core::identity::Identity::try_new_from_slices(
        prepared.remote.identity.public_key_bytes(),
        prepared.remote.identity.verifying_key_bytes(),
    )
    .unwrap();
    outbox
        .lock()
        .unwrap()
        .remember_recipient(destination.as_slice().try_into().unwrap(), &recipient, 1)
        .unwrap();
    let config = (*outbox.config).clone();
    drop(outbox);
    let outbox = LocalPropagation::open(config, &path, StoreLimits::default()).unwrap();
    assert!(outbox.config.transport.destination_identity(&destination).await.is_none());
    let request = SendRequest::new(
        "test",
        destination.to_hex_string(),
        serde_json::json!({"content_base64":"aGVsbG8gd29ybGQ="}),
    );
    let prepared = tokio::time::timeout(Duration::from_millis(250), outbox.prepare(&request))
        .await
        .unwrap()
        .unwrap();
    let id = prepared.message_id().unwrap();
    let status = outbox.deposit_own(prepared, MessageKind::Chat, 2, None).unwrap();
    assert_eq!(status.message_id, id);
    assert!(status.encrypted_bytes > 0);
    assert!(outbox.config.transport.destination_identity(&destination).await.is_none());
    assert!(!outbox.config.transport.has_path(&destination).await);
    assert!(matches!(
        outbox.remember_recipient(destination, 3).await,
        Err(Error::UnresolvedRecipient)
    ));
}
