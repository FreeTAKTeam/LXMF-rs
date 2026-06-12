fn built_in_entries() -> Vec<OperationEntry> {
    vec![
        OperationEntry::new(
            "app.runtime.start",
            "runtime",
            OperationKind::Command,
            TransportVariant::App,
            "Start or attach to the configured runtime session.",
        )
        .with_alias("sdk_negotiate_v2")
        .with_alias("sdk_configure_v2")
        .with_alias("sdk_start_v2"),
        OperationEntry::new(
            "app.runtime.restart",
            "runtime",
            OperationKind::Command,
            TransportVariant::App,
            "Restart the runtime with a new app configuration.",
        ),
        OperationEntry::new(
            "app.runtime.stop",
            "runtime",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Stop the runtime session.",
        )
        .with_alias("sdk_shutdown_v2"),
        OperationEntry::new(
            "app.runtime.status",
            "runtime",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Return runtime status and queue counters.",
        )
        .with_alias("sdk_snapshot_v2"),
        OperationEntry::new(
            "app.delivery.send",
            "delivery",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Queue one outbound message for delivery.",
        )
        .with_alias("sdk_send_v2"),
        OperationEntry::new(
            "app.delivery.send_batch",
            "delivery",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Queue a batch of outbound messages for delivery.",
        )
        .with_alias("sdk_send_batch_v2")
        .with_required_capability("sdk.capability.batch_send"),
        OperationEntry::new(
            "app.delivery.status",
            "delivery",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Return delivery state for a specific message id.",
        )
        .with_alias("sdk_status_v2"),
        OperationEntry::new(
            "app.event.poll",
            "events",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Poll batches of runtime events.",
        )
        .with_alias("sdk_poll_events_v2"),
        OperationEntry::new(
            "app.event.subscribe",
            "events",
            OperationKind::Query,
            TransportVariant::App,
            "Subscribe to the async runtime event stream.",
        )
        .with_alias("sdk_subscribe_events_v2")
        .with_required_capability("sdk.capability.async_events"),
        OperationEntry::new(
            "app.identity.list",
            "identity",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List identities visible to the runtime.",
        )
        .with_alias("sdk_identity_list_v2")
        .with_required_capability("sdk.capability.identity_multi"),
        OperationEntry::new(
            "app.identity.announce",
            "identity",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Trigger an announce for the active identity.",
        )
        .with_alias("sdk_identity_announce_now_v2")
        .with_required_capability("sdk.capability.identity_discovery"),
        OperationEntry::new(
            "app.identity.presence.list",
            "identity",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List recently seen peers and announce-derived presence state.",
        )
        .with_alias("sdk_identity_presence_list_v2")
        .with_required_capability("sdk.capability.identity_discovery"),
        OperationEntry::new(
            "app.contact.list",
            "identity",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List contacts for a selected identity.",
        )
        .with_alias("sdk_identity_contact_list_v2")
        .with_required_capability("sdk.capability.contact_management"),
        OperationEntry::new(
            "app.contact.update",
            "identity",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Create or update a contact record for an identity.",
        )
        .with_alias("sdk_identity_contact_update_v2")
        .with_required_capability("sdk.capability.contact_management"),
        OperationEntry::new(
            "app.identity.bootstrap",
            "identity",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Bootstrap trust and optional sync state for an identity.",
        )
        .with_alias("sdk_identity_bootstrap_v2")
        .with_required_capability("sdk.capability.contact_management"),
        OperationEntry::new(
            "app.topic.create",
            "topics",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Create a topic record for collaborative app flows.",
        )
        .with_alias("sdk_topic_create_v2")
        .with_required_capability("sdk.capability.topics"),
        OperationEntry::new(
            "app.topic.get",
            "topics",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Fetch one topic record by id.",
        )
        .with_alias("sdk_topic_get_v2")
        .with_required_capability("sdk.capability.topics"),
        OperationEntry::new(
            "app.topic.list",
            "topics",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List known topics with cursor pagination.",
        )
        .with_alias("sdk_topic_list_v2")
        .with_required_capability("sdk.capability.topics"),
        OperationEntry::new(
            "app.topic.subscribe",
            "topics",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Subscribe the runtime to topic updates.",
        )
        .with_alias("sdk_topic_subscribe_v2")
        .with_required_capability("sdk.capability.topic_subscriptions"),
        OperationEntry::new(
            "app.topic.unsubscribe",
            "topics",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Remove a topic subscription from the runtime.",
        )
        .with_alias("sdk_topic_unsubscribe_v2")
        .with_required_capability("sdk.capability.topic_subscriptions"),
        OperationEntry::new(
            "app.topic.publish",
            "topics",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Publish one payload fanout to a topic.",
        )
        .with_alias("sdk_topic_publish_v2")
        .with_required_capability("sdk.capability.topic_fanout"),
        OperationEntry::new(
            "app.telemetry.query",
            "telemetry",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Query telemetry points filtered by peer, topic, and time bounds.",
        )
        .with_alias("sdk_telemetry_query_v2")
        .with_required_capability("sdk.capability.telemetry_query"),
        OperationEntry::new(
            "app.telemetry.subscribe",
            "telemetry",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Subscribe the runtime to telemetry stream updates.",
        )
        .with_alias("sdk_telemetry_subscribe_v2")
        .with_required_capability("sdk.capability.telemetry_stream"),
        OperationEntry::new(
            "app.attachment.store",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Store one attachment payload with optional topic associations.",
        )
        .with_alias("sdk_attachment_store_v2")
        .with_required_capability("sdk.capability.attachments"),
        OperationEntry::new(
            "app.attachment.get",
            "attachments",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Fetch one attachment metadata record by id.",
        )
        .with_alias("sdk_attachment_get_v2")
        .with_required_capability("sdk.capability.attachments"),
        OperationEntry::new(
            "app.attachment.list",
            "attachments",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List stored attachments with topic filtering and cursor pagination.",
        )
        .with_alias("sdk_attachment_list_v2")
        .with_required_capability("sdk.capability.attachments"),
        OperationEntry::new(
            "app.attachment.delete",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Delete one stored attachment by id.",
        )
        .with_alias("sdk_attachment_delete_v2")
        .with_required_capability("sdk.capability.attachment_delete"),
        OperationEntry::new(
            "app.attachment.associate_topic",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Associate an existing attachment with an additional topic.",
        )
        .with_alias("sdk_attachment_associate_topic_v2")
        .with_required_capability("sdk.capability.attachments"),
        OperationEntry::new(
            "app.attachment.upload_start",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Open a chunked attachment upload session.",
        )
        .with_alias("sdk_attachment_upload_start_v2")
        .with_required_capability("sdk.capability.attachment_streaming"),
        OperationEntry::new(
            "app.attachment.upload_chunk",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Append one chunk to an attachment upload session.",
        )
        .with_alias("sdk_attachment_upload_chunk_v2")
        .with_required_capability("sdk.capability.attachment_streaming"),
        OperationEntry::new(
            "app.attachment.upload_commit",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Commit a completed attachment upload session.",
        )
        .with_alias("sdk_attachment_upload_commit_v2")
        .with_required_capability("sdk.capability.attachment_streaming"),
        OperationEntry::new(
            "app.attachment.download_chunk",
            "attachments",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Read one chunk from a stored attachment payload.",
        )
        .with_alias("sdk_attachment_download_chunk_v2")
        .with_required_capability("sdk.capability.attachment_streaming"),
        OperationEntry::new(
            "app.marker.create",
            "markers",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Create a shared marker anchored to an optional topic.",
        )
        .with_alias("sdk_marker_create_v2")
        .with_required_capability("sdk.capability.markers"),
        OperationEntry::new(
            "app.marker.list",
            "markers",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List markers with topic filtering and cursor pagination.",
        )
        .with_alias("sdk_marker_list_v2")
        .with_required_capability("sdk.capability.markers"),
        OperationEntry::new(
            "app.marker.update_position",
            "markers",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Move an existing marker while enforcing revision checks.",
        )
        .with_alias("sdk_marker_update_position_v2")
        .with_required_capability("sdk.capability.markers"),
        OperationEntry::new(
            "app.marker.delete",
            "markers",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Delete an existing marker while enforcing revision checks.",
        )
        .with_alias("sdk_marker_delete_v2")
        .with_required_capability("sdk.capability.markers"),
        OperationEntry::new(
            "app.workflow.peer_ready",
            "workflow",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Ensure a peer contact exists and optionally announce before use.",
        )
        .with_alias("sdk_workflow_peer_ready_v2")
        .with_required_capability("sdk.capability.contact_management")
        .with_required_capability("sdk.capability.identity_discovery"),
        OperationEntry::new(
            "app.workflow.topic_sync",
            "workflow",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Ensure a topic exists, subscribe to it, and fetch a telemetry snapshot.",
        )
        .with_alias("sdk_workflow_topic_sync_v2")
        .with_required_capability("sdk.capability.topics")
        .with_required_capability("sdk.capability.topic_subscriptions")
        .with_required_capability("sdk.capability.telemetry_query"),
        OperationEntry::new(
            "app.workflow.attachment_report_publish",
            "workflow",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Ensure a topic, store an attachment, and publish a summary report.",
        )
        .with_alias("sdk_workflow_attachment_report_publish_v2")
        .with_required_capability("sdk.capability.topics")
        .with_required_capability("sdk.capability.attachments")
        .with_required_capability("sdk.capability.topic_fanout"),
        OperationEntry::new(
            "app.workflow.mission_update_send",
            "workflow",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Ensure peer and optional topic state, store attachments, and send a mission update.",
        )
        .with_alias("sdk_workflow_mission_update_send_v2")
        .with_required_capability("sdk.capability.contact_management")
        .with_required_capability("sdk.capability.topics")
        .with_required_capability("sdk.capability.attachments"),
        OperationEntry::new(
            "app.voice.session.open",
            "voice",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Open a voice signaling session for a peer.",
        )
        .with_alias("sdk_voice_session_open_v2")
        .with_required_capability("sdk.capability.voice_signaling"),
        OperationEntry::new(
            "app.voice.session.update",
            "voice",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Advance the state of a voice signaling session.",
        )
        .with_alias("sdk_voice_session_update_v2")
        .with_required_capability("sdk.capability.voice_signaling"),
        OperationEntry::new(
            "app.voice.session.close",
            "voice",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Close a voice signaling session.",
        )
        .with_alias("sdk_voice_session_close_v2")
        .with_required_capability("sdk.capability.voice_signaling"),
        OperationEntry::new(
            "app.message.history.list",
            "messaging",
            OperationKind::Query,
            TransportVariant::LegacyRpc,
            "List message history records for app chat flows.",
        )
        .with_alias("list_messages"),
        OperationEntry::new(
            "app.delivery.destination_hash",
            "identity",
            OperationKind::Query,
            TransportVariant::LegacyRpc,
            "Resolve the runtime delivery destination hash.",
        )
        .with_alias("status"),
    ]
}
