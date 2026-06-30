mod direct_delivery_success_sdk_events {
    use rand_core::OsRng;
    use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
    use rns_rpc::{RpcDaemon, RpcRequest};
    use rns_transport::destination::{DestinationName, SingleInputDestination};
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::identity_bridge::{
        to_core_private_identity, to_transport_private_identity,
    };
    use rns_transport::transport::{ReceivedPayloadMode, Transport, TransportConfig};
    use serde_json::{json, Value};

    use super::super::delivery_events;

    #[tokio::test]
    async fn direct_packet_delivery_is_visible_to_sdk_poll_events_with_python_callback_metadata() {
        let daemon = RpcDaemon::test_instance();
        let (transport, source_private, source_hash, destination_hash) =
            direct_delivery_context().await;
        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "direct packet event title",
            "direct packet event content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let cursor = poll_cursor(&daemon, 170);

        delivery_events::accept_delivery_packet(
            &daemon,
            &transport,
            hex::encode(destination_hash).as_str(),
            destination_hash,
            &wire,
            ReceivedPayloadMode::FullWire,
        )
        .await;

        assert_direct_success_event(
            &daemon,
            cursor.as_str(),
            171,
            &wire,
            "direct packet event title",
            "direct packet event content",
        );
    }

    #[tokio::test]
    async fn direct_resource_delivery_is_visible_to_sdk_poll_events_with_python_callback_metadata()
    {
        let daemon = RpcDaemon::test_instance();
        let (transport, source_private, source_hash, destination_hash) =
            direct_delivery_context().await;
        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "direct resource event title",
            "direct resource event content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let cursor = poll_cursor(&daemon, 172);

        delivery_events::accept_delivery_resource(&daemon, &transport, destination_hash, &wire)
            .await;

        assert_direct_success_event(
            &daemon,
            cursor.as_str(),
            173,
            &wire,
            "direct resource event title",
            "direct resource event content",
        );
    }

    async fn direct_delivery_context() -> (Transport, PrivateIdentity, [u8; 16], [u8; 16]) {
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
        let transport_identity =
            to_transport_private_identity(&to_core_private_identity(&delivery_private));
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let delivery_destination = SingleInputDestination::new(
            delivery_private,
            DestinationName::new("lxmf", "delivery"),
        );
        let mut source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        destination_hash.copy_from_slice(delivery_destination.desc.address_hash.as_slice());
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());

        remember_source_destination(&transport, &mut source_destination).await;

        (transport, source_private, source_hash, destination_hash)
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

    fn poll_cursor(daemon: &RpcDaemon, id: u64) -> String {
        let pre_poll = daemon
            .handle_rpc(RpcRequest {
                id,
                method: "sdk_poll_events_v2".to_string(),
                params: Some(json!({ "cursor": null, "max": 20 })),
            })
            .expect("pre poll sdk events")
            .result
            .expect("pre poll result");
        pre_poll["next_cursor"].as_str().expect("pre cursor").to_owned()
    }

    fn assert_direct_success_event(
        daemon: &RpcDaemon,
        cursor: &str,
        poll_id: u64,
        wire: &[u8],
        expected_title: &str,
        expected_content: &str,
    ) {
        let messages = daemon
            .handle_rpc(RpcRequest {
                id: poll_id + 100,
                method: "list_messages".to_string(),
                params: None,
            })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert_eq!(items.len(), 1);
        let stored = &items[0];
        assert_eq!(stored["direction"], json!("in"));
        assert_eq!(stored["title"], json!(expected_title));
        assert_eq!(stored["content"], json!(expected_content));
        assert_direct_lxmf_metadata(stored);

        let poll = daemon
            .handle_rpc(RpcRequest {
                id: poll_id,
                method: "sdk_poll_events_v2".to_string(),
                params: Some(json!({ "cursor": cursor, "max": 20 })),
            })
            .expect("poll sdk events")
            .result
            .expect("poll result");
        let events = poll["events"].as_array().expect("event rows");
        let event = events
            .iter()
            .find(|event| event["event_type"] == json!("inbound"))
            .expect("sdk inbound event");
        assert_eq!(event["payload"]["lxmf_bytes_hex"], json!(hex::encode(wire)));
        let event_message = &event["payload"]["message"];
        assert_eq!(event_message["id"], stored["id"]);
        assert_eq!(event_message["title"], json!(expected_title));
        assert_eq!(event_message["content"], json!(expected_content));
        assert_direct_lxmf_metadata(event_message);
    }

    fn assert_direct_lxmf_metadata(record: &Value) {
        let lxmf = &record["fields"]["_lxmf"];
        assert_eq!(lxmf["method"], json!(2));
        assert_eq!(lxmf["transport_encrypted"], json!(true));
        assert_eq!(lxmf["transport_encryption"], json!("Curve25519"));
        assert_eq!(lxmf["signature_checked"], json!(true));
        assert_eq!(lxmf["signature_valid"], json!(true));
        assert_eq!(lxmf["signature_status"], json!("verified"));
    }
}
