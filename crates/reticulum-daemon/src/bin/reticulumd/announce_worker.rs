use super::bridge::PeerCrypto;
use reticulum::rpc::RpcDaemon;
use reticulum::time::now_epoch_secs_i64;
use reticulum::transport::Transport;
use reticulum_daemon::announce_names::parse_peer_name_from_app_data;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(super) fn spawn_announce_worker(
    daemon: Rc<RpcDaemon>,
    transport: Arc<Transport>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
) {
    let daemon_announce = daemon;
    tokio::task::spawn_local(async move {
        let mut rx = transport.recv_announces().await;
        loop {
            if let Ok(event) = rx.recv().await {
                let dest = event.destination.lock().await;
                let peer = hex::encode(dest.desc.address_hash.as_slice());
                let identity = dest.desc.identity;
                let (peer_name, peer_name_source) =
                    parse_peer_name_from_app_data(event.app_data.as_slice())
                        .map(|(name, source)| (Some(name), Some(source.to_string())))
                        .unwrap_or((None, None));
                let _ratchet = event.ratchet;
                peer_crypto.lock().expect("peer map").insert(peer.clone(), PeerCrypto { identity });
                if let Some(name) = peer_name.as_ref() {
                    eprintln!("[daemon] rx announce peer={} name={}", peer, name);
                } else {
                    eprintln!("[daemon] rx announce peer={}", peer);
                }
                let timestamp = now_epoch_secs_i64();
                let _ = daemon_announce.accept_announce_with_details(
                    peer,
                    timestamp,
                    peer_name,
                    peer_name_source,
                );
            }
        }
    });
}
