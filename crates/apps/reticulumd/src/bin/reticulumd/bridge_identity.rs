use super::PeerCrypto;
use rns_transport::destination::{DestinationName, SingleOutputDestination};
use rns_transport::hash::AddressHash;
use rns_transport::identity::Identity;
use rns_transport::transport::Transport;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
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

pub(super) fn cached_identity_for_destination(
    destination_hash: AddressHash,
    peer_identity: Option<Identity>,
    propagation_node_identity: Option<Identity>,
    peer_crypto: &Mutex<HashMap<String, PeerCrypto>>,
    outbound_propagation_identities: &Mutex<HashMap<String, Identity>>,
) -> Option<Identity> {
    let mut candidates = Vec::new();
    push_unique_identity(&mut candidates, peer_identity);
    push_unique_identity(&mut candidates, propagation_node_identity);
    if let Ok(peers) = peer_crypto.lock() {
        peers.values().for_each(|info| push_unique_identity(&mut candidates, Some(info.identity)));
    }
    if let Ok(identities) = outbound_propagation_identities.lock() {
        identities
            .values()
            .for_each(|identity| push_unique_identity(&mut candidates, Some(*identity)));
    }
    candidates.into_iter().find(|identity| {
        ["delivery", "propagation", "propagation.control"].iter().any(|aspect| {
            SingleOutputDestination::new(*identity, DestinationName::new("lxmf", aspect))
                .desc
                .address_hash
                == destination_hash
        })
    })
}

fn push_unique_identity(candidates: &mut Vec<Identity>, candidate: Option<Identity>) {
    let Some(candidate) = candidate else {
        return;
    };
    let already_present = candidates.iter().any(|existing| {
        existing.public_key_bytes() == candidate.public_key_bytes()
            && existing.verifying_key_bytes() == candidate.verifying_key_bytes()
    });
    if !already_present {
        candidates.push(candidate);
    }
}
