use super::announce_ingest::ingest_announce_event;
use super::announce_persistence::spawn_path_table_persistence_worker;
use super::bridge::PeerCrypto;
use rns_rpc::RpcDaemon;
use rns_transport::transport::Transport;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub(super) fn spawn_announce_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    reticulum_storage_path: Option<PathBuf>,
) {
    let daemon_announce = daemon;
    let persist_tx = reticulum_storage_path
        .as_ref()
        .map(|path| spawn_path_table_persistence_worker(transport.clone(), path.clone()));
    tokio::spawn(async move {
        let mut rx = transport.recv_announces().await;
        loop {
            if let Ok(event) = rx.recv().await {
                ingest_announce_event(daemon_announce.as_ref(), event, peer_crypto.as_ref()).await;
                if let Some(tx) = persist_tx.as_ref() {
                    let _ = tx.try_send(());
                }
            }
        }
    });
}
