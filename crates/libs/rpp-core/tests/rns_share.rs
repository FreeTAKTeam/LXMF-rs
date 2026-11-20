//! REM/RCH sharing over real RNS: publish, Offer to one peer, verified fetch;
//! a peer that was not offered the Package is refused.

use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use rand_core::OsRng;
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::transport::{Transport, TransportConfig};
use rpp_core::control::OfferDecision;
use rpp_core::rns::system_clock;
use rpp_core::rns_share::ShareNode;
use rpp_core::RppError;

const TIMEOUT: Duration = Duration::from_secs(20);

fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port");
    listener.local_addr().expect("addr").port()
}

async fn node(
    name: &str,
    port: u16,
    server: bool,
) -> (
    Arc<ShareNode>,
    PrivateIdentity,
    tokio::sync::mpsc::UnboundedReceiver<rpp_core::rns_share::ReceivedOffer>,
) {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Arc::new(Transport::new(TransportConfig::new(name, &identity, server)));
    let manager = transport.iface_manager();
    if server {
        manager
            .lock()
            .await
            .spawn(TcpServer::new(format!("127.0.0.1:{port}"), manager.clone()), TcpServer::spawn);
    } else {
        manager.lock().await.spawn(TcpClient::new(format!("127.0.0.1:{port}")), TcpClient::spawn);
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let node = ShareNode::start(transport, identity.clone(), system_clock, move |offer| {
        let _ = tx.send(offer);
    })
    .await;
    (node, identity, rx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offered_peer_fetches_a_verified_file_and_others_are_refused() {
    let port = free_port();
    let (sender, _, _) = node("share-a", port, true).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (receiver, receiver_identity, mut offers) = node("share-b", port, false).await;
    let (outsider, _, _) = node("share-c", port, false).await;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let dir = tempfile::tempdir().expect("dir");
    let source = dir.path().join("plan.pdf");
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&source, &data).expect("source");
    let shared =
        sender.publish_file(&source, "plan.pdf", Some("application/pdf")).await.expect("publish");

    let inbox = ShareNode::inbox_of(*receiver_identity.as_identity());
    let decision = sender.offer(&shared, receiver.me(), inbox, TIMEOUT).await.expect("offer");
    assert_eq!(decision, OfferDecision::Defer, "the operator decides later");
    let offer =
        tokio::time::timeout(TIMEOUT, offers.recv()).await.expect("offer event").expect("offer");
    assert_eq!(offer.sender, sender.me());

    let store = tempfile::tempdir().expect("store");
    let fetched =
        receiver.fetch(offer.package_destination, store.path(), TIMEOUT).await.expect("fetch");
    assert_eq!(fetched.name, "plan.pdf");
    assert_eq!(fetched.media_type.as_deref(), Some("application/pdf"));
    assert_eq!(std::fs::read(&fetched.path).expect("file"), data);

    let other = tempfile::tempdir().expect("store");
    let refused = outsider.fetch(offer.package_destination, other.path(), TIMEOUT).await;
    assert!(matches!(refused, Err(RppError::NotAuthorized)), "{refused:?}");
}
