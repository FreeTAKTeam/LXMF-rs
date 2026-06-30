#[cfg(test)]
mod signature_metadata_tests {
    use super::*;
    use lxmf::WireMessage;
    use rand_core::OsRng;
    use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
    use rns_transport::destination::DestinationName;
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::identity_bridge::{to_core_identity, to_core_private_identity};
    use std::sync::Arc;
    use tokio::sync::Mutex as TokioMutex;

    #[tokio::test]
    async fn stored_downloaded_payload_records_signature_metadata() {
        let daemon = RpcDaemon::test_instance();
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
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

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "downloaded signature title",
            "downloaded signature content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let transient_payload = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient")
                .0
        };

        let outcome = accept_downloaded_propagation_payload(
            &daemon,
            &delivery_destination,
            transient_payload.as_slice(),
            None,
        )
        .await
        .expect("accept downloaded payload");

        assert_eq!(outcome, DownloadAcceptOutcome::Stored);
        let messages = daemon
            .handle_rpc(RpcRequest { id: 72, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["fields"]["_lxmf"]["signature_checked"], json!(false));
        assert_eq!(items[0]["fields"]["_lxmf"]["signature_valid"], json!(false));
        assert_eq!(
            items[0]["fields"]["_lxmf"]["signature_status"],
            json!("source_identity_unknown")
        );
        let event = daemon.take_event().expect("downloaded inbound event");
        assert_eq!(event.event_type, "inbound");
        assert_eq!(event.payload["message"]["fields"]["_lxmf"]["signature_checked"], json!(false));
        assert_eq!(event.payload["message"]["fields"]["_lxmf"]["signature_valid"], json!(false));
        assert_eq!(
            event.payload["message"]["fields"]["_lxmf"]["signature_status"],
            json!("source_identity_unknown")
        );
    }
}
