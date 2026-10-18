use rand_core::OsRng;
use reticulum_daemon::receipt_bridge::{
    CustodyCommit, ReceiptBridge, ReceiptEvent, ReceiptPublisher,
};
use rns_rpc::broker::PropagationCustody;
use rns_transport::destination::link::{Link, LinkStatus};
use rns_transport::destination::DestinationName;
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::{IfaceRole, IfaceSource, InterfaceChannel, RxMessage};
use rns_transport::packet::{Packet, PacketContext};
use rns_transport::transport::{
    DeliveryReceipt, LinkPacketReceipt, ReceiptHandler, SendPacketOutcome, Transport,
    TransportConfig,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

struct ObservedBridge {
    bridge: ReceiptBridge,
    completed: tokio::sync::watch::Sender<bool>,
}
impl ReceiptHandler for ObservedBridge {
    fn on_receipt(&self, receipt: &DeliveryReceipt) {
        self.bridge.on_receipt(receipt);
        self.completed.send_replace(true);
    }
}

async fn forward(channel: &InterfaceChannel, packet: Packet) {
    channel
        .rx_channel
        .send(RxMessage { address: *channel.address(), packet, source: IfaceSource::None })
        .await
        .expect("native ingress");
}

async fn next(channel: &mut InterfaceChannel) -> Packet {
    tokio::time::timeout(Duration::from_secs(2), channel.tx_channel.recv())
        .await
        .expect("native packet timeout")
        .expect("native interface")
        .packet
}

async fn link_pair(
) -> (Transport, Transport, Arc<AsyncMutex<Link>>, InterfaceChannel, InterfaceChannel) {
    let local = PrivateIdentity::new_from_rand(OsRng);
    let sender = Transport::new(TransportConfig::new("owned-packet-sender", &local, true));
    let remote = PrivateIdentity::new_from_rand(OsRng);
    let receiver = Transport::new(TransportConfig::new("owned-packet-peer", &remote, true));
    let destination =
        receiver.add_destination(remote, DestinationName::new("lxmf", "delivery")).await;
    let destination = destination.lock().await.desc;
    let mut sending =
        sender.iface_manager().lock().await.new_channel_with_role(16, IfaceRole::Unicast);
    let mut receiving =
        receiver.iface_manager().lock().await.new_channel_with_role(16, IfaceRole::Unicast);
    let link = sender.link(destination).await;
    forward(&receiving, next(&mut sending).await).await;
    forward(&sending, next(&mut receiving).await).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while link.lock().await.status() != LinkStatus::Active {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native Link activation");
    let rtt = next(&mut sending).await;
    assert_eq!(rtt.context, PacketContext::LinkRTT);
    forward(&receiving, rtt).await;
    (sender, receiver, link, sending, receiving)
}

async fn send_owned(
    transport: &Transport,
    link: &Arc<AsyncMutex<Link>>,
    publisher: &ReceiptPublisher,
    event: ReceiptEvent,
    payload: &[u8],
) -> LinkPacketReceipt {
    let packet = link.lock().await.data_packet(payload).expect("Link data packet");
    let mut observed = None;
    let outcome = transport
        .send_link_packet_on_bound_iface_observed(link, packet, |receipt| {
            assert!(publisher
                .track_link_packet(event, receipt.clone())
                .expect("immutable correlation"));
            observed = Some(receipt);
            true
        })
        .await
        .expect("native send");
    assert_eq!(outcome, Some(SendPacketOutcome::SentDirect));
    observed.expect("native packet token")
}

#[tokio::test]
async fn packet_custody_preserves_earlier_direct_meaning_through_real_late_proof() {
    let (sender, _receiver, link, mut sending, mut receiving) = link_pair().await;
    let (tx, mut events) = mpsc::channel(4);
    let publisher = ReceiptPublisher::from(tx);
    publisher.reserve_completion("message", true).expect("paired completion capacity");
    let bridge = ReceiptBridge::new(Arc::new(Mutex::new(HashMap::new())), publisher.clone());
    sender.set_receipt_handler(Box::new(bridge.clone())).await;
    let mut direct = send_owned(
        &sender,
        &link,
        &publisher,
        ReceiptEvent::new("message", "delivered").with_method("direct"),
        b"Direct attempt bytes",
    )
    .await;
    bridge.on_receipt(&DeliveryReceipt::new(direct.packet_hash().to_bytes()));
    assert!(events.try_recv().is_err(), "unvalidated callback cannot claim owned delivery");
    assert_eq!(
        publisher
            .track_link_packet(
                ReceiptEvent::propagation_custody("message", PropagationCustody::Packet),
                direct.clone()
            )
            .expect_err("immutable method")
            .kind(),
        std::io::ErrorKind::AlreadyExists
    );
    forward(&receiving, next(&mut sending).await).await;
    let late_direct_proof = next(&mut receiving).await;
    let mut propagated = send_owned(
        &sender,
        &link,
        &publisher,
        ReceiptEvent::propagation_custody("message", PropagationCustody::Packet)
            .with_method("propagation"),
        b"Propagation attempt bytes",
    )
    .await;
    forward(&receiving, next(&mut sending).await).await;
    forward(&sending, next(&mut receiving).await).await;
    propagated.wait_proved().await.expect("native peer custody proof");
    let custody = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("custody publication")
        .expect("receipt queue");
    assert_eq!(custody.custody(), Some(PropagationCustody::Packet));
    assert_eq!(custody.status, PropagationCustody::Packet.status());
    assert_eq!(custody.packet_hash, Some(hex::encode(propagated.packet_hash().as_slice())));
    publisher.complete_custody("message", CustodyCommit::Stored);
    assert!(!publisher
        .publish_link_packet(propagated.packet_hash().to_bytes())
        .expect("custody retired"));
    sender.teardown_link(&link).await.expect("native teardown");
    next(&mut sending).await;
    forward(&sending, late_direct_proof).await;
    direct.wait_proved().await.expect("native late Direct proof after teardown");
    let delivered = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("delivery publication")
        .expect("receipt queue");
    assert_eq!(delivered.status, "delivered");
    assert_eq!(delivered.method.as_deref(), Some("direct"));
    assert!(delivered.custody().is_none());
    assert_eq!(delivered.packet_hash, Some(hex::encode(direct.packet_hash().as_slice())));
    publisher.complete_terminal("message");
    assert!(!publisher
        .publish_link_packet(direct.packet_hash().to_bytes())
        .expect("terminal retirement"));
}

#[tokio::test]
async fn native_packet_proof_survives_receipt_queue_backpressure_for_owner_retry() {
    let (sender, _receiver, link, mut sending, mut receiving) = link_pair().await;
    let (tx, mut events) = mpsc::channel(1);
    let publisher = ReceiptPublisher::from(tx);
    let (completed, mut completion) = tokio::sync::watch::channel(false);
    sender
        .set_receipt_handler(Box::new(ObservedBridge {
            bridge: ReceiptBridge::new(Arc::new(Mutex::new(HashMap::new())), publisher.clone()),
            completed,
        }))
        .await;
    publisher.try_send(ReceiptEvent::new("unrelated", "queued")).expect("fill queue");
    let mut receipt = send_owned(
        &sender,
        &link,
        &publisher,
        ReceiptEvent::new("message", "delivered"),
        b"bounded proof bytes",
    )
    .await;
    forward(&receiving, next(&mut sending).await).await;
    forward(&sending, next(&mut receiving).await).await;
    receipt.wait_proved().await.expect("native proof retained despite full queue");
    tokio::time::timeout(Duration::from_secs(2), completion.changed())
        .await
        .expect("receipt callback completed")
        .expect("callback observer");
    assert!(!publisher.terminal_pending("message").expect("publication state"));
    assert_eq!(events.recv().await.expect("queued event").message_id, "unrelated");
    assert!(publisher
        .publish_link_packet(receipt.packet_hash().to_bytes())
        .expect("retry publication only"));
    assert_eq!(events.recv().await.expect("retried receipt").status, "delivered");
    assert!(sending.tx_channel.try_recv().is_err(), "receipt retry did not retransmit");
    publisher.complete_terminal("message");
}
