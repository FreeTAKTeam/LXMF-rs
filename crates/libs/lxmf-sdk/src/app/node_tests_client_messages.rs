#[test]
fn messages_domain_lists_typed_history_without_raw_envelope_decoding() {
    let backend = MockBackend::new();
    backend.queue_envelope_result(Ok(crate::app::EnvelopeResponse {
        operation_id: crate::app::OperationId::from("app.message.history.list"),
        kind: EnvelopeKind::Result,
        accepted: true,
        correlation_id: None,
        payload: json!({
            "messages": [{
                "id": "msg-1",
                "source": "peer-src",
                "destination": "local-dst",
                "title": "Check in",
                "content": "All clear",
                "timestamp": 42,
                "direction": "inbound",
                "fields": {
                    "_lxmf": {
                        "signature_status": "verified",
                        "stamp_status": "accepted"
                    }
                },
                "receipt_status": "delivered:receipt"
            }],
            "next_cursor": "cursor-2"
        }),
        extensions: BTreeMap::new(),
    }));

    let app = Client::new(backend);
    let page = app
        .messages()
        .history(crate::MessageHistoryListRequest {
            peer_id: Some("peer-src".to_owned()),
            conversation_id: Some("conv-1".to_owned()),
            include_receipts: Some(true),
            limit: Some(25),
            before_ts: None,
            cursor: Some("cursor-1".to_owned()),
        })
        .expect("history page");

    assert_eq!(page.next_cursor.as_deref(), Some("cursor-2"));
    assert_eq!(page.messages.len(), 1);
    let message = &page.messages[0];
    assert_eq!(message.id, "msg-1");
    assert_eq!(message.source, "peer-src");
    assert_eq!(message.destination, "local-dst");
    assert_eq!(message.content, "All clear");
    assert_eq!(message.receipt_status.as_deref(), Some("delivered:receipt"));
    assert_eq!(
        message
            .fields
            .as_ref()
            .and_then(|fields| fields.pointer("/_lxmf/signature_status"))
            .and_then(serde_json::Value::as_str),
        Some("verified")
    );
}

#[test]
fn messages_domain_lists_typed_conversations_without_raw_envelope_decoding() {
    let backend = MockBackend::new();
    backend.queue_envelope_result(Ok(crate::app::EnvelopeResponse {
        operation_id: crate::app::OperationId::from("app.message.conversation.list"),
        kind: EnvelopeKind::Result,
        accepted: true,
        correlation_id: None,
        payload: json!({
            "conversations": [{
                "conversation_id": "conv-1",
                "peer_destination_hex": "peer-dst",
                "peer_display_name": "REM Team",
                "last_message_preview": "All clear",
                "last_message_at_ms": 42,
                "unread_count": 2,
                "last_message_state": "delivered"
            }],
            "next_cursor": null
        }),
        extensions: BTreeMap::new(),
    }));

    let app = Client::new(backend);
    let page = app
        .messages()
        .conversations(crate::ConversationListRequest {
            peer_id: Some("peer-dst".to_owned()),
            include_receipts: Some(true),
            limit: Some(10),
            cursor: None,
        })
        .expect("conversation page");

    assert_eq!(page.next_cursor, None);
    assert_eq!(page.conversations.len(), 1);
    let conversation = &page.conversations[0];
    assert_eq!(conversation.conversation_id, "conv-1");
    assert_eq!(conversation.peer_destination_hex, "peer-dst");
    assert_eq!(conversation.peer_display_name.as_deref(), Some("REM Team"));
    assert_eq!(conversation.last_message_preview.as_deref(), Some("All clear"));
    assert_eq!(conversation.unread_count, 2);
    assert_eq!(conversation.last_message_state, Some(crate::MessageState::Delivered));
}
