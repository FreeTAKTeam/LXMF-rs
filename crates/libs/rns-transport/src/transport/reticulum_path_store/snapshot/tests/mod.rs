use super::*;
use crate::transport::reticulum_path_store::snapshot_allocation_tests::path_entry;

type SnapshotParts = (Vec<PythonPathEntry>, Vec<PythonTunnelEntry>, Vec<PacketReference>);
type InterfaceView = HashMap<AddressHash, (Option<InterfaceMode>, Option<Hash>)>;

fn seeded(
    count: usize,
    cached: impl Fn(usize) -> bool,
) -> (PathTable, AnnounceTable, Instant, f64) {
    let now = Instant::now();
    let unix = 1_700_000_000.25;
    let mut paths = PathTable::new();
    let mut announces = AnnounceTable::new(count, 3);
    for index in 0..count {
        let mut entry = path_entry(index, 1);
        entry.iface = AddressHash::new_empty();
        if cached(index) {
            let packet = Packet { destination: entry.destination, ..Packet::default() };
            announces.add_cached(&packet, entry.destination, entry.iface);
        }
        paths.restore_python_entry(entry, now, unix);
    }
    (paths, announces, now, unix)
}

// Local owned-vector regression budget; this is not a daemon resource gate.
#[test]
fn dense_checkpoint_capacity_tracks_eligible_rows_without_growth_slack() {
    let (paths, announces, now, unix) = seeded(100_000, |_| true);
    let snapshot = capture(&paths, &TunnelTable::new(), &announces, now, unix, |_| {
        (Some(InterfaceMode::Full), Some(Hash::new_empty()))
    })
    .expect("snapshot");
    assert_eq!(snapshot.kept_entries.len(), 100_000);
    assert_eq!(snapshot.packets.len(), 100_000);
    let metadata = snapshot.kept_entries.capacity() * std::mem::size_of::<PythonPathEntry>();
    let refs = snapshot.packets.capacity() * std::mem::size_of::<PacketReference>();
    let useful =
        100_000 * (std::mem::size_of::<PythonPathEntry>() + std::mem::size_of::<PacketReference>());
    println!("dense metadata_capacity_bytes={metadata} packet_reference_capacity_bytes={refs} useful_bytes={useful}");
    assert!(metadata + refs <= useful + 1024 * 1024,
        "checkpoint scratch retains geometric growth slack: metadata={metadata} refs={refs} useful={useful}");
}

fn legacy_capture(
    paths: &PathTable,
    tunnels: &TunnelTable,
    announces: &AnnounceTable,
    now: Instant,
    unix: f64,
    iface: impl Fn(&AddressHash) -> (Option<InterfaceMode>, Option<Hash>),
) -> io::Result<SnapshotParts> {
    let mut kept_entries = paths.export_python_entries(now, unix, |address| {
        let (mode, hash) = iface(address);
        Some((mode?, hash?))
    });
    let mut packets = Vec::new();
    kept_entries.retain(|entry| {
        let Some(packet) = announces.shared_cached_packet_for_destination(&entry.destination)
        else {
            return false;
        };
        packets.push((entry.packet_hash, entry.iface, packet));
        true
    });
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
    Ok((kept_entries, tunnel_entries, packets))
}

fn assert_no_fill_growth(snapshot: &Snapshot) {
    assert_eq!(
        snapshot.reservation.metadata,
        (snapshot.kept_entries.as_ptr() as usize, snapshot.kept_entries.capacity())
    );
    assert_eq!(
        snapshot.reservation.packets,
        (snapshot.packets.as_ptr() as usize, snapshot.packets.capacity())
    );
}

fn assert_equivalent(new: &Snapshot, old: &SnapshotParts) {
    assert_eq!(new.kept_entries, old.0);
    assert_eq!(new.tunnel_entries, old.1);
    assert_eq!(
        PathTable::encode_python_entries(&new.kept_entries).expect("new path bytes"),
        PathTable::encode_python_entries(&old.0).expect("old path bytes")
    );
    assert_eq!(
        TunnelTable::encode_python_entries(&new.tunnel_entries).expect("new tunnel bytes"),
        TunnelTable::encode_python_entries(&old.1).expect("old tunnel bytes")
    );
    assert_eq!(new.packets.len(), old.2.len());
    for (new, old) in new.packets.iter().zip(&old.2) {
        assert_eq!((new.0, new.1), (old.0, old.1));
        assert!(Arc::ptr_eq(&new.2, &old.2), "packet bodies must be shared in the same order");
    }
    assert_no_fill_growth(new);
}

