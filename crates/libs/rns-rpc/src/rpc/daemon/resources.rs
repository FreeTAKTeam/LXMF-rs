use super::*;

fn string_inventory_bytes(ids: &Vec<String>) -> usize {
    ids.capacity() * std::mem::size_of::<String>() + ids.iter().map(String::capacity).sum::<usize>()
}

// This counts owned buffers and inline element slots, not allocator metadata or
// BTreeMap node overhead. Traversal borrows every value: diagnostics must not
// clone/serialize the inventories whose allocation they are measuring.
fn json_heap_lower_bound(value: &JsonValue) -> usize {
    match value {
        JsonValue::String(value) => value.capacity(),
        JsonValue::Array(values) => {
            values.capacity() * std::mem::size_of::<JsonValue>()
                + values.iter().map(json_heap_lower_bound).sum::<usize>()
        }
        JsonValue::Object(values) => values
            .iter()
            .map(|(key, value)| {
                std::mem::size_of::<(String, JsonValue)>()
                    + key.capacity()
                    + json_heap_lower_bound(value)
            })
            .sum(),
        _ => 0,
    }
}

impl RpcDaemon {
    pub fn zmq_pipeline_metrics(&self) -> Arc<super::super::zmq_metrics::ZmqPipelineMetrics> {
        Arc::clone(&self.zmq_pipeline_metrics)
    }

    /// Explicit diagnostics only. Approximate owned buffers are separate from
    /// process RSS/swap; broadcast subscribers and SQLite caches are not included.
    pub fn resource_usage_snapshot(&self) -> Result<JsonValue, std::io::Error> {
        let (peers, handled, unhandled, inventory_bytes) = {
            let peers = self
                .peers
                .lock()
                .map_err(|error| std::io::Error::other(format!("resource peers lock: {error}")))?;
            let mut handled = 0;
            let mut unhandled = 0;
            let mut bytes = 0;
            for peer in peers.values() {
                handled += peer.restored_handled_ids.len();
                unhandled += peer.restored_unhandled_ids.len();
                bytes += string_inventory_bytes(&peer.restored_handled_ids)
                    + string_inventory_bytes(&peer.restored_unhandled_ids);
            }
            (peers.len(), handled, unhandled, bytes)
        };
        let (legacy_events, legacy_bytes) = {
            let events = self.event_queue.lock().map_err(|error| {
                std::io::Error::other(format!("resource legacy events lock: {error}"))
            })?;
            (
                events.len(),
                events
                    .iter()
                    .map(|event| {
                        event.event_type.capacity() + json_heap_lower_bound(&event.payload)
                    })
                    .sum::<usize>(),
            )
        };
        let (sdk_events, sdk_bytes) = {
            let events = self.sdk_event_log.lock().map_err(|error| {
                std::io::Error::other(format!("resource SDK events lock: {error}"))
            })?;
            (
                events.len(),
                events
                    .iter()
                    .map(|event| {
                        event.event.event_type.capacity()
                            + json_heap_lower_bound(&event.event.payload)
                    })
                    .sum::<usize>(),
            )
        };
        Ok(json!({
            "peer_records": peers,
            "zmq_pipeline": self.zmq_pipeline_metrics.snapshot(),
            "peer_inventory": {"handled_ids": handled, "unhandled_ids": unhandled, "owned_buffer_bytes": inventory_bytes},
            "legacy_events": {"items": legacy_events, "heap_bytes_lower_bound": legacy_bytes},
            "sdk_events": {"items": sdk_events, "heap_bytes_lower_bound": sdk_bytes},
            "accounting": "Owned buffers only; excludes allocator metadata, map nodes, broadcast copies, SQLite and transport memory."
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_counts_capacity_without_materializing_another_copy() {
        let mut ids = Vec::with_capacity(8);
        ids.push(String::with_capacity(128));
        ids[0].push_str("identifier");
        assert_eq!(string_inventory_bytes(&ids), 8 * std::mem::size_of::<String>() + 128);
        assert_eq!(ids[0], "identifier");
    }

    #[test]
    fn empty_daemon_resources_are_explicit_and_nonmutating() {
        let daemon = RpcDaemon::test_instance();
        let before = daemon.store.contention_snapshot().write_ops_total;
        let value = daemon.resource_usage_snapshot().expect("snapshot");
        assert_eq!(value["peer_inventory"]["owned_buffer_bytes"], 0);
        assert_eq!(value["legacy_events"]["items"], 0);
        assert_eq!(value["sdk_events"]["items"], 0);
        assert_eq!(daemon.store.contention_snapshot().write_ops_total, before);
    }
}
