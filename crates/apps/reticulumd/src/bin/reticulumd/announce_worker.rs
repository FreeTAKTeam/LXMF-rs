use super::bridge::PeerCrypto;
use super::bridge_helpers::diagnostics_enabled;
use reticulum_daemon::announce_names::{
    delivery_stamp_cost_from_app_data, lxmf_aspect_from_name_hash, parse_peer_name_from_app_data,
    pn_peering_cost_from_app_data, pn_stamp_cost_flexibility_from_app_data,
    pn_stamp_cost_from_app_data,
};
use rns_rpc::RpcDaemon;
use rns_transport::time::now_epoch_secs_i64;
use rns_transport::transport::Transport;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};

const RETICULUM_PATH_TABLE_SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

pub(super) fn spawn_announce_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    reticulum_storage_path: Option<PathBuf>,
) {
    let daemon_announce = daemon;
    let persist_tx = reticulum_storage_path.as_ref().map(|path| {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
        let transport = transport.clone();
        let path = path.clone();
        tokio::spawn(async move {
            while rx.recv().await.is_some() {
                sleep(RETICULUM_PATH_TABLE_SAVE_DEBOUNCE).await;
                while rx.try_recv().is_ok() {}
                if let Err(err) = transport.save_reticulum_path_table(&path).await {
                    if diagnostics_enabled() {
                        eprintln!("[daemon] failed to persist Reticulum path table: {err}");
                    }
                }
            }
        });
        tx
    });
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
                let stamp_cost = match aspect.as_deref() {
                    Some("lxmf.delivery") => {
                        delivery_stamp_cost_from_app_data(event.app_data.as_slice())
                    }
                    _ => pn_stamp_cost_from_app_data(event.app_data.as_slice()),
                };
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
                    stamp_cost,
                    Some(pn_stamp_cost_flexibility_from_app_data(event.app_data.as_slice())),
                    Some(pn_peering_cost_from_app_data(event.app_data.as_slice())),
                    aspect,
                    hops,
                    interface,
                    None,
                    None,
                    None,
                );
                if let Some(tx) = persist_tx.as_ref() {
                    let _ = tx.try_send(());
                }
            }
        }
    });
}