#[test]
fn sparse_and_empty_snapshots_reserve_only_eligible_active_rows() {
    for stride in [1, 10_000, usize::MAX] {
        let (paths, announces, now, unix) =
            seeded(100_000, |i| stride != usize::MAX && i.is_multiple_of(stride));
        let iface = |_: &AddressHash| (Some(InterfaceMode::Full), Some(Hash::new_empty()));
        let tunnels = TunnelTable::new();
        let new = capture(&paths, &tunnels, &announces, now, unix, iface).expect("new");
        let old = legacy_capture(&paths, &tunnels, &announces, now, unix, iface).expect("old");
        assert_equivalent(&new, &old);
        let eligible = if stride == usize::MAX { 0 } else { 100_000 / stride };
        assert_eq!(new.kept_entries.len(), eligible);
        assert!(
            new.kept_entries.capacity() <= eligible + 1024,
            "filtered-out rows must not dictate capacity"
        );
        assert!(new.packets.capacity() <= eligible + 1024);
    }
}

fn mixed_fixture(count: usize) -> (PathTable, TunnelTable, AnnounceTable, Instant, f64) {
    let (mut paths, announces, now, unix) = seeded(count, |i| !i.is_multiple_of(5));
    for index in 0..count {
        let mut entry = path_entry(index, index % 3);
        entry.iface = path_entry(index % 4, 0).destination;
        entry.hops = (index % 8) as u8;
        entry.timestamp_secs = unix - (index % 12) as f64;
        paths.restore_python_entry(entry, now, unix);
    }
    let mut tunnels = TunnelTable::new();
    let entries = (0..5)
        .map(|t| PythonTunnelEntry {
            tunnel_id: path_entry(500_000 + t, 0).packet_hash,
            interface_hash: None,
            expires_secs: unix + 3600.0,
            paths: (0..16)
                .map(|index| {
                    // Repeat the same destinations across tunnels, including one
                    // cached tunnel-only path (no active mapping) and missing packets.
                    let p = path_entry(index, index % 3);
                    crate::transport::tunnels::PythonTunnelPathEntry {
                        destination: p.destination,
                        timestamp_secs: unix - index as f64,
                        received_from: p.received_from,
                        hops: p.hops,
                        expires_secs: unix + 3600.0,
                        random_blobs: p.random_blobs,
                        interface_hash: None,
                        packet_hash: p.packet_hash,
                    }
                })
                .collect(),
        })
        .collect();
    tunnels.restore_python_entries(entries, now, unix);
    for t in 0..3 {
        tunnels.attach_for_snapshot_test(
            path_entry(500_000 + t, 0).packet_hash,
            path_entry(t, 0).destination,
            now,
        );
    }
    // An attached empty tunnel must still be omitted.
    tunnels.attach_for_snapshot_test(
        path_entry(600_000, 0).packet_hash,
        AddressHash::new_empty(),
        now,
    );
    (paths, tunnels, announces, now, unix)
}

fn mixed_interfaces() -> InterfaceView {
    HashMap::from([
        (
            path_entry(0, 0).destination,
            (Some(InterfaceMode::Roaming), Some(path_entry(700_000, 0).packet_hash)),
        ),
        (path_entry(1, 0).destination, (None, Some(path_entry(700_001, 0).packet_hash))),
        (path_entry(2, 0).destination, (Some(InterfaceMode::AccessPoint), None)),
    ])
}

#[test]
fn mixed_tunnels_preserve_exact_order_optional_interfaces_and_packet_multiplicity() {
    let (paths, tunnels, announces, now, unix) = mixed_fixture(1000);
    let interfaces = mixed_interfaces();
    let iface = |address: &AddressHash| interfaces.get(address).copied().unwrap_or((None, None));
    let new = capture(&paths, &tunnels, &announces, now, unix, iface).expect("new");
    let old = legacy_capture(&paths, &tunnels, &announces, now, unix, iface).expect("old");
    assert_equivalent(&new, &old);
    assert_eq!(new.kept_entries.len(), 200);
    assert_eq!(new.tunnel_entries.len(), 5);
    assert_eq!(new.packets.len(), 260);
    let destination = path_entry(1, 0).destination;
    assert!(!new.kept_entries.iter().any(|p| p.destination == destination));
    let refs = new
        .packets
        .iter()
        .filter(|(hash, _, _)| *hash == path_entry(1, 0).packet_hash)
        .collect::<Vec<_>>();
    assert_eq!(refs.len(), 5, "each tunnel reference survives");
    assert!(refs.iter().any(|(_, iface, _)| *iface == destination), "detached/unmapped fallback");
    assert!(refs
        .iter()
        .any(|(_, iface, _)| *iface
            == AddressHash::new_from_hash(&path_entry(700_000, 0).packet_hash)));
    assert!(
        refs.iter().any(|(_, iface, _)| *iface
            == AddressHash::new_from_hash(&path_entry(700_001, 0).packet_hash)),
        "tunnel hash survives missing active mode"
    );
}

