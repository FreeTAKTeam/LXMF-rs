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

/// Peer identities keyed by announced destination hex, plus an index from every
/// destination hash an identity owns under [`DESTINATION_NAMES`] so lookups are O(1).
#[derive(Default)]
pub(in super::super) struct PeerCryptoCache {
    peers: HashMap<String, PeerCrypto>,
    // One copy per identity, keyed by identity hash and refcounted: several
    // announced destinations (aspects) usually share one identity.
    identities: HashMap<AddressHash, (Identity, usize)>,
    // Derived destination hash -> identity hash; written only when an identity
    // first appears, so repeat announces do no hashing under the lock.
    by_destination: HashMap<AddressHash, AddressHash>,
}

impl PeerCryptoCache {
    pub(in super::super) fn insert(&mut self, destination_hex: String, peer: PeerCrypto) {
        let previous = self.peers.insert(destination_hex, peer);
        if let Some(previous) = previous {
            if previous.identity.address_hash == peer.identity.address_hash {
                return;
            }
            self.release(previous.identity);
        }
        let entry = self.identities.entry(peer.identity.address_hash).or_insert((peer.identity, 0));
        entry.1 += 1;
        if entry.1 == 1 {
            for hash in derived_destination_hashes(peer.identity) {
                self.by_destination.insert(hash, peer.identity.address_hash);
            }
        }
    }

    pub(in super::super) fn get(&self, destination_hex: &str) -> Option<&PeerCrypto> {
        self.peers.get(destination_hex)
    }

    pub(in super::super) fn len(&self) -> usize {
        self.peers.len()
    }

    pub(in super::super) fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        let mut removed = Vec::new();
        self.peers.retain(|destination_hex, peer| {
            let kept = keep(destination_hex);
            if !kept {
                removed.push(peer.identity);
            }
            kept
        });
        removed.into_iter().for_each(|identity| self.release(identity));
    }

    pub(in super::super) fn identity_for_destination(
        &self,
        destination_hash: &AddressHash,
    ) -> Option<Identity> {
        let identity_hash = self.by_destination.get(destination_hash)?;
        self.identities.get(identity_hash).map(|(identity, _)| *identity)
    }

    fn release(&mut self, identity: Identity) {
        let Some(entry) = self.identities.get_mut(&identity.address_hash) else {
            return;
        };
        entry.1 -= 1;
        if entry.1 == 0 {
            self.identities.remove(&identity.address_hash);
            for hash in derived_destination_hashes(identity) {
                self.by_destination.remove(&hash);
            }
        }
    }
}

pub(super) fn cached_identity_for_destination(
    destination_hash: AddressHash,
    peer_identity: Option<Identity>,
    propagation_node_identity: Option<Identity>,
    peer_crypto: &Mutex<PeerCryptoCache>,
    outbound_propagation_identities: &Mutex<HashMap<String, Identity>>,
) -> Option<Identity> {
    let matches = |identity: &Identity| identity_matches_destination(*identity, destination_hash);
    if let Some(identity) = peer_identity.into_iter().chain(propagation_node_identity).find(matches)
    {
        return Some(identity);
    }
    let cached = match peer_crypto.lock() {
        Ok(peers) => peers.identity_for_destination(&destination_hash),
        Err(error) => {
            log::error!("[daemon] peer identity cache lock poisoned: {error}");
            None
        }
    };
    if cached.is_some() {
        return cached;
    }
    // Only propagation nodes this daemon used; small, so copy out and hash outside the lock.
    let propagation_identities: Vec<Identity> = match outbound_propagation_identities.lock() {
        Ok(identities) => identities.values().copied().collect(),
        Err(error) => {
            log::error!("[daemon] propagation identity cache lock poisoned: {error}");
            Vec::new()
        }
    };
    propagation_identities.into_iter().find(matches)
}

pub(super) fn persisted_identity_for_destination(
    daemon: &RpcDaemon,
    destination_hash: AddressHash,
) -> Option<Identity> {
    let destination_hex = hex::encode(destination_hash.as_slice());
    let (public_key_hex, verifying_key_hex) = match daemon
        .announce_identity_keys(destination_hex.as_str())
    {
        Ok(keys) => keys?,
        Err(error) => {
            log::error!(
                "[daemon] failed to read persisted announce identity destination={destination_hex}: {error}"
            );
            return None;
        }
    };
    let identity = match identity_from_key_hex(public_key_hex.as_str(), verifying_key_hex.as_str())
    {
        Ok(identity) => identity,
        Err(error) => {
            log::error!(
                "[daemon] persisted announce identity is malformed destination={destination_hex}: {error}"
            );
            return None;
        }
    };
    identity_matches_destination(identity, destination_hash).then_some(identity)
}

fn identity_from_key_hex(
    public_key_hex: &str,
    verifying_key_hex: &str,
) -> Result<Identity, &'static str> {
    let public_key = hex::decode(public_key_hex).map_err(|_| "public key is not valid hex")?;
    let verifying_key =
        hex::decode(verifying_key_hex).map_err(|_| "verifying key is not valid hex")?;
    Identity::try_new_from_slices(public_key.as_slice(), verifying_key.as_slice())
        .map_err(|_| "public or verifying key is invalid")
}

const DESTINATION_NAMES: [(&str, &str); 4] = [
    ("lxmf", "delivery"),
    ("lxmf", "propagation"),
    ("lxmf", "propagation.control"),
    ("r3akt", "emergency"),
];

