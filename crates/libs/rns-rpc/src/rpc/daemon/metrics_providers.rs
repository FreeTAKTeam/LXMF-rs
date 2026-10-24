use super::*;

/// A metrics source owned outside `rns-rpc` (the daemon binary's announce
/// persistence worker, transport tables) that the daemon metrics snapshot
/// includes under `providers.<name>`. Snapshots must be cheap and non-blocking:
/// they run inline in `/metrics`; report `{"available": false, ...}` instead of
/// waiting when state is busy.
pub trait MetricsProvider: Send + Sync {
    fn snapshot(&self) -> JsonValue;
}

impl RpcDaemon {
    /// Register or replace a named provider. Names are stable snake_case keys
    /// with bounded cardinality; they are not derived from runtime data.
    pub fn register_metrics_provider(
        &self,
        name: &'static str,
        provider: Arc<dyn MetricsProvider>,
    ) {
        let mut providers =
            self.metrics_providers.lock().expect("metrics_providers mutex poisoned");
        if let Some(slot) = providers.iter_mut().find(|(existing, _)| *existing == name) {
            slot.1 = provider;
        } else {
            providers.push((name, provider));
        }
    }

    pub(super) fn metrics_provider_snapshots(&self) -> JsonValue {
        let providers: Vec<(&'static str, Arc<dyn MetricsProvider>)> =
            self.metrics_providers.lock().expect("metrics_providers mutex poisoned").clone();
        let mut map = JsonMap::new();
        for (name, provider) in providers {
            map.insert(name.to_string(), provider.snapshot());
        }
        JsonValue::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(u64);

    impl MetricsProvider for Fixed {
        fn snapshot(&self) -> JsonValue {
            json!({"value": self.0})
        }
    }

    #[test]
    fn registered_providers_appear_in_metrics_snapshot_and_replace_by_name() {
        let daemon = RpcDaemon::test_instance();
        daemon.register_metrics_provider("fixed", Arc::new(Fixed(1)));
        assert_eq!(daemon.metrics_snapshot()["providers"]["fixed"]["value"], 1);
        daemon.register_metrics_provider("fixed", Arc::new(Fixed(2)));
        let snapshot = daemon.metrics_snapshot();
        assert_eq!(snapshot["providers"]["fixed"]["value"], 2);
        assert_eq!(snapshot["providers"].as_object().map(|m| m.len()), Some(1));
    }
}