#[test]
fn missing_active_interfaces_do_not_reserve_raw_path_count_or_discard_tunnels() {
    let (paths, tunnels, announces, now, unix) = mixed_fixture(1000);
    let iface = |_: &AddressHash| (None, None);
    let new = capture(&paths, &tunnels, &announces, now, unix, iface).expect("new");
    let old = legacy_capture(&paths, &tunnels, &announces, now, unix, iface).expect("old");
    assert_equivalent(&new, &old);
    assert!(new.kept_entries.is_empty());
    assert_eq!(new.kept_entries.capacity(), 0);
    assert_eq!(new.packets.len(), 60);
    assert_eq!(new.tunnel_entries.len(), 5);
    assert!(new.tunnel_entries.iter().all(|t| t.interface_hash.is_none()));
}

#[test]
fn queued_and_cached_packet_sources_both_count_and_preserve_shared_handles() {
    let (paths, mut announces, now, unix) = seeded(3, |_| false);
    let queued = path_entry(1, 0);
    let cached = path_entry(2, 0);
    announces.add(
        &Packet { destination: queued.destination, ..Packet::default() },
        queued.destination,
        queued.iface,
    );
    announces.add_cached(
        &Packet { destination: cached.destination, ..Packet::default() },
        cached.destination,
        cached.iface,
    );
    let tunnels = TunnelTable::new();
    let iface = |_: &AddressHash| (Some(InterfaceMode::Full), Some(Hash::new_empty()));
    let new = capture(&paths, &tunnels, &announces, now, unix, iface).expect("new");
    let old = legacy_capture(&paths, &tunnels, &announces, now, unix, iface).expect("old");
    assert_equivalent(&new, &old);
    assert_eq!(new.kept_entries.len(), 2);
    for entry in [&queued, &cached] {
        assert!(announces.has_cached_packet_for_destination(&entry.destination));
        let original = announces
            .shared_cached_packet_for_destination(&entry.destination)
            .expect("original handle");
        let (_, _, captured) = new
            .packets
            .iter()
            .find(|(hash, _, _)| *hash == entry.packet_hash)
            .expect("captured handle");
        assert!(Arc::ptr_eq(&original, captured));
    }
}

#[test]
fn public_exporter_still_invokes_stateful_lookup_once_per_visited_route() {
    let (paths, _, now, unix) = seeded(100, |_| false);
    let mut calls = 0usize;
    let rows = paths.export_python_entries(now, unix, |_| {
        calls += 1;
        (calls.is_multiple_of(2)).then_some((InterfaceMode::Full, Hash::new_empty()))
    });
    assert_eq!(calls, 100);
    assert_eq!(rows.len(), 50);
}

