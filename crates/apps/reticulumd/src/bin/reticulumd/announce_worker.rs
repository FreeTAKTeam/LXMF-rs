use super::announce_ingest::ingest_announce_event;
use super::announce_persistence::{
    spawn_path_table_persistence_worker, PathTablePersistenceContext,
};
use super::bridge::PeerCrypto;
use rand_core::OsRng;
use rns_rpc::RpcDaemon;
use rns_transport::destination::DestinationName;
use rns_transport::discovery::announce::{
    decode_announce, encode_announce, DiscoveryAnnounceError,
};
use rns_transport::discovery::InterfaceDiscoveryStore;
use rns_transport::identity::PrivateIdentity;
use rns_transport::ratchets::{decrypt_with_identity, encrypt_for_public_key};
use rns_transport::time::now_epoch_secs_u64;
use rns_transport::transport::AnnounceEvent;
use rns_transport::transport::Transport;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) struct DiscoveryWorkerConfig {
    pub storage_path: PathBuf,
    pub allowed_network_ids: Vec<String>,
    pub required_value: u32,
    pub local_identity: PrivateIdentity,
    pub network_identity: Option<PrivateIdentity>,
    pub outbound: Vec<crate::discovery_publish::OutboundDiscoveryAnnouncement>,
}

pub(super) fn spawn_announce_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    reticulum_storage_path: Option<PathBuf>,
    discovery: Option<DiscoveryWorkerConfig>,
) {
    if let Some(config) = discovery.as_ref() {
        spawn_discovery_publish_worker(
            transport.clone(),
            config.local_identity.clone(),
            config.network_identity.clone(),
            config.outbound.clone(),
        );
    }
    let daemon_announce = daemon;
    let persist_tx = reticulum_storage_path.as_ref().map(|path| {
        spawn_path_table_persistence_worker(PathTablePersistenceContext::new(
            transport.clone(),
            path.clone(),
        ))
    });
    tokio::spawn(async move {
        let mut rx = transport.recv_announces().await;
        loop {
            if let Ok(event) = rx.recv().await {
                if let Some(config) = discovery.as_ref() {
                    ingest_discovery_announce(&event, config).await;
                }
                ingest_announce_event(daemon_announce.as_ref(), event, peer_crypto.as_ref()).await;
                if let Some(tx) = persist_tx.as_ref() {
                    if let Err(err) = tx.try_send(()) {
                        log::warn!("[daemon] dropped path-table persistence trigger: {err}");
                    }
                }
            }
        }
    });
}

fn spawn_discovery_publish_worker(
    transport: Arc<Transport>,
    local_identity: PrivateIdentity,
    network_identity: Option<PrivateIdentity>,
    outbound: Vec<crate::discovery_publish::OutboundDiscoveryAnnouncement>,
) {
    if outbound.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let destination_identity =
            discovery_announce_identity(&local_identity, network_identity.as_ref());
        let destination = transport
            .add_destination(
                destination_identity,
                DestinationName::new("rnstransport", "discovery.interface"),
            )
            .await;
        let mut next_due = vec![tokio::time::Instant::now(); outbound.len()];
        loop {
            let (index, due) = next_due
                .iter()
                .enumerate()
                .min_by_key(|(_, due)| **due)
                .map(|(index, due)| (index, *due))
                .expect("non-empty discovery publisher");
            tokio::time::sleep_until(due).await;
            let candidate = &outbound[index];
            let encoded = encode_discovery_candidate(candidate, network_identity.as_ref());
            match encoded {
                Ok(payload) => transport.send_announce(&destination, Some(&payload)).await,
                Err(error) => log::warn!(
                    "failed to encode discovery announce for {}: {error}",
                    candidate.interface.name
                ),
            }
            next_due[index] = tokio::time::Instant::now()
                + std::time::Duration::from_secs(candidate.interval_secs);
        }
    });
}

fn encode_discovery_candidate(
    candidate: &crate::discovery_publish::OutboundDiscoveryAnnouncement,
    network_identity: Option<&PrivateIdentity>,
) -> Result<Vec<u8>, DiscoveryAnnounceError> {
    if candidate.encrypted {
        let identity =
            network_identity.cloned().ok_or(DiscoveryAnnounceError::MissingNetworkIdentity)?;
        encode_announce(
            &candidate.interface,
            candidate.stamp_value,
            Some(move |body: &[u8]| {
                encrypt_for_public_key(
                    &identity.as_identity().public_key,
                    identity.address_hash().as_slice(),
                    body,
                    OsRng,
                )
                .ok()
            }),
        )
    } else {
        encode_announce(
            &candidate.interface,
            candidate.stamp_value,
            None::<fn(&[u8]) -> Option<Vec<u8>>>,
        )
    }
}

