use super::bridge::PeerCrypto;
use super::bridge_helpers::diagnostics_enabled;
use reticulum_daemon::announce_names::{
    lxmf_aspect_from_name_hash, parse_peer_name_from_app_data, pn_peering_cost_from_app_data,
    pn_stamp_cost_flexibility_from_app_data, pn_stamp_cost_from_app_data,
};
use rns_rpc::RpcDaemon;
use rns_transport::time::now_epoch_secs_i64;
use rns_transport::transport::Transport;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(super) fn spawn_announce_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
) {
    let daemon_announce = daemon;
    tokio::spawn(async move {
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
                if diagnostics_enabled() {
                    if let Some(name) = peer_name.as_ref() {
                        eprintln!("[daemon] rx announce peer={} name={}", peer, name);
                    } else {
                        eprintln!("[daemon] rx announce peer={}", peer);
                    }
                }
                let timestamp = now_epoch_secs_i64();
                let app_data_hex = (!event.app_data.as_slice().is_empty())
                    .then(|| hex::encode(event.app_data.as_slice()));
                let aspect = lxmf_aspect_from_name_hash(dest.desc.name.as_name_hash_slice());
                let hops = Some(u32::from(event.hops));
                let interface = Some(hex::encode(event.interface.as_slice()));
                let _ = daemon_announce.accept_announce_with_metadata(
                    peer,
                    timestamp,
                    peer_name,
                    peer_name_source,
                    app_data_hex,
                    None,
                    None,
                    None,
                    None,
                    pn_stamp_cost_from_app_data(event.app_data.as_slice()),
                    Some(pn_stamp_cost_flexibility_from_app_data(event.app_data.as_slice())),
                    Some(pn_peering_cost_from_app_data(event.app_data.as_slice())),
                    aspect,
                    hops,
                    interface,
                    None,
                    None,
                    None,
                );
            }
        }
    });
}
