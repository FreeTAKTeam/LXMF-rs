use rns_transport::hash::AddressHash;
use rns_transport::transport::Transport;
use std::sync::Arc;

pub(super) fn spawn_stream_reconnect_tunnel_synthesizer(
    transport: Arc<Transport>,
    mut reconnect_rx: tokio::sync::mpsc::Receiver<AddressHash>,
) {
    tokio::spawn(async move {
        while let Some(iface) = reconnect_rx.recv().await {
            if transport.synthesize_tunnel_on_interface(iface).await {
                log::info!("[daemon] stream reconnect synthesized tunnel iface={}", iface);
            } else {
                log::warn!("[daemon] stream reconnect could not synthesize tunnel iface={}", iface);
            }
        }
    });
}