fn discovery_announce_identity(
    local_identity: &PrivateIdentity,
    network_identity: Option<&PrivateIdentity>,
) -> PrivateIdentity {
    network_identity.cloned().unwrap_or_else(|| local_identity.clone())
}

async fn ingest_discovery_announce(event: &AnnounceEvent, config: &DiscoveryWorkerConfig) {
    let discovery_name = DestinationName::new("rnstransport", "discovery.interface");
    let destination = event.destination.lock().await;
    if destination.desc.name.as_name_hash_slice() != discovery_name.as_name_hash_slice() {
        return;
    }
    let network_id = hex::encode(destination.desc.identity.address_hash.as_slice());
    drop(destination);
    let network_identity = config.network_identity.clone();
    let record = match decode_announce(
        event.app_data.as_slice(),
        &network_id,
        &config.allowed_network_ids,
        event.hops,
        now_epoch_secs_u64() as f64,
        config.required_value,
        network_identity.map(|identity| {
            move |ciphertext: &[u8]| {
                decrypt_with_identity(&identity, identity.address_hash().as_slice(), ciphertext)
                    .ok()
            }
        }),
    ) {
        Ok(record) => record,
        Err(error) => {
            log::debug!("[daemon] ignored interface discovery announce: {error}");
            return;
        }
    };
    if let Err(error) = InterfaceDiscoveryStore::new(&config.storage_path).observe(record) {
        log::warn!("[daemon] failed to persist discovered interface: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery_publish::OutboundDiscoveryAnnouncement;
    use rns_transport::discovery::announce::{decode_announce, DiscoverableInterface};

    #[test]
    fn rns_1_5_encrypted_discovery_cross_node_uses_shared_network_identity() {
        let publisher_identity = PrivateIdentity::new_from_name("rns-1.5-publisher");
        let receiver_identity = PrivateIdentity::new_from_name("rns-1.5-receiver");
        let network_identity = PrivateIdentity::new_from_name("rns-1.5-discovery-network");
        assert_ne!(publisher_identity.address_hash(), receiver_identity.address_hash());
        assert_eq!(
            discovery_announce_identity(&publisher_identity, Some(&network_identity))
                .address_hash(),
            discovery_announce_identity(&receiver_identity, Some(&network_identity)).address_hash()
        );
        let candidate = OutboundDiscoveryAnnouncement {
            interface: DiscoverableInterface {
                interface_type: "BackboneInterface".to_string(),
                transport: true,
                transport_id: publisher_identity
                    .address_hash()
                    .as_slice()
                    .try_into()
                    .expect("address hash length"),
                name: "encrypted-backbone".to_string(),
                operator_lxmf_address: Some([0x42; 16]),
                reachable_on: Some("relay.example".to_string()),
                port: Some(4242),
                latitude: None,
                longitude: None,
                height: None,
                ifac_netname: None,
                ifac_netkey: None,
                frequency: None,
                bandwidth: None,
                spreading_factor: None,
                coding_rate: None,
                modulation: None,
                channel: None,
            },
            stamp_value: 1,
            interval_secs: 300,
            encrypted: true,
        };
        let payload = encode_discovery_candidate(&candidate, Some(&network_identity))
            .expect("encrypted payload");
        let decrypt_identity = network_identity.clone();
        let record = decode_announce(
            &payload,
            &hex::encode(network_identity.address_hash().as_slice()),
            &[],
            1,
            1.0,
            1,
            Some(move |ciphertext: &[u8]| {
                decrypt_with_identity(
                    &decrypt_identity,
                    decrypt_identity.address_hash().as_slice(),
                    ciphertext,
                )
                .ok()
            }),
        )
        .expect("encrypted discovery roundtrip");
        assert_eq!(
            record.operator_lxmf_address.as_deref(),
            Some("42424242424242424242424242424242")
        );
    }

    #[test]
    fn rns_1_5_encrypted_discovery_rejects_missing_network_identity() {
        let local_identity = PrivateIdentity::new_from_name("rns-1.5-local-only");
        let candidate = OutboundDiscoveryAnnouncement {
            interface: DiscoverableInterface {
                interface_type: "BackboneInterface".to_string(),
                transport: true,
                transport_id: local_identity
                    .address_hash()
                    .as_slice()
                    .try_into()
                    .expect("address hash length"),
                name: "encrypted-backbone".to_string(),
                operator_lxmf_address: None,
                reachable_on: Some("relay.example".to_string()),
                port: Some(4242),
                latitude: None,
                longitude: None,
                height: None,
                ifac_netname: None,
                ifac_netkey: None,
                frequency: None,
                bandwidth: None,
                spreading_factor: None,
                coding_rate: None,
                modulation: None,
                channel: None,
            },
            stamp_value: 1,
            interval_secs: 300,
            encrypted: true,
        };
        assert_eq!(
            encode_discovery_candidate(&candidate, None),
            Err(DiscoveryAnnounceError::MissingNetworkIdentity)
        );
    }
}
