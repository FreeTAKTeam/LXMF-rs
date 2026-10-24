use super::*;
use lxmf_core::message::Payload;
use rand_core::OsRng;
use rns_transport::{
    destination::{link::LinkStatus, DestinationName, SingleOutputDestination},
    identity::PrivateIdentity,
    iface::{IfaceRole, IfaceSource, InterfaceChannel, RxMessage},
    packet::{Packet, PacketContext},
    transport::{ReceivedData, TransportConfig},
};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::{
    runtime::Handle,
    sync::{broadcast, Semaphore},
    task::JoinHandle,
};

struct Pair {
    config: Arc<InProcessBackendConfig>,
    peer: Transport,
    relay: AddressHash,
    link: Arc<Mutex<Link>>,
    outgoing: InterfaceChannel,
    incoming: InterfaceChannel,
    requests: broadcast::Receiver<ReceivedData>,
    transient: Vec<u8>,
    wire: Vec<u8>,
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
        .expect("packet timeout")
        .expect("interface")
        .packet
}
impl Pair {
    async fn new() -> Self {
        let recipient = PrivateIdentity::new_from_rand(OsRng);
        let sender = PrivateIdentity::new_from_rand(OsRng);
        let local =
            Arc::new(Transport::new(TransportConfig::new("fetch-client", &recipient, true)));
        let peer = Transport::new(TransportConfig::new("fetch-relay", &sender, true));
        let destination =
            peer.add_destination(sender.clone(), DestinationName::new("lxmf", "propagation")).await;
        let delivery =
            peer.add_destination(sender.clone(), DestinationName::new("lxmf", "delivery")).await;
        let desc = destination.lock().await.desc;
        let mut outgoing =
            local.iface_manager().lock().await.new_channel_with_role(32, IfaceRole::Unicast);
        let mut incoming =
            peer.iface_manager().lock().await.new_channel_with_role(32, IfaceRole::Unicast);
        local.iface_manager().lock().await.set_shared_config(
            *outgoing.address(),
            rns_transport::iface::InterfaceSharedConfig {
                ingress_control: Some(false),
                ..Default::default()
            },
        );
        let mut announces = local.recv_announces().await;
        for destination in [destination, delivery] {
            forward(&outgoing, destination.lock().await.announce(OsRng, None).expect("announce"))
                .await;
            tokio::time::timeout(Duration::from_secs(2), announces.recv())
                .await
                .expect("announce timeout")
                .expect("learn identity");
        }
        while outgoing.tx_channel.try_recv().is_ok() {}
        let link = local.link(desc).await;
        forward(&incoming, next(&mut outgoing).await).await;
        forward(&outgoing, next(&mut incoming).await).await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while link.lock().await.status() != LinkStatus::Active {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("active link");
        forward(&incoming, next(&mut outgoing).await).await;
        let source = SingleOutputDestination::new(
            *recipient.as_identity(),
            DestinationName::new("lxmf", "delivery"),
        )
        .desc
        .address_hash;
        let owner = lxmf_core::identity::PrivateIdentity::from_private_key_bytes(
            &recipient.to_private_key_bytes(),
        )
        .expect("recipient");
        let signer = lxmf_core::identity::PrivateIdentity::from_private_key_bytes(
            &sender.to_private_key_bytes(),
        )
        .expect("sender");
        let mut wire = WireMessage::new(
            source.as_slice().try_into().expect("destination hash"),
            lxmf_propagation::prepared::delivery_address(signer.as_identity()),
            Payload::new(1.0, Some(vec![0x52; 300]), None, None, None),
        );
        wire.sign(&signer).expect("signature");
        let (transient, _) = wire
            .pack_propagation_transient_with_rng(owner.as_identity(), OsRng)
            .expect("transient");
        let config = Arc::new(InProcessBackendConfig::new(
            "fetch",
            Handle::current(),
            local,
            recipient,
            source,
        ));
        let requests = peer.received_data_events();
        Self {
            config,
            peer,
            relay: desc.address_hash,
            link,
            outgoing,
            incoming,
            requests,
            transient,
            wire: wire.pack().expect("wire"),
        }
    }
    async fn respond(&self, event: &ReceivedData, response: rmpv::Value) {
        let id = event.request_id.expect("request id");
        let data = rmp_serde::to_vec(&rmpv::Value::Array(vec![
            rmpv::Value::Binary(id.to_vec()),
            response,
        ]))
        .expect("response");
        let link = self.peer.find_in_link(&event.destination).await.expect("server link");
        let packet = link.lock().await.response_packet(&data);
        match packet {
            Ok(packet) => {
                assert!(rns_transport::delivery::send_outcome_is_sent(
                    self.peer.send_link_packet_on_bound_iface(&link, packet).await
                ));
            }
            Err(_) => {
                self.peer
                    .send_response_resource(&event.destination, id.to_vec(), data, None)
                    .await
                    .expect("response Resource");
            }
        }
    }
    async fn drive(
        &mut self,
        task: &JoinHandle<Result<PropagationFetchReport, SdkError>>,
        saved: &AtomicBool,
        python: bool,
        oversized_inventory: bool,
    ) -> usize {
        let mut haves = 0;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !task.is_finished() {
            tokio::select! {
                Some(message) = self.outgoing.tx_channel.recv() => forward(&self.incoming, message.packet).await,
                Some(message) = self.incoming.tx_channel.recv() => forward(&self.outgoing, message.packet).await,
                Ok(event) = self.requests.recv() => {
                    if event.context != Some(PacketContext::Request) { continue; }
                    let request: rmpv::Value = rmp_serde::from_slice(event.data.as_slice()).expect("request envelope");
                    let args = request.as_array().expect("request")[2].as_array().expect("get args");
                    let response = if args[0].is_nil() && args[1].is_nil() {
                        if oversized_inventory {
                            rmpv::Value::Array((0u16..100).map(|index| rmpv::Value::Binary(vec![u8::try_from(index).expect("index");32])).collect())
                        } else { rmpv::Value::Array(vec![rmpv::Value::Binary(Hash::new_from_slice(&self.transient).as_slice().to_vec())]) }
                    } else if args[0].is_nil() {
                        assert!(saved.load(Ordering::Acquire), "haves must follow durable import");
                        haves += 1;
                        if python { rmpv::Value::Array(vec![]) } else { rmpv::Value::Boolean(true) }
                    } else { rmpv::Value::Array(vec![rmpv::Value::Binary(self.transient.clone())]) };
                    self.respond(&event, response).await;
                },
                _ = tokio::time::sleep_until(deadline) => panic!("fetch did not finish"),
                _ = tokio::time::sleep(Duration::from_millis(1)) => {},
            }
        }
        haves
    }
}
fn limits() -> PropagationFetchLimits {
    PropagationFetchLimits {
        max_messages: 1,
        max_response_bytes: 2048,
        request_timeout: Duration::from_millis(500),
        link_timeout: Duration::from_secs(1),
        link_attempts: 1,
    }
}

#[tokio::test]
async fn native_fetch_confirms_only_after_import_and_accepts_python_haves_shape() {
    for (durable, python) in [(true, false), (true, true), (false, false)] {
        let mut pair = Pair::new().await;
        let expected = pair.wire.clone();
        let saved = Arc::new(AtomicBool::new(false));
        let observer = saved.clone();
        let config = pair.config.clone();
        let relay = pair.relay;
        let task = tokio::spawn(fetch_retained(config, relay, limits(), None, move |wire| {
            assert_eq!(wire, expected);
            observer.store(durable, Ordering::Release);
            std::future::ready(durable)
        }));
        let haves = pair.drive(&task, &saved, python, false).await;
        let result = task.await.expect("waiter").expect("fetch");
        assert_eq!(result.imported, usize::from(durable));
        assert_eq!(result.confirmed, usize::from(durable));
        assert_eq!(haves, usize::from(durable));
        assert_eq!(pair.link.lock().await.status(), LinkStatus::Closed);
    }
}

#[tokio::test]
async fn oversized_inventory_is_rejected_without_import_or_haves() {
    let mut pair = Pair::new().await;
    let saved = AtomicBool::new(false);
    let task =
        tokio::spawn(fetch_retained(pair.config.clone(), pair.relay, limits(), None, |_| async {
            panic!("oversized inventory must never import")
        }));
    assert_eq!(pair.drive(&task, &saved, false, true).await, 0);
    assert!(task.await.expect("waiter").is_err());
    assert_eq!(pair.link.lock().await.status(), LinkStatus::Closed);
}

#[tokio::test]
async fn dropped_waiter_keeps_admission_until_native_timeout_cleanup() {
    let mut pair = Pair::new().await;
    let admission = Arc::new(Semaphore::new(1));
    let permit = admission.clone().acquire_owned().await.expect("permit");
    let task = tokio::spawn(fetch_retained(
        pair.config.clone(),
        pair.relay,
        limits(),
        Some(permit),
        |_| async { panic!("no response") },
    ));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        tokio::select! {
            Some(message) = pair.outgoing.tx_channel.recv() => forward(&pair.incoming, message.packet).await,
            Some(message) = pair.incoming.tx_channel.recv() => forward(&pair.outgoing, message.packet).await,
            Ok(event) = pair.requests.recv() => if event.context == Some(PacketContext::Request) { break; },
            _ = tokio::time::sleep_until(deadline) => panic!("no request"),
        }
    }
    task.abort();
    assert!(task.await.expect_err("cancelled waiter").is_cancelled());
    assert!(admission.clone().try_acquire_owned().is_err());
    tokio::time::timeout(Duration::from_secs(2), async {
        while admission.available_permits() == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("retained owner cleanup");
    assert_eq!(pair.link.lock().await.status(), LinkStatus::Closed);
}

#[test]
fn malformed_inventory_and_wrong_transient_never_count_as_empty_or_delivered() {
    assert!(inventory_ids(rmpv::Value::Nil).is_err());
    assert!(inventory_ids(rmpv::Value::Array(vec![rmpv::Value::Binary(vec![1; 16])])).is_err());
    assert!(single_payload(rmpv::Value::Array(vec![rmpv::Value::Binary(vec![2; 100])]), &[1; 32])
        .is_err());
}

#[test]
fn compatible_response_wrappers_keep_the_original_correlated_transient() {
    let payload = vec![0x33; 120];
    let id = Hash::new_from_slice(&payload).as_slice().to_vec();
    let envelope = rmpv::Value::Array(vec![
        rmpv::Value::F64(1.0),
        rmpv::Value::Array(vec![rmpv::Value::Binary(payload.clone())]),
    ]);
    for response in [
        rmpv::Value::Array(vec![rmpv::Value::Binary(payload.clone())]),
        rmpv::Value::Array(vec![rmpv::Value::Array(vec![
            rmpv::Value::Binary(id.clone()),
            rmpv::Value::Binary(payload.clone()),
        ])]),
        rmpv::Value::Array(vec![rmpv::Value::Binary(
            rmp_serde::to_vec(&envelope).expect("envelope"),
        )]),
        envelope,
    ] {
        assert_eq!(
            single_payload(response, &id).expect("compatible response"),
            Some(payload.clone())
        );
    }
}

#[tokio::test]
async fn relay_cannot_manufacture_a_message_from_a_known_sender() {
    let mut pair = Pair::new().await;
    let mut message = WireMessage::unpack(&pair.wire).expect("wire");
    message.signature.as_mut().expect("signature")[0] ^= 1;
    let recipient = lxmf_core::identity::PrivateIdentity::from_private_key_bytes(
        &pair.config.identity.to_private_key_bytes(),
    )
    .expect("recipient");
    pair.transient = message
        .pack_propagation_transient_with_rng(recipient.as_identity(), OsRng)
        .expect("transient")
        .0;
    let task =
        tokio::spawn(fetch_retained(pair.config.clone(), pair.relay, limits(), None, |_| async {
            panic!("forged source must not reach app import")
        }));
    assert_eq!(pair.drive(&task, &AtomicBool::new(false), false, false).await, 0);
    let result = task.await.expect("waiter").expect("fetch");
    assert_eq!(result.failed, 1);
    assert_eq!(result.imported, 0);
}
