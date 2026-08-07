use super::*;
use crate::iface::{Interface, InterfaceContext};
use std::io;
use std::path::Path;

impl Transport {
    pub async fn add_interface<T, F, R>(&self, interface: T, worker: F) -> AddressHash
    where
        T: Interface,
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        R::Output: Send + 'static,
    {
        self.iface_manager.lock().await.spawn(interface, worker)
    }

    pub async fn remove_interface(&self, address: AddressHash) -> bool {
        self.stop_interface(address).await
    }

    pub async fn interface_hashes(&self) -> std::collections::HashSet<AddressHash> {
        self.iface_manager.lock().await.interface_hashes()
    }

    pub async fn internal_identity(&self) -> AddressHash {
        *self.handler.lock().await.config.identity.address_hash()
    }

    pub async fn is_shared_instance(&self, interface: &AddressHash) -> bool {
        self.iface_manager.lock().await.is_shared_instance(interface)
    }

    pub async fn should_apply_delta(&self, packet: &Packet, interface: &AddressHash) -> bool {
        let should_apply = {
            let handler = self.handler.lock().await;
            handler.local_hops_delta_for_packet(packet).is_some()
        };
        if !should_apply {
            return false;
        }
        !self.iface_manager.lock().await.is_shared_instance(interface)
    }

    pub async fn void_queues(&self) -> usize {
        let dropped = self.iface_manager.lock().await.drop_announce_queues();
        let mut handler = self.handler.lock().await;
        let dropped_held = handler.announce_limits.clear_held_announces();
        handler.packet_signal_cache.clear();
        handler.resource_response_packets.clear();
        dropped + dropped_held
    }

    pub async fn set_network_identity(&self, identity: PrivateIdentity) -> bool {
        let mut handler = self.handler.lock().await;
        if handler.network_identity.is_some() {
            return false;
        }
        handler.network_identity = Some(identity);
        true
    }

    pub async fn has_network_identity(&self) -> bool {
        self.handler.lock().await.network_identity.is_some()
    }

    pub async fn enable_discovery(&self) -> bool {
        let mut handler = self.handler.lock().await;
        let changed = !handler.discovery_enabled;
        handler.discovery_enabled = true;
        changed
    }

    pub async fn discovery_enabled(&self) -> bool {
        self.handler.lock().await.discovery_enabled
    }

    pub async fn prioritize_interfaces(&self) {
        self.iface_manager.lock().await.prioritize_interfaces();
    }

    pub const fn should_cache(_packet: &Packet) -> bool {
        false
    }

    pub async fn cache_packet<P: AsRef<Path>>(
        &self,
        storage_path: P,
        packet: &Packet,
        interface_reference: Option<&str>,
        force: bool,
        announce: bool,
    ) -> io::Result<Option<Hash>> {
        if !force && !Self::should_cache(packet) {
            return Ok(None);
        }
        ReticulumPacketDiskCache::new(storage_path)
            .write(packet, interface_reference, announce)
            .await
            .map(Some)
    }

    pub async fn get_cached_packet<P: AsRef<Path>>(
        &self,
        storage_path: P,
        packet_hash: Hash,
        announce: bool,
    ) -> io::Result<Option<CachedPacket>> {
        ReticulumPacketDiskCache::new(storage_path).read(packet_hash, announce).await
    }

    pub async fn save_packet_hashlist<P: AsRef<Path>>(&self, storage_path: P) -> io::Result<usize> {
        let hashes = {
            let handler = self.handler.lock().await;
            if handler.config.transport_enabled {
                handler
                    .packet_cache
                    .try_lock()
                    .map_err(|_| io::Error::other("packet cache is busy"))?
                    .hashes()
            } else {
                Vec::new()
            }
        };
        ReticulumPacketDiskCache::new(storage_path.as_ref())
            .save_packet_hashlist(storage_path, &hashes)
            .await?;
        Ok(hashes.len())
    }

    pub async fn persist_data<P: AsRef<Path>>(
        &self,
        storage_path: P,
    ) -> io::Result<(usize, usize)> {
        let hash_count = self.save_packet_hashlist(storage_path.as_ref()).await?;
        let path_count = self.save_reticulum_path_table(storage_path).await?;
        Ok((hash_count, path_count))
    }
}
