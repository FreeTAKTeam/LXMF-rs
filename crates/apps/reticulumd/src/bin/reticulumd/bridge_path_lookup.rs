use rns_rpc::PathLookupBridge;
use rns_transport::destination_hash::parse_destination_hash_required;
use rns_transport::discovery::{DiscoveryListFilter, InterfaceDiscoveryStore};
use rns_transport::hash::{AddressHash, Hash, HASH_SIZE};
use rns_transport::transport::Transport;
use serde_json::{json, Value as JsonValue};
use std::sync::Arc;

pub(crate) struct DaemonPathLookupBridge {
    transport: Arc<Transport>,
    discovery_store: Option<InterfaceDiscoveryStore>,
    discovery_sources: Vec<String>,
}

impl DaemonPathLookupBridge {
    #[cfg(test)]
    pub(crate) fn new(transport: Arc<Transport>) -> Self {
        Self { transport, discovery_store: None, discovery_sources: Vec::new() }
    }

    pub(crate) fn with_discovery_store(
        transport: Arc<Transport>,
        storage_path: impl AsRef<std::path::Path>,
        discovery_sources: Vec<String>,
    ) -> Self {
        Self {
            transport,
            discovery_store: Some(InterfaceDiscoveryStore::new(storage_path)),
            discovery_sources,
        }
    }

    fn destination_hash(destination: &str) -> Result<AddressHash, std::io::Error> {
        parse_destination_hash_required(destination)
            .map(AddressHash::new)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))
    }

    fn optional_hash(
        value: Option<&str>,
        field: &str,
    ) -> Result<Option<AddressHash>, std::io::Error> {
        value
            .map(Self::destination_hash)
            .transpose()
            .map_err(|err| std::io::Error::new(err.kind(), format!("{field} {err}")))
    }

    fn hash_hex(value: AddressHash) -> String {
        value.to_hex_string()
    }

    fn packet_hash(value: &str) -> Result<Hash, std::io::Error> {
        let bytes = hex::decode(value).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("packet_hash must be hexadecimal: {err}"),
            )
        })?;
        let bytes: [u8; HASH_SIZE] = bytes.try_into().map_err(|bytes: Vec<u8>| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("packet_hash must be {HASH_SIZE} bytes, got {}", bytes.len()),
            )
        })?;
        Ok(Hash::new(bytes))
    }

    fn run_transport<F, T>(&self, f: F) -> Result<T, std::io::Error>
    where
        F: FnOnce(Arc<Transport>) -> Result<T, std::io::Error> + Send + 'static,
        T: Send + 'static,
    {
        let transport = self.transport.clone();
        std::thread::spawn(move || f(transport))
            .join()
            .map_err(|_| std::io::Error::other("path lookup helper thread panicked"))?
    }
}

