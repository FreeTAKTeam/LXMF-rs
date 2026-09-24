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
            let entries = runtime.block_on(transport.path_table(max_hops))?;
            let iface_manager = transport.iface_manager();
            let rows = runtime.block_on(async move {
                let iface_manager = iface_manager.lock().await;
                entries
                    .into_iter()
                    .map(|entry| {
                        let reference_interface = iface_manager
                            .tcp_client_path_table_metadata(&entry.interface)
                            .map(crate::tcp_client_path_table::render);
                        Self::path_table_entry_json(entry, reference_interface)
                    })
                    .collect::<Vec<_>>()
            });
            Ok(JsonValue::Array(rows))
        })
    }

    pub(super) fn path_table_entry_json(
        entry: rns_transport::transport::TransportPathTableEntry,
        reference_interface: Option<String>,
    ) -> JsonValue {
        let interface = reference_interface.or(entry.interface_name);
        json!({
            "hash": entry.destination.to_hex_string(),
            "timestamp": entry.timestamp_secs,
            "via": entry.next_hop.to_hex_string(),
            "hops": entry.hops,
            "expires": entry.expires_secs,
            "interface": interface,
            "interface_hash": hex::encode(entry.interface_hash.as_slice()),
        })
    }
}
