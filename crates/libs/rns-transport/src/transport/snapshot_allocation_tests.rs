//! Run this ignored probe alone in a fresh test process: allocator peak is global.
use crate::destination::RAND_HASH_LENGTH;
use crate::hash::{AddressHash, Hash};
use crate::transport::path_table::{PathTable, PythonPathEntry};
use crate::transport::tunnels::{PythonTunnelEntry, PythonTunnelPathEntry, TunnelTable};
use sha2::{Digest, Sha256};
use std::time::Instant;

// Safe instrumentation only in the library test binary, without an allocation
// limit. The daemon already uses cap; no production allocator is changed.
#[global_allocator]
static ALLOCATOR: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, usize::MAX);

pub(super) fn path_entry(index: usize, blobs: usize) -> PythonPathEntry {
    let hash = Hash::new_from_slice(&index.to_be_bytes());
    PythonPathEntry {
        destination: AddressHash::new_from_hash(&hash),
        timestamp_secs: 1_700_000_000.25,
        received_from: AddressHash::new_from_hash(&hash),
        hops: (index % 256) as u8,
        expires_secs: 1_700_604_800.25,
        random_blobs: vec![[7; RAND_HASH_LENGTH]; blobs],
        iface: AddressHash::new_from_hash(&hash),
        interface_hash: hash,
        packet_hash: hash,
    }
}

pub(super) fn tunnel_entry(index: usize, blobs: usize) -> PythonTunnelEntry {
    let path = path_entry(index, blobs);
    PythonTunnelEntry {
        tunnel_id: path.packet_hash,
        interface_hash: (index.is_multiple_of(2)).then_some(path.interface_hash),
        expires_secs: path.expires_secs,
        paths: vec![PythonTunnelPathEntry {
            destination: path.destination,
            timestamp_secs: path.timestamp_secs,
            received_from: path.received_from,
            hops: path.hops,
            expires_secs: path.expires_secs,
            random_blobs: path.random_blobs,
            interface_hash: None,
            packet_hash: path.packet_hash,
        }],
    }
}

#[test]
#[ignore = "isolated 100k-entry allocator probe; run only this test with --ignored --exact --nocapture"]
fn persistence_codec_allocation_probe() {
    let mode = std::env::var("LXMF_SNAPSHOT_CODEC").unwrap_or_else(|_| "paths".into());
    let paths =
        if mode == "paths" { (0..100_000).map(|i| path_entry(i, 1)).collect() } else { Vec::new() };
    let tunnels = if mode == "tunnels" {
        (0..100_000).map(|i| tunnel_entry(i, 1)).collect()
    } else {
        Vec::new()
    };
    let before = ALLOCATOR.allocated();
    let total_before = ALLOCATOR.total_allocated();
    let start = Instant::now();
    let encoded = if mode == "paths" {
        PathTable::encode_python_entries(&paths)
    } else {
        assert_eq!(mode, "tunnels");
        TunnelTable::encode_python_entries(&tunnels)
    }
    .expect("encode owned metadata");
    let elapsed_ns = start.elapsed().as_nanos();
    let peak_extra = ALLOCATOR.max_allocated().saturating_sub(before);
    let total = ALLOCATOR.total_allocated().saturating_sub(total_before);
    let retained_extra = ALLOCATOR.allocated().saturating_sub(before);
    assert!(
        peak_extra <= encoded.len().saturating_add(1024 * 1024),
        "codec scratch must stay bounded by output size: peak={peak_extra} output={}",
        encoded.len()
    );
    assert!(
        total <= encoded.len().saturating_add(1024 * 1024),
        "codec allocation churn exceeds one output buffer"
    );
    println!(
        "{}",
        serde_json::json!({"mode":mode,"entries":100000,"encoded_bytes":encoded.len(),
        "peak_extra_requested_bytes":peak_extra,"total_requested_bytes":total,
        "retained_extra_requested_bytes":retained_extra,"elapsed_ns":elapsed_ns,
        "sha256":hex::encode(Sha256::digest(&encoded))})
    );
}

