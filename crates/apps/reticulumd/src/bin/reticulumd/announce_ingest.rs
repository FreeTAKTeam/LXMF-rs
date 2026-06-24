use super::bridge::PeerCrypto;
use reticulum_daemon::announce_names::{
    delivery_stamp_cost_from_app_data, lxmf_aspect_from_name_hash, parse_peer_name_from_app_data,
    pn_peering_cost_from_app_data, pn_stamp_cost_flexibility_from_app_data,
    pn_stamp_cost_from_app_data, AnnounceNamesDecodeError,
};
use rns_rpc::RpcDaemon;
use rns_transport::time::now_epoch_secs_i64;
use rns_transport::transport::AnnounceEvent;
use std::collections::HashMap;
use std::sync::Mutex;

pub(super) async fn ingest_announce_event(
    daemon: &RpcDaemon,
    event: AnnounceEvent,
    peer_crypto: &Mutex<HashMap<String, PeerCrypto>>,
) {
    let dest = event.destination.lock().await;
    let peer = hex::encode(dest.desc.address_hash.as_slice());
    let identity = dest.desc.identity;
    let (peer_name, peer_name_source) = if event.app_data.is_empty() {
        (None, None)
    } else {
        match parse_peer_name_from_app_data(event.app_data.as_slice()) {
            Ok(Some((name, source))) => (Some(name), Some(source.to_string())),
            Ok(None) => (None, None),
            Err(e) => {
                log::warn!("[daemon] rx announce peer={}: failed to parse peer name: {e}", peer);
                (None, None)
            }
        }
    };
    let _ratchet = event.ratchet;
    peer_crypto.lock().expect("peer map").insert(peer.clone(), PeerCrypto { identity });
    if let Some(name) = peer_name.as_ref() {
        log::debug!("[daemon] rx announce peer={} name={}", peer, name);
    } else {
        log::debug!("[daemon] rx announce peer={}", peer);
    }
    let timestamp = now_epoch_secs_i64();
    let app_data = event.app_data.as_slice();
    let app_data_hex = (!app_data.is_empty()).then(|| hex::encode(app_data));
    let aspect = lxmf_aspect_from_name_hash(dest.desc.name.as_name_hash_slice());
    let hops = Some(u32::from(event.hops));
    let interface = Some(hex::encode(event.interface.as_slice()));
    let stamp_cost = match announce_stamp_cost(aspect.as_deref(), app_data) {
        Ok(cost) => cost,
        Err(e) => {
            log::debug!("[daemon] rx announce peer={}: failed to parse stamp cost: {e}", peer);
            None
        }
    };
    let pn_flexibility = match pn_stamp_cost_flexibility_from_app_data(app_data) {
        Ok(v) => Some(v),
        Err(e) => {
            log::debug!("[daemon] rx announce peer={}: no PN stamp cost flexibility: {e}", peer);
            None
        }
    };
    let pn_peering = match pn_peering_cost_from_app_data(app_data) {
        Ok(v) => Some(v),
        Err(e) => {
            log::debug!("[daemon] rx announce peer={}: no PN peering cost: {e}", peer);
            None
        }
    };
    let _ = daemon.accept_announce_with_metadata(
        peer.clone(),
        timestamp,
        peer_name,
        peer_name_source,
        app_data_hex,
        None,
        None,
        None,
        None,
        stamp_cost,
        Some(pn_flexibility),
        Some(pn_peering),
        aspect,
        hops,
        interface,
        None,
        None,
        None,
    );
    let _ = daemon.record_announce_identity(
        peer.as_str(),
        hex::encode(identity.public_key_bytes()).as_str(),
        hex::encode(identity.verifying_key_bytes()).as_str(),
        timestamp,
    );
}

fn announce_stamp_cost(
    aspect: Option<&str>,
    app_data: &[u8],
) -> Result<Option<u32>, AnnounceNamesDecodeError> {
    match aspect {
        Some("lxmf.delivery") => delivery_stamp_cost_from_app_data(app_data),
        _ => pn_stamp_cost_from_app_data(app_data).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reticulum_daemon::announce_names::{
        encode_delivery_announce_app_data, encode_propagation_node_app_data,
        PropagationNodeAnnounceConfig,
    };

    #[test]
    fn announce_stamp_cost_uses_delivery_shape_for_delivery_aspect() {
        let app_data =
            encode_delivery_announce_app_data("peer", Some(19)).expect("delivery app data");

        assert_eq!(
            announce_stamp_cost(Some("lxmf.delivery"), app_data.as_slice()).expect("ok"),
            Some(19)
        );
    }

    #[test]
    fn announce_stamp_cost_uses_propagation_shape_for_non_delivery_aspects() {
        let app_data = encode_propagation_node_app_data(
            Some("peer"),
            PropagationNodeAnnounceConfig {
                stamp_cost: 21,
                stamp_cost_flexibility: 5,
                peering_cost: 13,
                ..PropagationNodeAnnounceConfig::default()
            },
        )
        .expect("propagation app data");

        assert_eq!(
            announce_stamp_cost(Some("lxmf.propagation"), app_data.as_slice()).expect("ok"),
            Some(21)
        );
        assert_eq!(announce_stamp_cost(None, app_data.as_slice()).expect("ok"), Some(21));
    }
}
