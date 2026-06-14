fn propagation_operation_entries() -> Vec<OperationEntry> {
    vec![
        OperationEntry::new(
            "app.propagation.peer_sync",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Run a propagation peer sync and return offer, transfer, retry, and queue state.",
        )
        .with_alias("peer_sync"),
        OperationEntry::new(
            "app.propagation.remote_status",
            "propagation",
            OperationKind::Query,
            TransportVariant::LegacyRpc,
            "Query remote propagation router status.",
        )
        .with_alias("propagation_remote_status"),
        OperationEntry::new(
            "app.propagation.remote_fetch",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Fetch propagation payloads from a remote router and preserve lifecycle state.",
        )
        .with_alias("propagation_remote_fetch"),
        OperationEntry::new(
            "app.propagation.remote_download",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Download propagation payloads from a remote router and preserve lifecycle state.",
        )
        .with_alias("propagation_remote_download"),
        OperationEntry::new(
            "app.propagation.remote_sync",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Run a remote propagation sync for one peer and preserve peer queue state.",
        )
        .with_alias("propagation_remote_sync"),
        OperationEntry::new(
            "app.propagation.remote_unpeer",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Unpeer through a remote propagation router and report local queue cleanup.",
        )
        .with_alias("propagation_remote_unpeer"),
        OperationEntry::new(
            "app.propagation.acknowledge_sync_completion",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Acknowledge a propagation sync completion or failure and expose the resulting recovery state.",
        )
        .with_alias("propagation_acknowledge_sync_completion"),
        OperationEntry::new(
            "app.propagation.node.get",
            "propagation",
            OperationKind::Query,
            TransportVariant::LegacyRpc,
            "Return the selected outbound propagation router node.",
        )
        .with_alias("get_outbound_propagation_node"),
        OperationEntry::new(
            "app.propagation.node.set",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Select or clear the outbound propagation router node.",
        )
        .with_alias("set_outbound_propagation_node"),
        OperationEntry::new(
            "app.propagation.node.list",
            "propagation",
            OperationKind::Query,
            TransportVariant::LegacyRpc,
            "List known outbound propagation router nodes and selection state.",
        )
        .with_alias("list_propagation_nodes"),
        OperationEntry::new(
            "app.propagation.status",
            "propagation",
            OperationKind::Query,
            TransportVariant::LegacyRpc,
            "Return local propagation configuration, counters, sync state, and selected router state.",
        )
        .with_alias("propagation_status"),
        OperationEntry::new(
            "app.propagation.enable",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Enable or update local propagation configuration and return the resulting state.",
        )
        .with_alias("propagation_enable"),
        OperationEntry::new(
            "app.propagation.delivery_policy.get",
            "propagation",
            OperationKind::Query,
            TransportVariant::LegacyRpc,
            "Return the local propagation delivery policy.",
        )
        .with_alias("get_delivery_policy"),
        OperationEntry::new(
            "app.propagation.delivery_policy.set",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Update the local propagation delivery policy and return the resulting policy.",
        )
        .with_alias("set_delivery_policy"),
        OperationEntry::new(
            "app.propagation.peer_maintenance",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Cull, rotate, and sync propagation peers while reporting cleanup and retry state.",
        )
        .with_alias("propagation_peer_maintenance"),
        OperationEntry::new(
            "app.propagation.ingest",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Ingest a local propagation payload into durable propagation storage and return queue accounting.",
        )
        .with_alias("propagation_ingest"),
        OperationEntry::new(
            "app.propagation.fetch",
            "propagation",
            OperationKind::Command,
            TransportVariant::LegacyRpc,
            "Fetch a local propagation payload from memory or durable propagation storage.",
        )
        .with_alias("propagation_fetch"),
    ]
}