/// Allocation ownership microprobe: real native PathTable export + codec, with
/// the production packet-reference tuple layout. Shared dummy packet bodies
/// isolate snapshot scratch; this is not a filesystem/restore benchmark.
#[test]
#[ignore = "isolated native 100k-route ownership probe; fresh exact-test process only"]
fn persistence_snapshot_stage_probe() {
    use crate::iface::InterfaceMode;
    use crate::packet::Packet;
    use std::collections::HashSet;
    use std::sync::Arc;

    let now = Instant::now();
    let unix = 1_700_000_000.25;
    let mut native = PathTable::new();
    for index in 0..100_000 {
        let mut entry = path_entry(index, 1);
        entry.hops = 1;
        entry.iface = AddressHash::new_empty();
        native.restore_python_entry(entry, now, unix);
    }
    let packet = Arc::new(Packet::default());
    let mut stages = Vec::with_capacity(5);
    let before = ALLOCATOR.allocated();
    let total_before = ALLOCATOR.total_allocated();
    let prior_peak_extra = ALLOCATOR.max_allocated().saturating_sub(before);
    let stamp = |name| {
        (
            name,
            ALLOCATOR.allocated().saturating_sub(before),
            ALLOCATOR.max_allocated().saturating_sub(before),
            ALLOCATOR.total_allocated().saturating_sub(total_before),
        )
    };
    let start = Instant::now();
    let entries =
        native.export_python_entries(now, unix, |_| Some((InterfaceMode::Full, Hash::new_empty())));
    assert_eq!(entries.len(), 100_000);
    let metadata_vec_bytes = entries.capacity() * std::mem::size_of::<PythonPathEntry>();
    let random_blob_bytes: usize = entries
        .iter()
        .map(|entry| entry.random_blobs.capacity() * std::mem::size_of::<[u8; RAND_HASH_LENGTH]>())
        .sum();
    stages.push(stamp("native_export"));
    let mut packets = Vec::new();
    for entry in &entries {
        packets.push((entry.packet_hash, entry.iface, Arc::clone(&packet)));
    }
    let packet_refs_bytes =
        packets.capacity() * std::mem::size_of::<(Hash, AddressHash, Arc<Packet>)>();
    stages.push(stamp("packet_references"));
    let payload = PathTable::encode_python_entries(&entries).expect("snapshot bytes");
    let export_refs_encode_elapsed_ns = start.elapsed().as_nanos();
    stages.push(stamp("encoded_with_metadata"));
    drop(entries);
    stages.push(stamp("metadata_released"));
    let mut hashes = HashSet::with_capacity(packets.len());
    hashes.extend(packets.iter().map(|(hash, _, _)| *hash));
    assert_eq!(hashes.len(), 100_000);
    stages.push(stamp("cleanup_hash_set"));
    let peak = ALLOCATOR.max_allocated().saturating_sub(before);
    println!(
        "{}",
        serde_json::json!({"entries":100000,
        "metadata_vec_requested_bytes":metadata_vec_bytes,"random_blob_requested_bytes":random_blob_bytes,
        "packet_reference_vec_requested_bytes":packet_refs_bytes,"fixture_construction_prior_peak_extra":prior_peak_extra,"output_bytes":payload.len(),
        "output_capacity":payload.capacity(),"peak_extra_requested_bytes":peak,
        "stages_name_current_peak_total_requested_bytes":stages,"export_refs_encode_elapsed_ns":export_refs_encode_elapsed_ns,
        "scope":"native export, production packet-reference layout, encoding and hash-set scratch; excludes packet bodies, I/O, lock latency and allocator RSS retention"})
    );
    assert_eq!(&payload[..5], &[0xdd, 0, 1, 0x86, 0xa0]);
    assert!(
        peak <= metadata_vec_bytes
            + random_blob_bytes
            + packet_refs_bytes
            + payload.len()
            + 1024 * 1024,
        "snapshot peak exceeds owned metadata/packet refs plus one output buffer"
    );
}