#[test]
#[ignore = "controlled checkpoint capture allocation/call-cost probe; one fresh process per mode/case"]
fn checkpoint_capture_allocation_probe() {
    use crate::transport::reticulum_path_store::snapshot_allocation_tests::ALLOCATOR;
    use sha2::{Digest, Sha256};
    use std::cell::Cell;
    let mode = std::env::var("LXMF_CHECKPOINT_MODE").expect("legacy or candidate");
    let case = std::env::var("LXMF_CHECKPOINT_CASE").expect("dense, sparse or mixed");
    let (paths, tunnels, announces, now, unix) = if case == "mixed" {
        mixed_fixture(100_000)
    } else {
        let stride = if case == "dense" {
            1
        } else {
            assert_eq!(case, "sparse");
            10_000
        };
        let (p, a, n, u) = seeded(100_000, |i| i.is_multiple_of(stride));
        (p, TunnelTable::new(), a, n, u)
    };
    let interfaces = mixed_interfaces();
    let calls = Cell::new(0usize);
    let iface = |address: &AddressHash| {
        calls.set(calls.get() + 1);
        if case == "mixed" {
            interfaces.get(address).copied().unwrap_or((None, None))
        } else {
            (Some(InterfaceMode::Full), Some(Hash::new_empty()))
        }
    };
    let before = ALLOCATOR.allocated();
    let total_before = ALLOCATOR.total_allocated();
    let prior_peak = ALLOCATOR.max_allocated().saturating_sub(before);
    let started = Instant::now();
    let (active, tunnel, packets, stable) = if mode == "candidate" {
        let new = capture(&paths, &tunnels, &announces, now, unix, iface).expect("capture");
        assert_no_fill_growth(&new);
        (new.kept_entries, new.tunnel_entries, new.packets, true)
    } else {
        assert_eq!(mode, "legacy");
        let (a, t, p) =
            legacy_capture(&paths, &tunnels, &announces, now, unix, iface).expect("legacy capture");
        (a, t, p, false)
    };
    let capture_elapsed_ns = started.elapsed().as_nanos();
    let interface_lookup_calls = calls.get();
    let capture_current = ALLOCATOR.allocated().saturating_sub(before);
    let capture_total = ALLOCATOR.total_allocated().saturating_sub(total_before);
    let metadata_capacity_bytes = active.capacity() * std::mem::size_of::<PythonPathEntry>();
    let packet_reference_capacity_bytes =
        packets.capacity() * std::mem::size_of::<PacketReference>();
    let payload = PathTable::encode_python_entries(&active).expect("path bytes");
    let tunnel_payload = TunnelTable::encode_python_entries(&tunnel).expect("tunnel bytes");
    let peak = ALLOCATOR.max_allocated().saturating_sub(before);
    let total = ALLOCATOR.total_allocated().saturating_sub(total_before);
    let encoded_current = ALLOCATOR.allocated().saturating_sub(before);
    // Check the old/new order on the SAME immutable fixture: HashMap iteration
    // order is randomized across processes and must not be sorted for parity.
    // All allocation/timing fields above were captured before this oracle work.
    let other = if mode == "candidate" {
        legacy_capture(&paths, &tunnels, &announces, now, unix, iface).expect("old oracle")
    } else {
        let other = capture(&paths, &tunnels, &announces, now, unix, iface).expect("new oracle");
        assert_no_fill_growth(&other);
        (other.kept_entries, other.tunnel_entries, other.packets)
    };
    assert_eq!(active, other.0);
    assert_eq!(tunnel, other.1);
    assert_eq!(payload, PathTable::encode_python_entries(&other.0).expect("oracle paths"));
    assert_eq!(
        tunnel_payload,
        TunnelTable::encode_python_entries(&other.1).expect("oracle tunnels")
    );
    assert_eq!(packets.len(), other.2.len());
    for (left, right) in packets.iter().zip(&other.2) {
        assert_eq!((left.0, left.1), (right.0, right.1));
        assert!(Arc::ptr_eq(&left.2, &right.2));
    }
    let mut packet_digest = Sha256::new();
    for (hash, iface, packet) in &packets {
        packet_digest.update(hash.as_slice());
        packet_digest.update(iface.as_slice());
        packet_digest.update(packet.to_bytes().expect("packet bytes"));
    }
    println!(
        "{}",
        serde_json::json!({"mode":mode,"case":case,"native_paths":paths.len(),
        "active_rows":active.len(),"tunnel_rows":tunnel.len(),"packet_refs":packets.len(),
        "eligible_active_count":active.len(),"eligible_tunnel_reference_count":packets.len()-active.len(),
        "metadata_len":active.len(),"metadata_capacity":active.capacity(),"packet_ref_len":packets.len(),"packet_ref_capacity":packets.capacity(),
        "old_new_exact_bytes_order_and_shared_refs_equal":true,
        "interface_lookup_calls":interface_lookup_calls,"capture_elapsed_ns":capture_elapsed_ns,
        "metadata_capacity_bytes":metadata_capacity_bytes,"packet_reference_capacity_bytes":packet_reference_capacity_bytes,
        "reserved_fill_pointers_capacities_unchanged":stable,
        "capture_retained_requested_bytes":capture_current,"capture_total_requested_bytes":capture_total,
        "encoded_retained_requested_bytes":encoded_current,"capture_encode_total_requested_bytes":total,
        "global_peak_extra_requested_bytes":peak,"fixture_prior_peak_extra_requested_bytes":prior_peak,
        "path_output_bytes":payload.len(),"tunnel_output_bytes":tunnel_payload.len(),
        "path_sha256":hex::encode(Sha256::digest(&payload)),"tunnel_sha256":hex::encode(Sha256::digest(&tunnel_payload)),
        "packet_refs_and_bytes_sha256":hex::encode(packet_digest.finalize()),
        "scope":"real native tables/cache and production capture; excludes filesystem, runtime allocator RSS/charges, cleanup and locks; global peak may include disclosed fixture high-water"})
    );
}
