use rns_rpc::PathLookupBridge;
use rns_transport::destination_hash::parse_destination_hash_required;
use rns_transport::hash::AddressHash;
use rns_transport::transport::Transport;
use std::sync::Arc;

pub(crate) struct DaemonPathLookupBridge {
    transport: Arc<Transport>,
}

impl DaemonPathLookupBridge {
    pub(crate) fn new(transport: Arc<Transport>) -> Self {
        Self { transport }
    }

    fn destination_hash(destination: &str) -> Result<AddressHash, std::io::Error> {
        parse_destination_hash_required(destination)
            .map(AddressHash::new)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))
    }

    fn run_transport<F, T>(&self, f: F) -> Result<T, std::io::Error>
    where
        F: FnOnce(Arc<Transport>) -> Result<T, std::io::Error> + Send + 'static,
        T: Send + 'static,
    {
        let transport = self.transport.clone();
        std::thread::spawn(move || f(transport))
            .join()
            .map_err(|_| std::io::Error::other("path lookup helper thread panicked"))?
    }
}

impl PathLookupBridge for DaemonPathLookupBridge {
    fn has_path(&self, destination: &str) -> Result<bool, std::io::Error> {
        let destination = Self::destination_hash(destination)?;
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build path lookup runtime: {err}"))
                })?;
            Ok(runtime.block_on(async move { transport.has_path(&destination).await }))
        })
    }

    fn request_path(&self, destination: &str) -> Result<(), std::io::Error> {
        let destination = Self::destination_hash(destination)?;
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build path request runtime: {err}"))
                })?;
            runtime.block_on(async move {
                transport.request_path(&destination, None, None).await;
            });
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::transport::TransportConfig;

    fn bridge() -> DaemonPathLookupBridge {
        let identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
        let config = TransportConfig::new("path-lookup-bridge-test", &identity, true);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("transport construction runtime");
        let _guard = runtime.enter();
        DaemonPathLookupBridge::new(Arc::new(Transport::new(config)))
    }

    #[test]
    fn path_lookup_bridge_reports_missing_path_without_rpc_transport_leak() {
        let bridge = bridge();
        let known = bridge.has_path("00112233445566778899aabbccddeeff").expect("query path");

        assert!(!known);
    }

    #[test]
    fn path_lookup_bridge_dispatches_request_path() {
        let bridge = bridge();

        bridge.request_path("00112233445566778899aabbccddeeff").expect("dispatch path request");
    }
}
