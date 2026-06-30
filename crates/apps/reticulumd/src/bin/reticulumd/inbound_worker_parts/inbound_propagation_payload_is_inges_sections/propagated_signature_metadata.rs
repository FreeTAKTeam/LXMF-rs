mod propagated_signature_metadata {
    use lxmf::WireMessage;
    use rand_core::OsRng;
    use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
    use reticulum_daemon::lxmf_stamps::generate_propagation_stamp;
    use rns_rpc::{RpcDaemon, RpcRequest};
    use rns_transport::destination::{DestinationName, SingleInputDestination};
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::identity_bridge::{
        to_core_identity, to_core_private_identity, to_transport_private_identity,
    };
    use rns_transport::transport::{Transport, TransportConfig};
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex as TokioMutex;

    use super::super::propagation::ingest_propagation_envelope_with_transport;

    #[tokio::test]
    async fn propagated_local_delivery_marks_known_source_signature_verified() {
        let daemon = RpcDaemon::test_instance();
        enable_signature_test_propagation(&daemon);
        let (transport, delivery_destination, source_private, source_hash, destination_hash) =
            signature_test_context().await;
        let mut source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        remember_source_destination(&transport, &mut source_destination).await;

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "verified propagated title",
            "verified propagated content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let envelope = propagation_envelope_for_wire(&delivery_destination, &wire, 1).await;
        while daemon.take_event().is_some() {}

        let ingested = ingest_propagation_envelope_with_transport(
            &daemon,
            &envelope,
            Some(&delivery_destination),
            Some(&transport),
        )
        .await
        .expect("ingest propagation envelope");
        assert_eq!(ingested, 1);

        assert_signature_metadata(&daemon, true, true, "verified");
    }

    #[tokio::test]
    async fn propagated_local_delivery_marks_known_source_signature_invalid() {
        let daemon = RpcDaemon::test_instance();
        enable_signature_test_propagation(&daemon);
        let (transport, delivery_destination, source_private, source_hash, destination_hash) =
            signature_test_context().await;
        let mut source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        remember_source_destination(&transport, &mut source_destination).await;

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "invalid propagated title",
            "invalid propagated content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let wire = corrupt_lxmf_signature(&wire);
        let envelope = propagation_envelope_for_wire(&delivery_destination, &wire, 1).await;
        while daemon.take_event().is_some() {}

        let ingested = ingest_propagation_envelope_with_transport(
            &daemon,
            &envelope,
            Some(&delivery_destination),
            Some(&transport),
        )
        .await
        .expect("ingest propagation envelope");
        assert_eq!(ingested, 1);

        assert_signature_metadata(&daemon, true, false, "signature_invalid");
    }

    fn enable_signature_test_propagation(daemon: &RpcDaemon) {
        daemon
            .handle_rpc(RpcRequest {
                id: 140,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 1,
                })),
            })
            .expect("enable propagation");
    }

    async fn signature_test_context() -> (
        Transport,
        Arc<TokioMutex<SingleInputDestination>>,
        PrivateIdentity,
        [u8; 16],
        [u8; 16],
    ) {
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
        let transport_identity =
            to_transport_private_identity(&to_core_private_identity(&delivery_private));
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
            delivery_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        )));
        let source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        {
            let destination = delivery_destination.lock().await;
            destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        }
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());

        (transport, delivery_destination, source_private, source_hash, destination_hash)
    }

    async fn remember_source_destination(
        transport: &Transport,
        source_destination: &mut SingleInputDestination,
    ) {
        let iface_channel = transport.iface_manager().lock().await.new_channel(16);
        let iface = *iface_channel.address();
        let source_address = source_destination.desc.address_hash;
        let announce = source_destination.announce(OsRng, None).expect("source announce");
        iface_channel
            .rx_channel
            .send(rns_transport::iface::RxMessage {
                address: iface,
                packet: announce,
                source: rns_transport::iface::IfaceSource::None,
            })
            .await
            .expect("send source announce");

        for _ in 0..80 {
            if transport.destination_identity(&source_address).await.is_some() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("transport should learn source destination identity from announce");
    }

    async fn propagation_envelope_for_wire(
        delivery_destination: &Arc<TokioMutex<SingleInputDestination>>,
        wire: &[u8],
        stamp_cost: u32,
    ) -> Vec<u8> {
        let destination = delivery_destination.lock().await;
        let message = WireMessage::unpack(wire).expect("wire unpack");
        let (transient, transient_id) = message
            .pack_propagation_transient_with_rng(
                &to_core_identity(destination.identity.as_identity()),
                OsRng,
            )
            .expect("propagation transient");
        let stamp =
            generate_propagation_stamp(&transient_id, stamp_cost).expect("propagation stamp");
        WireMessage::pack_propagation_envelope(1.0, &transient, Some(&stamp))
            .expect("propagation envelope")
    }

    fn corrupt_lxmf_signature(wire: &[u8]) -> Vec<u8> {
        let mut message = WireMessage::unpack(wire).expect("wire unpack");
        let mut signature = message.signature.expect("signature");
        signature[0] ^= 0x80;
        message.signature = Some(signature);
        message.pack().expect("corrupted wire")
    }

    fn assert_signature_metadata(
        daemon: &RpcDaemon,
        checked: bool,
        valid: bool,
        status: &str,
    ) {
        let messages = daemon
            .handle_rpc(RpcRequest { id: 141, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["fields"]["_lxmf"]["signature_checked"], json!(checked));
        assert_eq!(items[0]["fields"]["_lxmf"]["signature_valid"], json!(valid));
        assert_eq!(items[0]["fields"]["_lxmf"]["signature_status"], json!(status));

        let event = daemon.take_event().expect("propagated inbound event");
        assert_eq!(event.event_type, "inbound");
        assert_eq!(
            event.payload["message"]["fields"]["_lxmf"]["signature_checked"],
            json!(checked)
        );
        assert_eq!(
            event.payload["message"]["fields"]["_lxmf"]["signature_valid"],
            json!(valid)
        );
        assert_eq!(
            event.payload["message"]["fields"]["_lxmf"]["signature_status"],
            json!(status)
        );
    }
}
