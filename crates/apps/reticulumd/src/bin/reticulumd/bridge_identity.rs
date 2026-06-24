use super::PeerCrypto;
use rns_rpc::RpcDaemon;
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
) -> Result<Option<Identity>, &'static str> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "failed to build tokio runtime")?;
        Ok(runtime.block_on(async move {
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
        }))
    })
    .join()
    .unwrap_or(Err("identity resolver thread panicked"))
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
    const DESTINATION_NAMES: [(&str, &str); 4] = [
        ("lxmf", "delivery"),
        ("lxmf", "propagation"),
        ("lxmf", "propagation.control"),
        ("r3akt", "emergency"),
    ];
    candidates.into_iter().find(|identity| {
        DESTINATION_NAMES.iter().any(|(app, aspect)| {
            SingleOutputDestination::new(*identity, DestinationName::new(app, aspect))
                .desc
                .address_hash
                == destination_hash
        })
    })
}

pub(super) fn persisted_identity_for_destination(
    daemon: &RpcDaemon,
    destination_hash: AddressHash,
) -> Option<Identity> {
    let destination_hex = hex::encode(destination_hash.as_slice());
    let (public_key_hex, verifying_key_hex) =
        daemon.announce_identity_keys(destination_hex.as_str()).ok().flatten()?;
    let identity = identity_from_key_hex(public_key_hex.as_str(), verifying_key_hex.as_str())?;
    identity_matches_destination(identity, destination_hash).then_some(identity)
}

fn identity_from_key_hex(public_key_hex: &str, verifying_key_hex: &str) -> Option<Identity> {
    let public_key = hex::decode(public_key_hex).ok()?;
    let verifying_key = hex::decode(verifying_key_hex).ok()?;
    if public_key.len() != rns_transport::identity::PUBLIC_KEY_LENGTH
        || verifying_key.len() != rns_transport::identity::PUBLIC_KEY_LENGTH
    {
        return None;
    }
    Some(Identity::new_from_slices(public_key.as_slice(), verifying_key.as_slice()))
}

fn identity_matches_destination(identity: Identity, destination_hash: AddressHash) -> bool {
    const DESTINATION_NAMES: [(&str, &str); 4] = [
        ("lxmf", "delivery"),
        ("lxmf", "propagation"),
        ("lxmf", "propagation.control"),
        ("r3akt", "emergency"),
    ];
    DESTINATION_NAMES.iter().any(|(app, aspect)| {
        SingleOutputDestination::new(identity, DestinationName::new(app, aspect)).desc.address_hash
            == destination_hash
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

#[cfg(test)]
mod tests {
    use super::*;
    use rns_rpc::{MessagesStore, RpcDaemon};

    #[test]
    fn persisted_identity_matches_lxmf_delivery_destination_after_restart() {
        let store = MessagesStore::in_memory().expect("store");
        let daemon = RpcDaemon::with_store(store, "test-node".to_string());
        let remote = rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let delivery_hash = SingleOutputDestination::new(
            *remote.as_identity(),
            DestinationName::new("lxmf", "delivery"),
        )
        .desc
        .address_hash;
        let delivery_hex = hex::encode(delivery_hash.as_slice());
        daemon
            .record_announce_identity(
                delivery_hex.as_str(),
                hex::encode(remote.as_identity().public_key_bytes()).as_str(),
                hex::encode(remote.as_identity().verifying_key_bytes()).as_str(),
                1_781_964_554,
            )
            .expect("record announce identity");

        let restored =
            persisted_identity_for_destination(&daemon, delivery_hash).expect("restored identity");

        assert_eq!(restored.public_key_bytes(), remote.as_identity().public_key_bytes());
        assert_eq!(restored.verifying_key_bytes(), remote.as_identity().verifying_key_bytes());
    }

    #[test]
    fn persisted_identity_rejects_mismatched_destination_hash() {
        let store = MessagesStore::in_memory().expect("store");
        let daemon = RpcDaemon::with_store(store, "test-node".to_string());
        let remote = rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let wrong_hash = AddressHash::new([0x42; 16]);
        daemon
            .record_announce_identity(
                hex::encode(wrong_hash.as_slice()).as_str(),
                hex::encode(remote.as_identity().public_key_bytes()).as_str(),
                hex::encode(remote.as_identity().verifying_key_bytes()).as_str(),
                1_781_964_554,
            )
            .expect("record announce identity");

        assert!(persisted_identity_for_destination(&daemon, wrong_hash).is_none());
    }
}
