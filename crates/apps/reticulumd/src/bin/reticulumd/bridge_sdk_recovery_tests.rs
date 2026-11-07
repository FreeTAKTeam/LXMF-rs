//! Public SDK -> canonical ZeroMQ -> production bridge/owner -> native peer.
//! No fake backend responses, injected delivery outcomes or shortened protocol
//! waits. The only faults are the explicitly configured external receipt sink.
use super::*;
use lxmf_sdk::{
    Client, ConfigPatch, DeliveryState, LxmfSdk, MessageId, SdkConfig, SendRequest, StartRequest,
    ZmqPipelineBackendClient, ZmqPipelineBackendConfig,
};
use rns_rpc::{EventSinkBridge, MessagesStore, OutboundBridge, RpcEventSinkEnvelope};
use rns_transport::iface::{IfaceRole, IfaceSource, RxMessage};
use rns_transport::packet::{PacketContext, PacketType};
use rns_transport::transport::{Transport, TransportConfig};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

struct RecoveringSink {
    attempts: AtomicUsize,
    receipts: Mutex<Vec<RpcEventSinkEnvelope>>,
}
impl EventSinkBridge for RecoveringSink {
    fn sink_id(&self) -> &str {
        "native-sdk-recovery"
    }
    fn sink_kind(&self) -> &'static str {
        "custom"
    }
    fn publish(&self, envelope: &RpcEventSinkEnvelope) -> std::io::Result<()> {
        if envelope.event.event_type != "receipt" {
            return Ok(());
        }
        match self.attempts.fetch_add(1, Ordering::SeqCst) {
            0 => return Err(std::io::Error::other("external receipt sink rejected notification")),
            1 => panic!("external receipt sink panicked"),
            _ => self.receipts.lock().expect("recovered sink").push(envelope.clone()),
        }
        Ok(())
    }
}
const CONTENT: &str = "SDK delivery after receipt callback error and panic";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn canonical_sdk_delivers_after_real_failed_sends_and_callback_error_and_panic() {
    let directory = tempfile::tempdir().expect("persistent qualification store");
    let path = directory.path().join("messages.db");
    let store = MessagesStore::open(&path).expect("messages");
    store.enable_durable_broker(32 * 1024 * 1024).expect("durable broker");
    let signer = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let native_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport =
        Arc::new(Transport::new(TransportConfig::new("sdk-recovery", &native_identity, true)));
    let local =
        transport.add_destination(native_identity, DestinationName::new("lxmf", "delivery")).await;
    let source = local.lock().await.desc.address_hash;
    let (sender, receiver) = tokio::sync::mpsc::channel(32);
    let publisher = reticulum_daemon::receipt_bridge::ReceiptPublisher::from(sender);
    let receipts = Arc::new(Mutex::new(HashMap::new()));
    let resources = Arc::new(Mutex::new(HashMap::new()));
    let bridge = Arc::new(TransportBridge::new(
        transport.clone(),
        directory.path().join("identities"),
        signer,
        source.as_slice().try_into().expect("source hash"),
        local,
        None,
        None,
        Vec::new(),
        None,
        None,
        None,
        Default::default(),
        receipts.clone(),
        resources.clone(),
        publisher.clone(),
    ));
    let sink = Arc::new(RecoveringSink {
        attempts: AtomicUsize::new(0),
        receipts: Mutex::new(Vec::new()),
    });
    let daemon = Arc::new(RpcDaemon::with_store_and_bridges_and_sinks(
        store,
        source.to_hex_string(),
        Some(bridge.clone()),
        None,
        vec![sink.clone()],
    ));
    bridge.set_daemon(daemon.clone());
    transport
        .set_receipt_handler(Box::new(reticulum_daemon::receipt_bridge::ReceiptBridge::new(
            receipts.clone(),
            publisher.clone(),
        )))
        .await;
    let receipt_owner =
        crate::receipt_worker::spawn_receipt_worker(daemon.clone(), receiver, receipts, publisher);
    let remote_identity = rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let peer =
        Arc::new(Transport::new(TransportConfig::new("sdk-recovery-peer", &remote_identity, true)));
    let destination =
        peer.add_destination(remote_identity, DestinationName::new("lxmf", "delivery")).await;
    let target = destination.lock().await.desc.address_hash;
    let mut incoming = peer.received_data_events();
    let mut outgoing =
        transport.iface_manager().lock().await.new_channel_with_role(64, IfaceRole::Unicast);
    let mut returning =
        peer.iface_manager().lock().await.new_channel_with_role(64, IfaceRole::Unicast);
    let announcement_ingress = outgoing.rx_channel.clone();
    let announcement_iface = *outgoing.address();
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let relay = tokio::spawn(async move {
        let mut requests = 0;
        let mut packets = 0;
        loop {
            tokio::select! {
                _ = stopped.changed() => break,
                frame = outgoing.tx_channel.recv() => {
                    let frame = frame.expect("sender native frame").packet;
                    requests += usize::from(frame.header.packet_type == PacketType::LinkRequest);
                    packets += usize::from(frame.header.packet_type == PacketType::Data && frame.context == PacketContext::None && frame.header.destination_type == rns_transport::packet::DestinationType::Link);
                    returning.rx_channel.send(RxMessage { address: *returning.address(), packet: frame, source: IfaceSource::None }).await.expect("native peer ingress");
                },
                frame = returning.tx_channel.recv() => {
                    outgoing.rx_channel.send(RxMessage { address: *outgoing.address(), packet: frame.expect("peer frame").packet, source: IfaceSource::None }).await.expect("native proof ingress");
                },
            }
        }
        (requests, packets)
    });
    let reserved =
        std::net::TcpListener::bind("127.0.0.1:0").expect("canonical endpoint reservation");
    let endpoint = format!("tcp://{}", reserved.local_addr().expect("endpoint"));
    drop(reserved);
    let server = tokio::spawn(crate::zmq_rpc_loop::run_zmq_router_loop_until(
        endpoint.clone(),
        true,
        daemon.clone(),
        stop.subscribe(),
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (failed, first_failures) = tokio::sync::oneshot::channel();
    let (ready, proceed) = std::sync::mpsc::sync_channel(1);
    let callback_sink = sink.clone();
    let source_hex = source.to_hex_string();
    let client = tokio::task::spawn_blocking(move || {
        let client = Client::new(
            ZmqPipelineBackendClient::new(ZmqPipelineBackendConfig::local(endpoint))
                .expect("real SDK backend"),
        );
        client
            .start(
                StartRequest::new(SdkConfig::desktop_full_default()).with_requested_capabilities([
                    "sdk.capability.event_sink_bridge",
                    "sdk.capability.receipt_terminality",
                    "sdk.capability.cursor_replay",
                    "sdk.capability.config_revision_cas",
                ]),
            )
            .expect("SDK start");
        let revision = client.snapshot().expect("SDK runtime snapshot").config_revision;
        let patch: ConfigPatch = serde_json::from_value(serde_json::json!({
            "event_sink": {"enabled": true, "allow_kinds": ["custom"]},
        }))
        .expect("typed public sink configuration");
        client.configure(revision, patch).expect("SDK enables external receipt sink");
        let request = |destination: String, content: &str| {
            SendRequest::new(
                source_hex.clone(),
                destination,
                serde_json::json!({"title":"recovery", "content":content}),
            )
            .with_delivery_method("direct")
            .with_stamp_cost(0)
            .with_ttl_ms(120_000)
            .with_try_propagation_on_fail(false)
        };
        let failed_ids = [
            client
                .send(request(hex::encode([0x96; 16]), "first missing peer"))
                .expect("admit first failure"),
            client
                .send(request(hex::encode([0x97; 16]), "second missing peer"))
                .expect("admit second failure"),
        ];
        for id in &failed_ids {
            wait_status(&client, id, DeliveryState::Failed, Duration::from_secs(70));
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while callback_sink.attempts.load(Ordering::SeqCst) < 2 {
            assert!(Instant::now() < deadline, "both callback faults were observed");
            std::thread::sleep(Duration::from_millis(10));
        }
        failed.send(()).expect("failed-send barrier");
        proceed.recv_timeout(Duration::from_secs(5)).expect("real signed peer path");
        let delivered = client
            .send(request(target.to_hex_string(), CONTENT))
            .expect("SDK admits later delivery");
        wait_status(&client, &delivered, DeliveryState::Delivered, Duration::from_secs(12));
        for id in &failed_ids {
            wait_status(&client, id, DeliveryState::Failed, Duration::from_secs(1));
        }
        let batch = client.poll_events(None, 64).expect("public SDK event replay");
        for id in failed_ids.iter().chain(std::iter::once(&delivered)) {
            assert!(
                batch.events.iter().any(|event| event.event_type == "receipt"
                    && event.payload["message_id"] == id.0
                    && event.payload["updated"] == true),
                "SDK replay includes committed receipt for {id:?}: {:?}",
                batch.events
            );
        }
        (failed_ids, delivered)
    });
    tokio::time::timeout(Duration::from_secs(80), first_failures)
        .await
        .expect("normal protocol exhaustion deadline")
        .expect("callback fault barrier");
    let announcement = destination
        .lock()
        .await
        .announce(rand_core::OsRng, None)
        .expect("real signed recipient announce");
    announcement_ingress
        .send(RxMessage {
            address: announcement_iface,
            packet: announcement,
            source: IfaceSource::None,
        })
        .await
        .expect("recipient announce ingress");
    tokio::time::timeout(Duration::from_secs(3), async {
        while !transport.has_path(&target).await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("learned native peer path");
    ready.send(()).expect("SDK resumes through the same backend");
    let payload = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let data = incoming.recv().await.expect("recipient native data");
            if data.context == Some(PacketContext::None) {
                break data.data.as_slice().to_vec();
            }
        }
    })
    .await
    .expect("recipient observes later successful send");
    let (failed_ids, delivered) = tokio::time::timeout(Duration::from_secs(20), client)
        .await
        .expect("SDK operation deadline")
        .expect("SDK client task");
    assert!(
        daemon
            .durable_prepared_payload(&delivered.0)
            .expect("stored preparation")
            .flatten()
            .is_none(),
        "terminal commit retires durable wire preparation"
    );
    let decoded = lxmf::WireMessage::unpack(&payload).expect("recipient LXMF decode");
    assert_eq!(
        decoded.payload.content.as_ref().expect("recipient content").as_ref(),
        CONTENT.as_bytes()
    );
    assert_eq!(decoded.destination.as_slice(), target.as_slice());
    assert_eq!(decoded.source.as_slice(), source.as_slice());
    assert!(decoded.verify(bridge.signer.as_identity()).expect("recipient signature validation"));
    tokio::time::timeout(Duration::from_secs(3), async {
        while sink.receipts.lock().expect("recovered sink").is_empty()
            || failed_ids
                .iter()
                .chain(std::iter::once(&delivered))
                .any(|id| bridge.owns_delivery(&id.0))
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("callback recovers and all admission retires");
    assert_eq!(sink.attempts.load(Ordering::SeqCst), 3);
    assert_eq!(
        sink.receipts.lock().expect("recovered sink")[0].event.payload["message_id"],
        delivered.0
    );
    assert_eq!(daemon.metrics_snapshot()["counters"]["sdk_event_sink_error_total"], 2);
    for id in &failed_ids {
        assert_eq!(
            daemon.message_receipt_status(&id.0).expect("failed status").as_deref(),
            Some("failed: Link delivery attempts exhausted")
        );
    }
    assert_eq!(
        daemon.message_receipt_status(&delivered.0).expect("success status").as_deref(),
        Some("delivered")
    );
    assert!(resources.lock().expect("Resource ownership").is_empty());
    let pipeline = bridge.delivery_pipeline_status().expect("pipeline status");
    assert_eq!(pipeline["queued_total"], 0);
    assert_eq!(pipeline["in_flight_total"], 0);
    stop.send_replace(true);
    let (requests, packets) = relay.await.expect("native relay joins");
    assert_eq!(
        (requests, packets),
        (1, 1),
        "only the successful send establishes a Link and hands off data"
    );
    server.await.expect("canonical router joins").expect("canonical shutdown");
    bridge.shutdown_delivery();
    receipt_owner.drain().await.expect("receipt owner drains");
    let connection = rusqlite::Connection::open(path).expect("independent persisted record read");
    for id in failed_ids.iter().chain(std::iter::once(&delivered)) {
        assert_eq!(connection.query_row(
            "SELECT COUNT(*) FROM broker_events WHERE event_type='receipt' AND json_extract(payload,'$.message_id')=?1 AND (json_extract(payload,'$.status')='delivered' OR json_extract(payload,'$.status') LIKE 'failed:%')",
            [&id.0], |row| row.get::<_, i64>(0),
        ).expect("persisted terminal receipt"), 1, "one terminal outcome per SDK operation");
    }
}

fn wait_status(
    client: &Client<ZmqPipelineBackendClient>,
    id: &MessageId,
    state: DeliveryState,
    limit: Duration,
) {
    let deadline = Instant::now() + limit;
    loop {
        let status = client.status(id.clone()).expect("real SDK status");
        if status.as_ref().is_some_and(|status| status.state == state && status.terminal) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "SDK status deadline id={id:?} expected={state:?} actual={status:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