impl PathLookupBridge for DaemonPathLookupBridge {
    fn has_path(&self, destination: &str) -> Result<bool, std::io::Error> {
        let destination = Self::destination_hash(destination)?;
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build path lookup runtime: {err}"))
                })?;
            Ok(runtime.block_on(async move { transport.has_path(&destination).await }))
        })
    }

    fn request_path(&self, destination: &str) -> Result<(), std::io::Error> {
        self.request_path_scoped(destination, None, None)
    }

    fn request_path_scoped(
        &self,
        destination: &str,
        on_iface: Option<&str>,
        tag: Option<&[u8]>,
    ) -> Result<(), std::io::Error> {
        let destination = Self::destination_hash(destination)?;
        let on_iface = Self::optional_hash(on_iface, "on_iface")?;
        let tag = tag.map(Vec::from);
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build path request runtime: {err}"))
                })?;
            runtime.block_on(async move {
                let dispatch = transport.request_path(&destination, on_iface, tag).await;
                if on_iface.is_some() && dispatch.matched_ifaces == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "scoped path request interface did not match an outgoing interface",
                    ));
                }
                Ok(())
            })
        })
    }

    fn path_status(&self, destination: &str) -> Result<JsonValue, std::io::Error> {
        let destination = Self::destination_hash(destination)?;
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build path status runtime: {err}"))
                })?;
            let status_transport = transport.clone();
            let status =
                runtime.block_on(async move { status_transport.path_status(&destination).await });
            let (interface_bitrate, interface_mtu) = runtime.block_on(async {
                let Some(interface) = status.interface else {
                    return (None, None);
                };
                let manager = transport.iface_manager();
                let manager = manager.lock().await;
                let bitrate = manager.announce_pacing(&interface).map(|(bitrate, _)| bitrate);
                let mtu = manager.mtu(&interface).map(|value| value as u64);
                (bitrate, mtu)
            });
            Ok(json!({
                "destination_hash": Self::hash_hex(status.destination),
                "path_found": status.path_found,
                "next_hop": status.next_hop.map(Self::hash_hex),
                "interface": status.interface.map(Self::hash_hex),
                "interface_name": status.interface.map(Self::hash_hex),
                "interface_bitrate": interface_bitrate,
                "interface_mtu": interface_mtu,
                "hops": status.hops,
            }))
        })
    }

    fn remove_paths_for_identity(&self, identity: &str) -> Result<usize, std::io::Error> {
        let identity = Self::destination_hash(identity)?;
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!(
                        "failed to build blackhole path cleanup runtime: {err}"
                    ))
                })?;
            Ok(runtime
                .block_on(async move { transport.expire_paths_for_identity(&identity).await }))
        })
    }

    fn set_identity_blackholed(
        &self,
        identity: &str,
        blackholed: bool,
    ) -> Result<usize, std::io::Error> {
        self.set_identity_blackholed_until(identity, blackholed, None)
    }
    fn set_identity_blackholed_until(
        &self,
        identity: &str,
        blackholed: bool,
        until: Option<f64>,
    ) -> Result<usize, std::io::Error> {
        let identity = Self::destination_hash(identity)?;
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!(
                        "failed to build expiring blackhole synchronization runtime: {err}"
                    ))
                })?;
            Ok(runtime.block_on(async move {
                transport.set_identity_blackholed_until(identity, blackholed, until).await
            }))
        })
    }

    fn drop_path(&self, destination: &str) -> Result<bool, std::io::Error> {
        let destination = Self::destination_hash(destination)?;
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build path mutation runtime: {err}"))
                })?;
            Ok(runtime.block_on(async move { transport.expire_path(&destination).await }))
        })
    }

    fn drop_all_via(&self, transport_hash: &str) -> Result<usize, std::io::Error> {
        let transport_hash = Self::destination_hash(transport_hash)?;
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build path mutation runtime: {err}"))
                })?;
            Ok(runtime.block_on(async move { transport.expire_paths_via(&transport_hash).await }))
        })
    }

    fn drop_announce_queues(&self) -> Result<usize, std::io::Error> {
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build announce queue runtime: {err}"))
                })?;
            Ok(runtime.block_on(async move { transport.drop_announce_queues().await }))
        })
    }

    fn rate_table(&self) -> Result<JsonValue, std::io::Error> {
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build rate table runtime: {err}"))
                })?;
            let rows = runtime.block_on(async move { transport.announce_rate_table().await });
            Ok(JsonValue::Array(
                rows.into_iter()
                    .map(|row| {
                        json!({
                            "hash": row.destination.to_hex_string(),
                            "last": row.last,
                            "rate_violations": row.rate_violations,
                            "blocked_until": row.blocked_until,
                            "timestamps": row.timestamps,
                        })
                    })
                    .collect(),
            ))
        })
    }

    fn packet_signal(&self, packet_hash: &str) -> Result<JsonValue, std::io::Error> {
        let packet_hash = Self::packet_hash(packet_hash)?;
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build packet signal runtime: {err}"))
                })?;
            let signal =
                runtime.block_on(async move { transport.packet_signal(&packet_hash).await });
            Ok(match signal {
                Some(signal) => json!({ "rssi": signal.rssi, "snr": signal.snr, "q": signal.q }),
                None => JsonValue::Null,
            })
        })
    }

    fn discovered_interfaces(&self) -> Result<JsonValue, std::io::Error> {
        let Some(store) = &self.discovery_store else {
            return Ok(JsonValue::Array(Vec::new()));
        };
        let rows = store.list(
            rns_transport::time::now_epoch_secs_u64() as f64,
            &self.discovery_sources,
            DiscoveryListFilter::default(),
        )?;
        serde_json::to_value(rows).map_err(std::io::Error::other)
    }

    fn link_count(&self) -> Result<usize, std::io::Error> {
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build link count runtime: {err}"))
                })?;
            Ok(runtime.block_on(async move { transport.link_count().await }))
        })
    }

    fn active_link_count(&self) -> Result<usize, std::io::Error> {
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| std::io::Error::other(format!("active link runtime: {err}")))?;
            Ok(runtime.block_on(async move { transport.active_link_count().await }))
        })
    }

    fn lowest_interface_bitrate(&self) -> Result<Option<u64>, std::io::Error> {
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| std::io::Error::other(format!("bitrate runtime: {err}")))?;
            Ok(runtime.block_on(async move { transport.lowest_interface_bitrate().await }))
        })
    }

    fn medium_path_timeout(&self) -> Result<f64, std::io::Error> {
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| std::io::Error::other(format!("timeout runtime: {err}")))?;
            Ok(runtime.block_on(async move { transport.medium_path_timeout().await }).as_secs_f64())
        })
    }

    fn transport_status(&self) -> Result<JsonValue, std::io::Error> {
        self.run_transport(crate::bridge_transport_status::build_transport_status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::transport::TransportConfig;

    fn bridge() -> DaemonPathLookupBridge {
        let identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
        let config = TransportConfig::new("path-lookup-bridge-test", &identity, true);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("transport construction runtime");
        let _guard = runtime.enter();
        DaemonPathLookupBridge::new(Arc::new(Transport::new(config)))
    }

    fn bridge_with_iface() -> (DaemonPathLookupBridge, AddressHash) {
        let identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
        let config = TransportConfig::new("path-lookup-bridge-test", &identity, true);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("transport construction runtime");
        let _guard = runtime.enter();
        let transport = Arc::new(Transport::new(config));
        let iface = runtime.block_on(async {
            let manager = transport.iface_manager();
            let mut manager = manager.lock().await;
            *manager.new_channel(16).address()
        });
        (DaemonPathLookupBridge::new(transport), iface)
    }

    #[test]
    fn path_lookup_bridge_reports_missing_path_without_rpc_transport_leak() {
        let bridge = bridge();
        let known = bridge.has_path("00112233445566778899aabbccddeeff").expect("query path");

        assert!(!known);
    }

    #[test]
    fn path_lookup_bridge_reports_fresh_transport_link_count() {
        let bridge = bridge();

        assert_eq!(bridge.link_count().expect("link count"), 0);
    }

    #[test]
    fn rns_1_5_transport_status_exposes_queue_traffic_and_violation_contract() {
        let (bridge, iface) = bridge_with_iface();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("status fixture runtime");
        runtime.block_on(async {
            let manager = bridge.transport.iface_manager();
            let mut manager = manager.lock().await;
            assert!(manager.record_inbound_traffic(
                iface,
                rns_transport::packet::PacketType::Announce,
                false,
                100
            ));
            assert!(manager.record_protocol_violation(iface, "fixture"));
            assert!(manager.record_ifac_violation(iface, "fixture"));
            assert!(manager.record_packet_filter_hit(iface));
        });

        let status = bridge.transport_status().expect("transport status");
        assert_eq!(status["inbound_queues"]["total"], 0);
        assert_eq!(status["inbound_queues"]["total_limit"], 1288);
        assert_eq!(status["traffic"]["rx_bytes"], 100);
        assert_eq!(status["traffic"]["announce_rx_count"], 1);
        assert_eq!(status["link_count"], 0);
        assert_eq!(status["active_link_count"], 0);
        assert_eq!(status["interfaces"][0]["address"], iface.to_hex_string());
        assert_eq!(status["interfaces"][0]["violations"]["protocol"], 1);
        assert_eq!(status["interfaces"][0]["violations"]["ifac"], 1);
        assert_eq!(status["interfaces"][0]["violations"]["packet_filter_hits"], 1);
    }

    #[test]
    fn path_lookup_bridge_reports_bare_hex_path_status_hashes() {
        let bridge = bridge();
        let status = bridge.path_status("00112233445566778899aabbccddeeff").expect("path status");

        assert_eq!(status["destination_hash"].as_str(), Some("00112233445566778899aabbccddeeff"));
        assert_eq!(status["path_found"].as_bool(), Some(false));
        assert_eq!(status["next_hop"], JsonValue::Null);
        assert_eq!(status["interface"], JsonValue::Null);

        let next_hop =
            AddressHash::new_from_hex_string("8899aabbccddeeff0011223344556677").expect("hash");
        let interface =
            AddressHash::new_from_hex_string("fedcba98765432100123456789abcdef").expect("hash");
        assert_eq!(DaemonPathLookupBridge::hash_hex(next_hop), "8899aabbccddeeff0011223344556677");
        assert_eq!(DaemonPathLookupBridge::hash_hex(interface), "fedcba98765432100123456789abcdef");
    }

    #[test]
    fn path_lookup_bridge_dispatches_request_path() {
        let bridge = bridge();

        bridge.request_path("00112233445566778899aabbccddeeff").expect("dispatch path request");
    }

    #[test]
    fn path_lookup_bridge_dispatches_scoped_request_path() {
        let (bridge, iface) = bridge_with_iface();

        bridge
            .request_path_scoped(
                "00112233445566778899aabbccddeeff",
                Some(&hex::encode(iface.as_slice())),
                Some(&[1, 2, 3, 4]),
            )
            .expect("dispatch scoped path request");
    }

    #[test]
    fn path_lookup_bridge_rejects_unknown_scoped_iface() {
        let bridge = bridge();

        let err = bridge
            .request_path_scoped(
                "00112233445566778899aabbccddeeff",
                Some("aabbccddeeff00112233445566778899"),
                Some(&[1, 2, 3, 4]),
            )
            .expect_err("unknown scoped iface should fail");

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(err.to_string().contains("scoped path request interface"));
    }

    #[test]
    fn path_lookup_bridge_rejects_invalid_scoped_iface() {
        let bridge = bridge();

        let err = bridge
            .request_path_scoped(
                "00112233445566778899aabbccddeeff",
                Some("abcd"),
                Some(&[1, 2, 3, 4]),
            )
            .expect_err("invalid scoped iface should fail");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("on_iface"));
    }
}
