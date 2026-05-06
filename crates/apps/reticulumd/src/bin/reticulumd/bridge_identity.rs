use rns_transport::hash::AddressHash;
use rns_transport::identity::Identity;
use rns_transport::transport::Transport;
use std::sync::Arc;
use std::time::Duration;

pub(super) fn resolve_destination_identity_blocking(
    transport: Arc<Transport>,
    destination_hash: AddressHash,
    timeout: Duration,
) -> Option<Identity> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
        runtime.block_on(async move {
            let mut identity = transport.destination_identity(&destination_hash).await;
            if identity.is_none() {
                transport.request_path(&destination_hash, None, None).await;
                let deadline = tokio::time::Instant::now() + timeout;
                while identity.is_none() && tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    identity = transport.destination_identity(&destination_hash).await;
                }
            }
            identity
        })
    })
    .join()
    .ok()
    .flatten()
}
