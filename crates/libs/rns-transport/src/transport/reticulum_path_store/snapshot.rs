//! Owned checkpoint metadata captured under one stable interface/cache view.
use super::*;
use crate::iface::InterfaceMode;
use crate::transport::announce_table::AnnounceTable;
use crate::transport::path_table::PythonPathEntry;
use crate::transport::tunnels::{PythonTunnelEntry, TunnelTable};

type PacketReference = (Hash, AddressHash, Arc<Packet>);

pub(super) struct Snapshot {
    pub kept_entries: Vec<PythonPathEntry>,
    pub tunnel_entries: Vec<PythonTunnelEntry>,
    pub packets: Vec<PacketReference>,
    #[cfg(test)]
    reservation: Reservation,
}

#[cfg(test)]
struct Reservation {
    metadata: (usize, usize),
    packets: (usize, usize),
}

/// The caller holds the handler and interface guards through this call. This
/// private lookup is a stable view, unlike the public exporter's FnMut callback.
pub(super) fn capture(
    paths: &PathTable,
    tunnels: &TunnelTable,
    announces: &AnnounceTable,
    now: Instant,
    unix: f64,
    iface: impl Fn(&AddressHash) -> (Option<InterfaceMode>, Option<Hash>),
) -> io::Result<Snapshot> {
    let active_count = paths
        .entries()
        .filter(|(destination, entry)| {
            let (mode, hash) = iface(&entry.iface);
            mode.is_some()
                && hash.is_some()
                && announces.has_cached_packet_for_destination(destination)
        })
        .count();
    // A destination can occur in multiple tunnels and on distinct interfaces.
    // Count every cached reference, including detached/unmapped tunnels.
    let references = tunnels
        .destinations()
        .filter(|destination| announces.has_cached_packet_for_destination(destination))
        .try_fold(active_count, |count, _| count.checked_add(1))
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "checkpoint reference count overflow")
        })?;
    let mut kept_entries = Vec::new();
    kept_entries
        .try_reserve_exact(active_count)
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    let mut packets = Vec::new();
    packets
        .try_reserve_exact(references)
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    #[cfg(test)]
    let reservation = Reservation {
        metadata: (kept_entries.as_ptr() as usize, kept_entries.capacity()),
        packets: (packets.as_ptr() as usize, packets.capacity()),
    };
    for (destination, entry) in paths.entries() {
        let (Some(mode), Some(hash)) = iface(&entry.iface) else { continue };
        let Some(packet) = announces.shared_cached_packet_for_destination(destination) else {
            continue;
        };
        kept_entries.push(entry.to_python_entry(*destination, now, unix, mode, hash));
        packets.push((entry.packet_hash, entry.iface, packet));
    }
    let mut tunnel_entries = tunnels.export_python_entries(now, unix, |address| iface(address).1);
    for tunnel in &mut tunnel_entries {
        tunnel.paths.retain(|path| {
            let Some(packet) = announces.shared_cached_packet_for_destination(&path.destination)
            else {
                return false;
            };
            let iface = path
                .interface_hash
                .map(|hash| AddressHash::new_from_hash(&hash))
                .unwrap_or(path.destination);
            packets.push((path.packet_hash, iface, packet));
            true
        });
    }
    tunnel_entries.retain(|entry| !entry.paths.is_empty());
    debug_assert_eq!(kept_entries.len(), active_count);
    debug_assert_eq!(packets.len(), references);
    Ok(Snapshot {
        kept_entries,
        tunnel_entries,
        packets,
        #[cfg(test)]
        reservation,
    })
}

#[cfg(test)]
mod tests;
