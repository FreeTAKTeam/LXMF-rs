use super::*;

impl DaemonPathLookupBridge {
    pub(super) fn path_table_impl(
        &self,
        max_hops: Option<u64>,
    ) -> Result<JsonValue, std::io::Error> {
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build path table runtime: {err}"))
                })?;
            let entries = runtime.block_on(async move { transport.path_table(max_hops).await })?;
            Ok(JsonValue::Array(entries.into_iter().map(Self::path_table_entry_json).collect()))
        })
    }

    pub(super) fn path_table_entry_json(
        entry: rns_transport::transport::TransportPathTableEntry,
    ) -> JsonValue {
        json!({
            "hash": entry.destination.to_hex_string(),
            "timestamp": entry.timestamp_secs,
            "via": entry.next_hop.to_hex_string(),
            "hops": entry.hops,
            "expires": entry.expires_secs,
            "interface": entry.interface_name,
            "interface_hash": hex::encode(entry.interface_hash.as_slice()),
        })
    }
}