fn derived_destination_hashes(identity: Identity) -> impl Iterator<Item = AddressHash> {
    DESTINATION_NAMES.into_iter().map(move |(app, aspect)| {
        SingleOutputDestination::new(identity, DestinationName::new(app, aspect)).desc.address_hash
    })
}

fn identity_matches_destination(identity: Identity, destination_hash: AddressHash) -> bool {
    derived_destination_hashes(identity).any(|hash| hash == destination_hash)
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

    fn random_identity() -> Identity {
        *rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng).as_identity()
    }

    fn hash_for(identity: Identity, app: &str, aspect: &str) -> AddressHash {
        SingleOutputDestination::new(identity, DestinationName::new(app, aspect)).desc.address_hash
    }

    fn lookup(
        peers: &Mutex<PeerCryptoCache>,
        propagation: &Mutex<HashMap<String, Identity>>,
        destination_hash: AddressHash,
    ) -> Option<[u8; 32]> {
        cached_identity_for_destination(destination_hash, None, None, peers, propagation)
            .map(|identity| *identity.verifying_key_bytes())
    }

    #[test]
    fn cached_lookup_resolves_every_aspect_and_propagation_only_identities() {
        let peers = Mutex::new(PeerCryptoCache::default());
        let propagation = Mutex::new(HashMap::new());
        let announced = random_identity();
        let delivery = hash_for(announced, "lxmf", "delivery");
        peers
            .lock()
            .expect("peers")
            .insert(hex::encode(delivery.as_slice()), PeerCrypto { identity: announced });
        let propagation_only = random_identity();
        propagation.lock().expect("propagation").insert("pn".to_string(), propagation_only);

        for (app, aspect) in DESTINATION_NAMES {
            assert_eq!(
                lookup(&peers, &propagation, hash_for(announced, app, aspect)),
                Some(*announced.verifying_key_bytes())
            );
        }
        assert!(lookup(&peers, &propagation, hash_for(propagation_only, "lxmf", "propagation"))
            .is_some());
        assert!(lookup(&peers, &propagation, hash_for(announced, "nomadnetwork", "node")).is_none());
    }

    #[test]
    fn retain_keeps_index_while_another_destination_shares_the_identity() {
        let mut cache = PeerCryptoCache::default();
        let shared = random_identity();
        let delivery = hash_for(shared, "lxmf", "delivery");
        let propagation = hash_for(shared, "lxmf", "propagation");
        cache.insert(hex::encode(delivery.as_slice()), PeerCrypto { identity: shared });
        cache.insert(hex::encode(propagation.as_slice()), PeerCrypto { identity: shared });

        let delivery_hex = hex::encode(delivery.as_slice());
        cache.retain(|hex| hex != delivery_hex);
        assert_eq!(cache.len(), 1);
        assert!(cache.identity_for_destination(&delivery).is_some());

        cache.retain(|_| false);
        assert_eq!(cache.len(), 0);
        assert!(cache.identity_for_destination(&delivery).is_none());
        assert!(cache.by_destination.is_empty() && cache.identities.is_empty());
    }

    #[test]
    fn reinserting_a_destination_with_a_new_identity_drops_the_old_index() {
        let mut cache = PeerCryptoCache::default();
        let (old, new) = (random_identity(), random_identity());
        cache.insert("aa".to_string(), PeerCrypto { identity: old });
        cache.insert("aa".to_string(), PeerCrypto { identity: new });
        assert!(cache.identity_for_destination(&hash_for(old, "lxmf", "delivery")).is_none());
        assert!(cache.identity_for_destination(&hash_for(new, "lxmf", "delivery")).is_some());
        assert_eq!(cache.by_destination.len(), DESTINATION_NAMES.len());
        assert_eq!(cache.identities.len(), 1);
        // A repeat announce of the same identity must not double-count it.
        cache.insert("aa".to_string(), PeerCrypto { identity: new });
        cache.retain(|_| false);
        assert!(cache.by_destination.is_empty() && cache.identities.is_empty());
    }

    #[test]
    fn cached_lookup_stays_fast_with_ten_thousand_peers() {
        let peers = Mutex::new(PeerCryptoCache::default());
        let propagation = Mutex::new(HashMap::new());
        let identities: Vec<Identity> = (0..10_000).map(|_| random_identity()).collect();
        for (index, identity) in identities.iter().enumerate() {
            peers
                .lock()
                .expect("peers")
                .insert(format!("{index:032x}"), PeerCrypto { identity: *identity });
        }
        let targets: Vec<AddressHash> =
            identities.iter().step_by(10).map(|id| hash_for(*id, "r3akt", "emergency")).collect();
        let started = std::time::Instant::now();
        assert!(targets.iter().all(|hash| lookup(&peers, &propagation, *hash).is_some()));
        // The old scan did ~50M key comparisons per lookup at this size.
        assert!(started.elapsed() < Duration::from_secs(1), "took {:?}", started.elapsed());
    }

    #[test]
    fn persisted_identity_key_decoder_rejects_malformed_material() {
        assert!(identity_from_key_hex("not-hex", &"00".repeat(32)).is_err());
        assert!(identity_from_key_hex(&"00".repeat(31), &"00".repeat(32)).is_err());
        assert!(identity_from_key_hex(&"00".repeat(32), &"00".repeat(31)).is_err());
    }
}
