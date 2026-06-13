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
    ]
}
